/// Parallel directory walker using `jwalk` — the Tier 2 (fallback) scanner.
///
/// This scanner does not require admin privileges and works on any filesystem.
/// It uses `jwalk`'s rayon-backed parallel traversal to walk the directory tree
/// at high speed, writing nodes into a shared `LiveTree` so the UI can render
/// the tree in real time.

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
    let mut dir_map: HashMap<PathBuf, NodeIndex> = HashMap::with_capacity(100_000);
    dir_map.insert(root_path.clone(), root_idx);

    let mut files_found: u64 = 0;
    let mut dirs_found: u64 = 1; // count the root
    let mut total_size: u64 = 0;
    let mut update_counter: u64 = 0;

    // Configure jwalk for maximum throughput.
    let walker = jwalk::WalkDir::new(&root_path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(num_cpus::get()));

    for entry_result in walker {
        // Check cancellation every 1000 entries.
        update_counter += 1;
        if update_counter % 1000 == 0 && cancel_flag.load(Ordering::Relaxed) {
            let _ = progress_tx.send(ScanProgress::Cancelled);
            return;
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                error_count += 1;
                // jwalk errors are typically access-denied on directories.
                // Extract what path info we can from the error.
                let err_path = err.path().map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let msg = format!("{err}");

                // Add an error placeholder node if we can determine the parent.
                if let Some(entry_path) = err.path() {
                    if let Some(parent_path) = entry_path.parent() {
                        let parent_idx = dir_map.get(&parent_path.to_path_buf()).copied();
                        if let Some(pidx) = parent_idx {
                            let name = entry_path.file_name()
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
        let file_name = entry
            .file_name()
            .to_string_lossy()
            .to_string();

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
                // Parent not in map — create ancestor chain lazily.
                ensure_ancestors(&live_tree, &mut dir_map, &parent_path, &root_path, root_idx)
            }
        };

        if entry.file_type().is_dir() {
            let dir_node = FileNode::new_dir(CompactString::new(&file_name), Some(parent_idx));
            let dir_idx = {
                let mut tree = live_tree.write();
                let idx = tree.add_node(dir_node);
                tree.add_child(parent_idx, idx);
                idx
            };
            dir_map.insert(path.clone(), dir_idx);
            dirs_found += 1;
        } else {
            let (size, modified) = match std::fs::symlink_metadata(&path) {
                Ok(meta) => {
                    let sz = meta.len();
                    let mod_time = meta.modified().ok();
                    (sz, mod_time)
                }
                Err(err) => {
                    error_count += 1;
                    // Create an error placeholder node so the user can see what failed.
                    let error_node = FileNode::new_error(
                        CompactString::new(&file_name),
                        false,
                        Some(parent_idx),
                    );
                    {
                        let mut tree = live_tree.write();
                        let idx = tree.add_node(error_node);
                        tree.add_child(parent_idx, idx);
                    }
                    let _ = progress_tx.send(ScanProgress::Error {
                        path: path.to_string_lossy().to_string(),
                        message: format!("{err}"),
                    });
                    continue;
                }
            };

            let mut file_node = FileNode::new_file(
                CompactString::new(&file_name),
                size,
                Some(parent_idx),
            );
            file_node.modified = modified;
            {
                let mut tree = live_tree.write();
                let file_idx = tree.add_node(file_node);
                tree.add_child(parent_idx, file_idx);
            }

            files_found += 1;
            total_size += size;
        }

        // Send progress updates roughly every 5000 entries.
        if update_counter % 5000 == 0 {
            // Run a quick aggregation so live sizes are visible.
            {
                let mut tree = live_tree.write();
                tree.aggregate_sizes();
            }

            let _ = progress_tx.send(ScanProgress::Update {
                files_found,
                dirs_found,
                total_size,
                current_path: path.to_string_lossy().into_owned(),
            });
        }
    }

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
fn ensure_ancestors(
    live_tree: &LiveTree,
    dir_map: &mut HashMap<PathBuf, NodeIndex>,
    target: &Path,
    root_path: &Path,
    root_idx: NodeIndex,
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

    let mut parent_idx = dir_map
        .get(&current)
        .copied()
        .unwrap_or(root_idx);

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
