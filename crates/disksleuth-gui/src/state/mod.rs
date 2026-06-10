/// Application state management.
///
/// Centralises all mutable state that the UI reads and writes. The scan
/// thread communicates via channels; state updates happen in
/// `process_scan_messages()` which runs once per frame.
///
/// During scanning, the tree view reads from a **shared `LiveTree`**
/// (`Arc<RwLock<FileTree>>`) so results appear in real time.
///
/// The `impl AppState` blocks are split by concern:
/// - `mod.rs` — state struct, scan lifecycle, monitor, drives
/// - `rows.rs` — visible-row building, expansion, sorting, selection
/// - `treemap_nav.rs` — treemap navigation history
/// - `actions.rs` — export, deletion, duplicate/old-file analysis, history
mod actions;
mod rows;
mod treemap_nav;

pub use actions::{DuplicateScan, ExportFormat, StatusFlash};

use disksleuth_core::analysis::{
    analyse_file_types, CategoryStats, DuplicateGroup, ScanSnapshot, StaleFile,
};
use disksleuth_core::model::{FileTree, NodeIndex, SortKey};
use disksleuth_core::monitor::{MonitorHandle, WriteEvent};
use disksleuth_core::platform::DriveInfo;
use disksleuth_core::scanner::progress::ScanProgress;
use disksleuth_core::scanner::{LiveTree, ScanHandle};
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPhase {
    /// Idle — no scan in progress, possibly showing previous results.
    Idle,
    /// Scanning — progress bar and live counters.
    Scanning,
    /// Scan complete — results are available.
    Results,
}

/// A row in the flattened visible-rows list for the virtualised TreeView.
#[derive(Clone, Debug)]
pub struct VisibleRow {
    /// Index into the `FileTree` arena.
    pub node_index: NodeIndex,
    /// Nesting depth (0 = root).
    pub depth: u16,
    /// Whether this directory is currently expanded (meaningless for files).
    pub is_expanded: bool,
}

/// Maximum number of scan-progress messages drained from the channel per frame.
///
/// Prevents a backlog (e.g. after the window was hidden) from blocking the
/// render thread for a perceptible duration when it is eventually shown again.
const MAX_MESSAGES_PER_FRAME: usize = 300;

/// Maximum entries in the treemap back/forward navigation history stacks.
///
/// Prevents unbounded growth under rapid or scripted navigation.
pub(crate) const MAX_NAV_HISTORY: usize = 50;

/// Maximum monitor messages drained per frame.
///
/// Prevents a write-heavy directory (e.g. a compiler output folder) from
/// stalling the render thread when the message backlog is processed after
/// the window is restored.  The monitor channel is bounded(2048), so this
/// caps worst-case per-frame work to 200 eviction/insert operations.
const MAX_MONITOR_MESSAGES_PER_FRAME: usize = 200;

/// Maximum rows in the virtualised tree-view visible-rows list.
///
/// Each `VisibleRow` is 8 bytes (NodeIndex u32 + depth u16 + bool + pad).
/// At 500 000 rows that is ~4 MB — acceptable — but prevents runaway
/// growth on fully-expanded multi-million-node trees. Users can collapse
/// nodes to explore deeper subtrees.
pub(crate) const MAX_VISIBLE_ROWS: usize = 500_000;

/// Maximum number of per-file scan errors retained in `AppState::scan_errors`.
///
/// Bounds memory and rendering cost for the error list. Errors beyond this
/// limit are counted (via `scan_error_count`) but not stored individually.
/// 1 000 errors gives more than enough context to diagnose access-denied
/// patterns without unbounded growth on heavily-restricted volumes.
const MAX_SCAN_ERRORS: usize = 1_000;

/// Maximum completed-scan snapshots retained for history comparison.
pub(crate) const MAX_SCAN_HISTORY: usize = 16;

/// All application state.
pub struct AppState {
    // ── Drives ─────────────────────────────────────────
    pub drives: Vec<DriveInfo>,
    pub selected_drive_index: Option<usize>,
    /// Whether the process is running with administrator privileges.
    ///
    /// Computed once at startup via `is_elevated()` and cached here.
    /// Elevation status never changes for the lifetime of a process, so
    /// caching avoids an `OpenProcessToken` + `GetTokenInformation` +
    /// `CloseHandle` round-trip on every render frame.
    pub is_elevated: bool,

