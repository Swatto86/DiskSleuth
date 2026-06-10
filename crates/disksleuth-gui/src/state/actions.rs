/// User-triggered actions on completed scan results: export, Recycle Bin
/// deletion, duplicate detection, old-file analysis, and scan history.
use super::{AppState, MAX_SCAN_HISTORY};
use disksleuth_core::analysis::{
    analyse_file_types, collect_candidates, find_duplicates_among, find_stale_files,
    DuplicateGroup, ScanSnapshot,
};
use disksleuth_core::model::size::{format_count, format_size};
use disksleuth_core::model::{FileTree, NodeIndex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Maximum stale files computed for the old-files window.
const MAX_OLD_FILES: usize = 200;

/// Signature shared by the tree export writers (CSV / JSON).
type ExportWriter = fn(&FileTree, &std::path::Path) -> anyhow::Result<u64>;

/// A transient status-bar message describing the outcome of an action.
#[derive(Debug, Clone)]
pub struct StatusFlash {
    pub text: String,
    pub is_error: bool,
}

impl StatusFlash {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// Export file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

/// Handle to an in-flight background duplicate search.
pub struct DuplicateScan {
    /// Delivers the final result exactly once.
    pub rx: crossbeam_channel::Receiver<Vec<DuplicateGroup>>,
    /// Set to stop hashing early (partial groups are still delivered).
    pub cancel: Arc<AtomicBool>,
    /// Total candidate files to hash.
    pub files_total: usize,
    /// Candidates hashed so far (written from rayon worker threads).
    pub files_done: Arc<AtomicUsize>,
}

impl AppState {
    // ── Export ──────────────────────────────────────────────────────────

    /// Export the completed scan tree to a timestamped file in the user's
    /// Documents folder (falling back to the temp directory).
    ///
    /// The outcome is stored in [`status_flash`](Self::status_flash) for the
    /// status bar to display.
    pub fn export(&mut self, format: ExportFormat) {
        let Some(ref tree) = self.tree else {
            self.status_flash = Some(StatusFlash::error(
                "Export failed: no completed scan to export",
            ));
            return;
        };

        let dir = std::env::var_os("USERPROFILE")
            .map(|p| std::path::PathBuf::from(p).join("Documents"))
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let (ext, write): (&str, ExportWriter) = match format {
            ExportFormat::Csv => ("csv", disksleuth_core::analysis::export_tree_csv),
            ExportFormat::Json => ("json", disksleuth_core::analysis::export_tree_json),
        };
        let path = dir.join(format!(
            "DiskSleuth_{}.{ext}",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));

        self.status_flash = Some(match write(tree, &path) {
            Ok(rows) => StatusFlash::info(format!(
                "Exported {} rows to {}",
                format_count(rows),
                path.display()
            )),
            Err(e) => StatusFlash::error(format!("Export failed: {e}")),
        });
    }

    /// Convenience wrapper kept for the CSV toolbar shortcut and tests.
    pub fn export_csv(&mut self) {
        self.export(ExportFormat::Csv);
    }

    // ── Recycle Bin deletion ────────────────────────────────────────────

    /// Ask for confirmation before deleting `target` to the Recycle Bin.
    ///
    /// Only valid on a completed scan; scan roots cannot be deleted.
    pub fn request_delete(&mut self, target: NodeIndex) {
        let Some(ref tree) = self.tree else { return };
        if target.idx() >= tree.len() || tree.node(target).is_deleted {
            return;
        }
        if tree.node(target).parent.is_none() {
            self.status_flash = Some(StatusFlash::error("Cannot delete a scan root"));
            return;
        }
        self.pending_delete = Some(target);
    }

    /// Move the pending node to the Recycle Bin and update the results.
    pub fn confirm_pending_delete(&mut self) {
        let Some(target) = self.pending_delete.take() else {
            return;
        };
        let Some(ref tree) = self.tree else { return };
        if target.idx() >= tree.len() || tree.node(target).is_deleted {
            return;
        }
        let node = tree.node(target);
        if node.parent.is_none() {
            return; // root — request_delete already rejects this
        }
        let name = node.name.to_string();
        let freed = node.size;
        let path = tree.full_path(target);

        match disksleuth_core::platform::move_to_recycle_bin(std::path::Path::new(&path)) {
            Ok(()) => {
                self.purge_node_from_results(target);
                self.status_flash = Some(StatusFlash::info(format!(
                    "Moved \"{name}\" to Recycle Bin — {} freed",
                    format_size(freed)
                )));
            }
            Err(e) => {
                self.status_flash = Some(StatusFlash::error(format!("Delete failed: {e}")));
            }
        }
    }

    /// Remove a node (and its subtree) from the in-memory results after the
    /// on-disk item is gone: tombstone it, refresh aggregates and caches,
    /// and rebuild the visible rows. Pure state update — no file I/O.
    pub fn purge_node_from_results(&mut self, target: NodeIndex) {
        // Re-root the treemap if it was showing (part of) the deleted subtree.
        if let (Some(tm_root), Some(ref tree)) = (self.treemap_root, &self.tree) {
            if tm_root.idx() < tree.len() && tree.is_in_subtree(tm_root, target) {
                self.treemap_root = tree.node(target).parent;
            }
        }

        let Some(tree) = self.tree.as_mut() else {
            return;
        };
        if target.idx() >= tree.len() {
            return;
        }
        tree.remove_subtree(target);
        tree.aggregate_sizes();
        self.file_type_stats = Some(analyse_file_types(tree));

        // Indices inside the removed subtree are now tombstones: drop every
        // cached reference that could point at them.
        self.selected_node = None;
        self.treemap_back.clear();
        self.treemap_forward.clear();
        self.old_files = None;
        self.duplicates = None;

        self.resort_visible_rows();
    }

    // ── Old files ───────────────────────────────────────────────────────

    /// (Re)compute the old-files cache for the current threshold.
    pub fn refresh_old_files(&mut self) {
        if let Some(ref tree) = self.tree {
            self.old_files = Some(find_stale_files(
                tree,
                self.old_files_min_age_days,
                MAX_OLD_FILES,
            ));
        }
    }

    // ── Duplicate detection ─────────────────────────────────────────────

    /// Start a background duplicate search over the completed tree.
    ///
    /// Candidate collection (pure tree walk) happens here on the UI thread;
    /// the expensive hashing runs on a named worker thread that reports
    /// progress through shared atomics and delivers the result via a
    /// single-shot channel drained by [`process_duplicate_messages`].
    pub fn start_duplicate_scan(&mut self) {
        self.cancel_duplicate_scan();
        let Some(ref tree) = self.tree else { return };

        let candidates = collect_candidates(tree, self.duplicate_min_size);
        let files_total = candidates.len();
        let cancel = Arc::new(AtomicBool::new(false));
        let files_done = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = crossbeam_channel::bounded(1);

        let cancel_worker = cancel.clone();
        let done_worker = files_done.clone();
        let spawned = std::thread::Builder::new()
            .name("disksleuth-duplicates".into())
            .spawn(move || {
                let groups = find_duplicates_among(candidates, &cancel_worker, |done, _| {
                    done_worker.store(done, Ordering::Relaxed);
                });
                let _ = tx.send(groups);
            });

        if spawned.is_err() {
            self.status_flash = Some(StatusFlash::error(
                "Duplicate search failed: could not spawn worker thread",
            ));
            return;
        }

        self.duplicates = None;
        self.duplicate_scan = Some(DuplicateScan {
            rx,
            cancel,
            files_total,
            files_done,
        });
    }

    /// Stop an in-flight duplicate search (no-op when none is running).
    pub fn cancel_duplicate_scan(&mut self) {
        if let Some(ref scan) = self.duplicate_scan {
            scan.cancel.store(true, Ordering::Relaxed);
        }
        self.duplicate_scan = None;
    }

    /// Poll the duplicate-search channel. Returns `true` on completion so
    /// the UI repaints with the results.
    pub fn process_duplicate_messages(&mut self) -> bool {
        let Some(ref scan) = self.duplicate_scan else {
            return false;
        };
        match scan.rx.try_recv() {
            Ok(groups) => {
                self.duplicates = Some(groups);
                self.duplicate_scan = None;
                true
            }
            Err(crossbeam_channel::TryRecvError::Empty) => false,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                // Worker died without a result (panic). Clear the handle so
                // the window stops showing a progress bar forever.
                self.duplicate_scan = None;
                self.status_flash =
                    Some(StatusFlash::error("Duplicate search stopped unexpectedly"));
                true
            }
        }
    }

    // ── Scan history ────────────────────────────────────────────────────

    /// Record a snapshot of a freshly completed scan, evicting the oldest
    /// entry beyond [`MAX_SCAN_HISTORY`].
    pub(super) fn record_snapshot(&mut self, tree: &FileTree) {
        let snapshot = ScanSnapshot::from_tree(
            tree,
            self.scan_root_path.clone(),
            self.scan_duration.unwrap_or_default(),
        );
        if self.scan_history.len() >= MAX_SCAN_HISTORY {
            self.scan_history.remove(0);
        }
        self.scan_history.push(snapshot);
        // Indices may have shifted — reset the comparison selection to the
        // default (previous scan of the same root vs the new one).
        self.history_baseline = None;
        self.history_compare = None;
        self.history_diff = None;
    }

    /// Resolve the snapshot pair to compare.
    ///
    /// Unset sides fall back to sensible defaults: compare = the newest
    /// snapshot, baseline = the most recent earlier scan of the same root.
    pub fn history_pair(&self) -> Option<(usize, usize)> {
        let len = self.scan_history.len();
        if len < 2 {
            return None;
        }
        let compare = self.history_compare.filter(|&i| i < len).unwrap_or(len - 1);
        let baseline = self.history_baseline.filter(|&i| i < len).or_else(|| {
            let root = &self.scan_history[compare].root_path;
            self.scan_history[..compare]
                .iter()
                .rposition(|s| s.root_path.eq_ignore_ascii_case(root))
        })?;
        (baseline != compare).then_some((baseline, compare))
    }

    /// Refresh the cached history diff for the active pair, if needed.
    pub fn refresh_history_diff(&mut self) {
        let Some(pair) = self.history_pair() else {
            self.history_diff = None;
            return;
        };
        if self.history_diff.as_ref().map(|(k, _)| *k) == Some(pair) {
            return; // cache hit
        }
        let diff = disksleuth_core::analysis::compare_snapshots(
            &self.scan_history[pair.0],
            &self.scan_history[pair.1],
        );
        self.history_diff = Some((pair, diff));
    }
}
