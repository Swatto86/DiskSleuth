/// Parallel directory walker using `jwalk` — the Tier 2 (fallback) scanner.
///
/// This scanner does not require admin privileges and works on any filesystem.
/// It uses `jwalk`'s rayon-backed parallel traversal to walk the directory tree
/// at high speed, writing nodes into a shared `LiveTree` so the UI can render
/// the tree in real time.
///
/// # Lock-contention mitigation
///
/// The naive approach acquires one `RwLock::write()` per node — 2M nodes means
/// 2M lock cycles.  Instead, nodes are accumulated in a local `Vec<PendingEntry>`
/// and flushed to the shared tree under a **single write lock per batch**.  At
/// `BATCH_SIZE = 2_000`, a 2M-node scan requires only ~1_000 write-lock
/// acquisitions, a 2_000× reduction.
///
/// **NodeIndex pre-computation**: because `FileTree::add_node` merely appends to
/// a `Vec`, the index for any pending entry is deterministic:
/// `NodeIndex(arena_base + position_in_pending_vec)`.  Directory entries are
/// registered in `dir_map` with their pre-computed index immediately, so child
/// entries processed in the same batch find their parent without an extra lock.
use crate::model::{FileNode, NodeIndex};
use crate::scanner::progress::ScanProgress;
use crate::scanner::LiveTree;
use compact_str::CompactString;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

/// Number of nodes to accumulate locally before flushing to the shared LiveTree.
///
/// Larger values reduce lock pressure further but delay live-view updates.
/// 2_000 is a good balance: ~1_000 flush events on a 2M-node drive vs the
/// default 2M, while each flush holds the write lock for < 1 ms.
const BATCH_SIZE: usize = 2_000;

/// A node buffered in the local pending vec before being flushed under one lock.
struct PendingEntry {
    node: FileNode,
    parent_idx: NodeIndex,
    /// Pre-computed index this entry will receive on insertion.
    /// Equals `arena_base + position_in_pending_vec` at push time.
    pre_idx: NodeIndex,
}

/// Drain `pending` into the shared tree under a single write-lock acquisition.
///
/// Returns the number of entries flushed so the caller can advance `arena_base`.
/// Verifies in debug builds that pre-computed indices are correct.
#[inline]
fn flush_pending(live_tree: &LiveTree, pending: &mut Vec<PendingEntry>) -> usize {
    let count = pending.len();
    if count == 0 {
        return 0;
    }
    let mut tree = live_tree.write();
    for pe in pending.drain(..) {
        let idx = tree.add_node(pe.node);
        debug_assert_eq!(
            idx, pe.pre_idx,
            "NodeIndex pre-computation mismatch — arena_base drift"
        );
        tree.add_child(pe.parent_idx, idx);
    }
    count
}