    // ── Scan ───────────────────────────────────────────
    pub phase: AppPhase,
    pub scan_handle: Option<ScanHandle>,
    pub scan_files_found: u64,
    pub scan_dirs_found: u64,
    pub scan_total_size: u64,
    pub scan_current_path: String,
    /// The path the current/last scan was started on (drive root or folder).
    pub scan_root_path: String,
    pub scan_error_count: u64,
    pub scan_duration: Option<Duration>,
    /// True if the most recent scan was cancelled (partial results).
    pub scan_was_cancelled: bool,
    /// True if the scanner is using MFT direct reader (Tier 1).
    pub scan_is_mft: bool,
    /// True if the process is running with admin privileges.
    pub scan_is_elevated: bool,

    // ── Results ────────────────────────────────────────
    /// The completed scan tree (set once scan finishes).
    pub tree: Option<FileTree>,
    /// The live tree reference during scanning (for real-time view).
    pub live_tree: Option<LiveTree>,
    pub visible_rows: Vec<VisibleRow>,
    pub selected_node: Option<NodeIndex>,
    /// Tracks node count from the last live-tree snapshot so we know
    /// when to rebuild visible rows.
    live_tree_last_len: usize,

    // ── Tree-view ordering & scrolling ─────────────────
    /// Column the tree view is ordered by.
    pub sort_key: SortKey,
    pub sort_ascending: bool,
    /// One-shot request for the tree view to scroll the selected row into
    /// view (set by selection sync, keyboard navigation, and reveal).
    pub scroll_to_selected: bool,

    // ── Treemap navigation ─────────────────────────────
    /// The directory currently shown as root of the treemap.
    pub treemap_root: Option<NodeIndex>,
    /// Back stack for treemap navigation (VecDeque for O(1) front eviction).
    pub treemap_back: VecDeque<NodeIndex>,
    /// Forward stack for treemap navigation (VecDeque for O(1) front eviction).
    pub treemap_forward: VecDeque<NodeIndex>,

    // ── UI state ───────────────────────────────────────
    pub show_errors: bool,
    pub show_about: bool,
    pub scan_errors: Vec<(String, String)>,
    /// Outcome of the most recent export/delete action, shown in the status bar.
    pub status_flash: Option<StatusFlash>,
    /// `true` = dark mode (default), `false` = light mode.
    pub dark_mode: bool,

    // ── Pre-computed analysis cache ────────────────────
    /// File-type breakdown — computed once after scan completes,
    /// not on every render frame.
    pub file_type_stats: Option<Vec<CategoryStats>>,

    // ── Analysis windows ───────────────────────────────
    pub show_largest_window: bool,
    pub show_old_files_window: bool,
    /// Age threshold for the old-files window.
    pub old_files_min_age_days: u64,
    /// Cached stale-file results (invalidated on scan/delete/threshold change).
    pub old_files: Option<Vec<StaleFile>>,
    pub show_duplicates_window: bool,
    /// Minimum file size considered by the duplicate finder.
    pub duplicate_min_size: u64,
    /// Completed duplicate-detection results.
    pub duplicates: Option<Vec<DuplicateGroup>>,
    /// In-flight background duplicate search, if any.
    pub duplicate_scan: Option<DuplicateScan>,

    // ── Scan history ───────────────────────────────────
    /// Snapshots of completed (non-cancelled) scans, oldest first.
    pub scan_history: Vec<ScanSnapshot>,
    pub show_history_window: bool,
    /// Snapshot indices chosen for comparison (baseline = older side).
    pub history_baseline: Option<usize>,
    pub history_compare: Option<usize>,
    /// Cached diff keyed by the (baseline, compare) pair it was computed for.
    pub history_diff: Option<((usize, usize), Vec<disksleuth_core::analysis::DirDelta>)>,

    // ── Deletion ───────────────────────────────────────
    /// Node awaiting user confirmation before Recycle Bin deletion.
    pub pending_delete: Option<NodeIndex>,

    // ── Live write monitor ─────────────────────────────
    /// Whether the monitor bottom panel is visible.
    pub show_monitor_panel: bool,
    /// Whether the monitor is actively watching.
    pub monitor_active: bool,
    /// Drive/path currently being monitored.
    pub monitor_path: String,
    /// Aggregated write events (capped at `MAX_MONITOR_ENTRIES`).
    pub monitor_entries: Vec<WriteEvent>,
    /// Handle to the background monitor thread.
    pub monitor_handle: Option<MonitorHandle>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Create initial application state.
    pub fn new() -> Self {
        let drives = disksleuth_core::platform::enumerate_drives();
        let selected = if drives.is_empty() { None } else { Some(0) };
        // Compute elevation once at startup — never changes within a process.
        let is_elevated = disksleuth_core::platform::is_elevated();

        Self {
            drives,
            selected_drive_index: selected,
            is_elevated,
            phase: AppPhase::Idle,
            scan_handle: None,
            scan_files_found: 0,
            scan_dirs_found: 0,
            scan_total_size: 0,
            scan_current_path: String::new(),
            scan_root_path: String::new(),
            scan_error_count: 0,
            scan_duration: None,
            scan_was_cancelled: false,
            scan_is_mft: false,
            scan_is_elevated: false,
            tree: None,
            live_tree: None,
            visible_rows: Vec::new(),
            selected_node: None,
            live_tree_last_len: 0,
            sort_key: SortKey::default(),
            sort_ascending: false,
            scroll_to_selected: false,
            treemap_root: None,
            treemap_back: VecDeque::new(),
            treemap_forward: VecDeque::new(),
            show_errors: false,
            show_about: false,
            scan_errors: Vec::new(),
            status_flash: None,
            dark_mode: true,
            file_type_stats: None,
            show_largest_window: false,
            show_old_files_window: false,
            old_files_min_age_days: 365,
            old_files: None,
            show_duplicates_window: false,
            duplicate_min_size: disksleuth_core::analysis::DEFAULT_MIN_DUPLICATE_SIZE,
            duplicates: None,
            duplicate_scan: None,
            scan_history: Vec::new(),
            show_history_window: false,
            history_baseline: None,
            history_compare: None,
            history_diff: None,
            pending_delete: None,
            show_monitor_panel: false,
            monitor_active: false,
            monitor_path: String::new(),
            monitor_entries: Vec::new(),
            monitor_handle: None,
        }
    }

    /// Start a scan of the selected drive or path.
    ///
    /// Cancels any in-progress scan before starting the new one.  The old
    /// scan thread sets its cancel flag and stops at the next check point
    /// (~1000 entries); it does not block here.  Without this call the old
    /// thread would become an orphan, continuing to run and consume CPU
    /// until its own scan completes.
    pub fn start_scan(&mut self, path: std::path::PathBuf) {
        // Cancel any running scan so its thread stops cleanly.
        // This is safe to call even when no scan is in progress.
        self.cancel_scan();
        self.cancel_duplicate_scan();

        // Reset scan state.
        self.phase = AppPhase::Scanning;
        self.scan_files_found = 0;
        self.scan_dirs_found = 0;
        self.scan_total_size = 0;
        self.scan_current_path = path.to_string_lossy().into_owned();
        self.scan_root_path = self.scan_current_path.clone();
        self.scan_error_count = 0;
        self.scan_duration = None;
        self.scan_was_cancelled = false;
        self.scan_is_mft = false;
        self.scan_is_elevated = false;
        self.scan_errors.clear();
        self.status_flash = None;
        self.tree = None;
        self.file_type_stats = None;
        self.old_files = None;
        self.duplicates = None;
        self.pending_delete = None;
        self.visible_rows.clear();
        self.selected_node = None;
        self.live_tree_last_len = 0;
        self.treemap_root = None;
        self.treemap_back.clear();
        self.treemap_forward.clear();

        let handle = disksleuth_core::scanner::start_scan(path);
        self.live_tree = Some(handle.live_tree.clone());
        self.scan_handle = Some(handle);
    }