/// Scan a directory tree using parallel directory walking.
///
/// Writes nodes into `live_tree` incrementally. The UI can read from this
/// tree each frame to show a real-time view while the scan runs.
pub fn scan_parallel(
    root_path: PathBuf,
    progress_tx: Sender<ScanProgress>,
    cancel_flag: Arc<AtomicBool>,
    live_tree: LiveTree,
) {
    let start = Instant::now();
    let mut error_count: u64 = 0;

    // The root display name: for drives like "C:\", show "C:"; for folders, show the name.
    let root_name = root_display_name(&root_path);

    // Create the root node in the shared tree.
    let root_idx = {
        let mut tree = live_tree.write();
        tree.add_root(CompactString::new(&root_name))
    };

    // Map from directory path to its NodeIndex in the arena.
    //
    // This map is scan-lifetime only (dropped when `scan_parallel` returns).
    // Memory is proportional to unique directory count. If an entry is ever
    // not found, `ensure_ancestors` recreates the missing chain from root, so
    // correctness is maintained even if this map were to be evicted.
    let mut dir_map: HashMap<PathBuf, NodeIndex> = HashMap::with_capacity(100_000);
    dir_map.insert(root_path.clone(), root_idx);

    let mut files_found: u64 = 0;
    let mut dirs_found: u64 = 1; // count the root
    let mut total_size: u64 = 0;
    let mut update_counter: u64 = 0;

    // Local batch buffer.  Flushed every BATCH_SIZE entries (or on demand
    // before ensure_ancestors / progress snapshots).
    let mut pending: Vec<PendingEntry> = Vec::with_capacity(BATCH_SIZE + 64);

    // Tracks how many nodes are in the arena at the start of the current batch.
    // Root node (index 0) was just inserted, so we start at 1.
    let mut arena_base: usize = 1;

    // Configure jwalk for maximum throughput.
    let walker = jwalk::WalkDir::new(&root_path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(num_cpus::get()));

    for entry_result in walker {
        // Check cancellation every 1000 entries.
        update_counter += 1;
        if update_counter.is_multiple_of(1_000) && cancel_flag.load(Ordering::Relaxed) {
            flush_pending(&live_tree, &mut pending);
            let _ = progress_tx.send(ScanProgress::Cancelled);
            return;
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                // Flush pending batch first so the arena is in a clean state
                // before the individual write lock for the error node.
                let flushed = flush_pending(&live_tree, &mut pending);
                arena_base += flushed;

                error_count += 1;
                // jwalk errors are typically access-denied on directories.
                let err_path = err
                    .path()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let msg = format!("{err}");

                // Add an error placeholder node if we can determine the parent.
                if let Some(entry_path) = err.path() {
                    if let Some(parent_path) = entry_path.parent() {
                        let parent_idx = dir_map.get(&parent_path.to_path_buf()).copied();
                        if let Some(pidx) = parent_idx {
                            let name = entry_path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "<access denied>".to_string());
                            let error_node = FileNode::new_error(
                                CompactString::new(&name),
                                true, // assume dir since jwalk only errors on dir reads
                                Some(pidx),
                            );
                            let mut tree = live_tree.write();
                            let idx = tree.add_node(error_node);
                            tree.add_child(pidx, idx);
                            arena_base += 1;
                        }
                    }
                }

                let _ = progress_tx.send(ScanProgress::Error {
                    path: err_path,
                    message: msg,
                });
                continue;
            }
        };

        let path = entry.path();

        // Skip the root itself (already created).
        if path == root_path {
            continue;
        }

        // Determine the parent directory's NodeIndex.
        let parent_path = match path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let parent_idx = match dir_map.get(&parent_path) {
            Some(&idx) => idx,
            None => {
                // Flush current batch first so the live tree is fully up to date
                // before ensure_ancestors creates new ancestor nodes.
                let flushed = flush_pending(&live_tree, &mut pending);
                arena_base += flushed;
                // Parent not in map — create ancestor chain lazily.
                ensure_ancestors(
                    &live_tree,
                    &mut dir_map,
                    &parent_path,
                    &root_path,
                    root_idx,
                    &mut arena_base,
                )
            }
        };

        // Pre-compute the NodeIndex this entry will receive on flush.
        let pre_idx = NodeIndex::new(arena_base + pending.len());
        let file_name = entry.file_name().to_string_lossy();

        if entry.file_type().is_dir() {
            let dir_node =
                FileNode::new_dir(CompactString::new(file_name.as_ref()), Some(parent_idx));

            // Register in dir_map immediately with the pre-computed index so that
            // child entries in the same batch can find this directory as their parent.
            dir_map.insert(path.clone(), pre_idx);
            pending.push(PendingEntry {
                node: dir_node,
                parent_idx,
                pre_idx,
            });
            dirs_found += 1;
        } else {
            // Stat the file outside the lock — this is the expensive syscall.
            let (size, modified) = match std::fs::symlink_metadata(&path) {
                Ok(meta) => (meta.len(), meta.modified().ok()),
                Err(err) => {
                    error_count += 1;
                    // Error node goes through the batch like any other entry.
                    let error_node = FileNode::new_error(
                        CompactString::new(file_name.as_ref()),
                        false,
                        Some(parent_idx),
                    );
                    pending.push(PendingEntry {
                        node: error_node,
                        parent_idx,
                        pre_idx,
                    });
                    let _ = progress_tx.send(ScanProgress::Error {
                        path: path.to_string_lossy().to_string(),
                        message: format!("{err}"),
                    });
                    // Check batch capacity (continue, not return, so we flush below).
                    if pending.len() >= BATCH_SIZE {
                        let flushed = flush_pending(&live_tree, &mut pending);
                        arena_base += flushed;
                    }
                    continue;
                }
            };

            let mut file_node = FileNode::new_file(
                CompactString::new(file_name.as_ref()),
                size,
                Some(parent_idx),
            );
            file_node.modified = modified;
            pending.push(PendingEntry {
                node: file_node,
                parent_idx,
                pre_idx,
            });
            files_found += 1;
            total_size += size;
        }

        // Flush when the batch is full — one write lock for BATCH_SIZE nodes.
        if pending.len() >= BATCH_SIZE {
            let flushed = flush_pending(&live_tree, &mut pending);
            arena_base += flushed;
        }

        // Send progress updates roughly every 5000 entries.
        if update_counter.is_multiple_of(5_000) {
            // Flush first so live sizes include these new nodes.
            let flushed = flush_pending(&live_tree, &mut pending);
            arena_base += flushed;
            // Run a lightweight aggregation (no expensive file-sort) so live
            // sizes are visible without blocking the scanner for long.
            {
                let mut tree = live_tree.write();
                tree.aggregate_sizes_live();
            }

            let _ = progress_tx.send(ScanProgress::Update {
                files_found,
                dirs_found,
                total_size,
                current_path: path.to_string_lossy().into_owned(),
            });
        }
    }

    // Flush any remaining buffered nodes before aggregation.
    flush_pending(&live_tree, &mut pending);

    // Final aggregation pass.
    debug!(
        "Scan walk complete: {} files, {} dirs in {:?}. Running aggregation...",
        files_found,
        dirs_found,
        start.elapsed()
    );

    {
        let mut tree = live_tree.write();
        tree.aggregate_sizes();
    }

    let duration = start.elapsed();
    debug!("Aggregation complete. Total duration: {duration:?}");

    // Send Complete — the tree is already in the shared LiveTree.
    // We send a flag-only message; the UI will take ownership of the LiveTree.
    let _ = progress_tx.send(ScanProgress::Complete {
        duration,
        error_count,
    });
}