    /// Cancel any running scan.
    pub fn cancel_scan(&mut self) {
        if let Some(ref handle) = self.scan_handle {
            handle.cancel();
        }
    }

    /// Re-enumerate drives, keeping the selection on the same drive letter
    /// when it still exists.
    ///
    /// The drive list can shrink (e.g. a USB stick was unplugged), so the
    /// selection must never be carried over as a raw index — a stale index
    /// would panic on the next `drives[idx]` access.
    pub fn refresh_drives(&mut self) {
        let previous_letter = self
            .selected_drive_index
            .and_then(|i| self.drives.get(i))
            .map(|d| d.letter.clone());

        self.drives = disksleuth_core::platform::enumerate_drives();

        self.selected_drive_index = previous_letter
            .and_then(|letter| self.drives.iter().position(|d| d.letter == letter))
            .or(if self.drives.is_empty() {
                None
            } else {
                Some(0)
            });
    }

    /// Start the live write monitor on `path`.
    ///
    /// Stops any previously running monitor first.
    pub fn start_monitor(&mut self, path: std::path::PathBuf) {
        self.stop_monitor();
        let path_str = path.to_string_lossy().into_owned();
        let handle = disksleuth_core::monitor::start_monitor(path);
        self.monitor_handle = Some(handle);
        self.monitor_active = true;
        self.monitor_path = path_str;
        self.monitor_entries.clear();
    }

    /// Stop the live write monitor.
    pub fn stop_monitor(&mut self) {
        if let Some(ref h) = self.monitor_handle {
            h.stop();
        }
        self.monitor_handle = None;
        self.monitor_active = false;
    }

    /// Drain pending monitor messages and update `monitor_entries`.
    ///
    /// Called once per frame; returns `true` if the UI should repaint.
    /// Capped at [`MAX_MONITOR_MESSAGES_PER_FRAME`] messages per call so that
    /// a backlog (e.g. after window restore during an active compilation)
    /// cannot stall the render thread.
    pub fn process_monitor_messages(&mut self) -> bool {
        let handle = match &self.monitor_handle {
            Some(h) => h,
            None => return false,
        };

        let mut repaint = false;
        let mut disconnected = false;
        let mut messages_this_frame = 0usize;
        while messages_this_frame < MAX_MONITOR_MESSAGES_PER_FRAME {
            let msg = match handle.receiver.try_recv() {
                Ok(m) => m,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    // The monitor thread exited on its own (e.g. the watched
                    // directory handle failed). Flag it so the UI stops
                    // showing an active monitor that no longer exists.
                    disconnected = true;
                    break;
                }
            };
            messages_this_frame += 1;
            repaint = true;
            match msg {
                disksleuth_core::monitor::MonitorMessage::FileChanged(path) => {
                    // Update existing entry or insert new one.
                    if let Some(entry) = self.monitor_entries.iter_mut().find(|e| e.path == path) {
                        entry.hit_count += 1;
                        entry.last_seen = chrono::Local::now();
                    } else {
                        // Evict oldest entry when at capacity.
                        if self.monitor_entries.len()
                            >= disksleuth_core::monitor::MAX_MONITOR_ENTRIES
                        {
                            // Remove the entry with the oldest last_seen timestamp.
                            if let Some(pos) = self
                                .monitor_entries
                                .iter()
                                .enumerate()
                                .min_by_key(|(_, e)| e.last_seen)
                                .map(|(i, _)| i)
                            {
                                self.monitor_entries.remove(pos);
                            }
                        }
                        self.monitor_entries
                            .push(disksleuth_core::monitor::WriteEvent {
                                path,
                                hit_count: 1,
                                last_seen: chrono::Local::now(),
                            });
                    }
                }
            }
        }

        if disconnected {
            self.monitor_handle = None;
            self.monitor_active = false;
            repaint = true;
        }
        repaint
    }

    /// Get a reference to the best available tree.
    ///
    /// During scanning returns the live tree (via read lock snapshot).
    /// After scan completion returns the final tree.
    /// Returns `None` if no data is available yet.
    pub fn current_tree(&self) -> Option<&FileTree> {
        self.tree.as_ref()
    }

    /// Process pending scan progress messages. Called once per frame.
    ///
    /// Returns `true` if the UI should repaint (new data arrived).
    pub fn process_scan_messages(&mut self) -> bool {
        let handle = match &self.scan_handle {
            Some(h) => h,
            None => return false,
        };

        let mut repaint = false;

        // Drain available messages without blocking, subject to a per-frame
        // budget so a large backlog (window restored after being hidden) cannot
        // stall the render thread for a perceptible duration.
        let mut messages_this_frame = 0usize;
        while messages_this_frame < MAX_MESSAGES_PER_FRAME {
            let msg = match handle.progress_rx.try_recv() {
                Ok(m) => m,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    // The scan thread died without sending Complete/Cancelled
                    // (e.g. it panicked). Without this branch the UI would
                    // stay in the Scanning phase forever. Salvage whatever
                    // was scanned and surface the failure.
                    self.scan_was_cancelled = true;
                    self.status_flash = Some(StatusFlash::error(
                        "Scan stopped unexpectedly — showing partial results",
                    ));
                    self.finalize_scan(false);
                    return true;
                }
            };
            messages_this_frame += 1;
            repaint = true;
            match msg {
                ScanProgress::ScanTier {
                    is_mft,
                    is_elevated,
                } => {
                    self.scan_is_mft = is_mft;
                    self.scan_is_elevated = is_elevated;
                }
                ScanProgress::Update {
                    files_found,
                    dirs_found,
                    total_size,
                    current_path,
                } => {
                    self.scan_files_found = files_found;
                    self.scan_dirs_found = dirs_found;
                    self.scan_total_size = total_size;
                    self.scan_current_path = current_path;
                }
                ScanProgress::Error { path, message } => {
                    self.scan_error_count += 1;
                    if self.scan_errors.len() < MAX_SCAN_ERRORS {
                        self.scan_errors.push((path, message));
                    }
                }
                ScanProgress::Complete {
                    duration,
                    error_count,
                } => {
                    self.scan_error_count = error_count;
                    self.scan_duration = Some(duration);
                    self.finalize_scan(true);
                    return true;
                }
                ScanProgress::Cancelled => {
                    self.scan_was_cancelled = true;
                    self.finalize_scan(false);
                    return true;
                }
            }
        }

        // During scanning, update visible rows from the live tree
        // when new nodes have appeared.
        if self.phase == AppPhase::Scanning {
            // Clone the Arc (cheap refcount bump) to avoid borrowing self.
            if let Some(lt) = self.live_tree.clone() {
                let tree = lt.read();
                let current_len = tree.len();
                if current_len != self.live_tree_last_len && current_len > 0 {
                    self.live_tree_last_len = current_len;
                    self.rebuild_live_visible_rows(&tree);
                    repaint = true;
                }
            }
        }

        repaint
    }

    /// Take ownership of the live tree and switch to the Results phase.
    ///
    /// Shared by the `Complete`, `Cancelled`, and disconnected-channel paths.
    /// `completed == true` means the scanner finished normally: its final
    /// aggregation pass already ran and the scan is recorded in the history.
    /// Otherwise (cancel / scanner death) the tree is re-aggregated here, as
    /// the scanner's interrupted exit paths skip the full aggregation pass.
    fn finalize_scan(&mut self, completed: bool) {
        self.phase = AppPhase::Results;

        if let Some(lt) = self.live_tree.take() {
            // Try to unwrap the Arc; if still shared, clone.
            let mut tree = parking_lot::RwLock::into_inner(
                std::sync::Arc::try_unwrap(lt)
                    .unwrap_or_else(|arc| parking_lot::RwLock::new(arc.read().clone())),
            );
            if !completed {
                tree.aggregate_sizes();
            }
            self.build_initial_visible_rows(&tree);
            // Pre-compute analysis cache so chart panel never runs
            // analyse_file_types on the render thread.
            self.file_type_stats = Some(analyse_file_types(&tree));
            if completed {
                self.record_snapshot(&tree);
            }
            self.tree = Some(tree);
        }

        self.scan_handle = None;
    }
}