/// Ensure all ancestor directories exist in the tree and dir_map.
///
/// Called only when a parent path is missing from `dir_map` (rare, typically
/// caused by jwalk ordering on very wide directory trees).  Each new ancestor
/// is inserted individually with its own write lock.  `arena_base` is
/// incremented for each inserted node so the caller's pre-computation stays
/// accurate.
fn ensure_ancestors(
    live_tree: &LiveTree,
    dir_map: &mut HashMap<PathBuf, NodeIndex>,
    target: &Path,
    root_path: &Path,
    root_idx: NodeIndex,
    arena_base: &mut usize,
) -> NodeIndex {
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut current = target.to_path_buf();

    while !dir_map.contains_key(&current) && current != *root_path {
        missing.push(current.clone());
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }

    let mut parent_idx = dir_map.get(&current).copied().unwrap_or(root_idx);

    for ancestor in missing.into_iter().rev() {
        let name = ancestor
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let dir_node = FileNode::new_dir(CompactString::new(&name), Some(parent_idx));
        let idx = {
            let mut tree = live_tree.write();
            let idx = tree.add_node(dir_node);
            tree.add_child(parent_idx, idx);
            idx
        };
        *arena_base += 1;
        dir_map.insert(ancestor, idx);
        parent_idx = idx;
    }

    parent_idx
}

/// Derive a display name for the scan root.
fn root_display_name(path: &Path) -> String {
    if let Some(name) = path.file_name() {
        name.to_string_lossy().to_string()
    } else {
        let s = path.to_string_lossy();
        s.trim_end_matches('\\').to_string()
    }
}
