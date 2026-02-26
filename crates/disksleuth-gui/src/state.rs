/// Application state management.
///
/// Centralises all mutable state that the UI reads and writes.
/// The scan thread communicates via channels; state updates happen
/// in `process_scan_messages()` which runs once per frame.
///
/// During scanning, the tree view reads from a **shared `LiveTree`**
/// (`Arc<RwLock<FileTree>>`) so results appear in real time.
use disksleuth_core::model::{FileTree, NodeIndex};
use disksleuth_core::monitor::{MonitorHandle, WriteEvent};
use disksleuth_core::platform::DriveInfo;
use disksleuth_core::scanner::progress::ScanProgress;
use disksleuth_core::scanner::{LiveTree, ScanHandle};
use std::time::Duration;

/// The current phase of the application.
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
const MAX_NAV_HISTORY: usize = 50;

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
const MAX_VISIBLE_ROWS: usize = 500_000;

/// All application state.
pub struct AppState {
    // ── Drives ─────────────────────────────────────────
    pub drives: Vec<DriveInfo>,
    pub selected_drive_index: Option<usize>,

    // ── Scan ───────────────────────────────────────────
    pub phase: AppPhase,
    pub scan_handle: Option<ScanHandle>,
    pub scan_files_found: u64,
    pub scan_dirs_found: u64,
    pub scan_total_size: u64,
    pub scan_current_path: String,
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

    // ── Treemap navigation ─────────────────────────────
    /// The directory currently shown as root of the treemap.
    pub treemap_root: Option<NodeIndex>,
    /// Back stack for treemap navigation.
    pub treemap_back: Vec<NodeIndex>,
    /// Forward stack for treemap navigation.
    pub treemap_forward: Vec<NodeIndex>,

    // ── UI state ───────────────────────────────────────
    pub tree_scroll_offset: f32,
    pub show_errors: bool,
    pub show_about: bool,
    pub scan_errors: Vec<(String, String)>,
    pub context_menu_node: Option<NodeIndex>,
    // ── Theme ──────────────────────────────────────────────
    /// `true` = dark mode (default), `false` = light mode.
    pub dark_mode: bool,

    // ── Live write monitor ─────────────────────────────────
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

        Self {
            drives,
            selected_drive_index: selected,
            phase: AppPhase::Idle,
            scan_handle: None,
            scan_files_found: 0,
            scan_dirs_found: 0,
            scan_total_size: 0,
            scan_current_path: String::new(),
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
            treemap_root: None,
            treemap_back: Vec::new(),
            treemap_forward: Vec::new(),
            tree_scroll_offset: 0.0,
            show_errors: false,
            show_about: false,
            scan_errors: Vec::new(),
            context_menu_node: None,
            dark_mode: true,
            show_monitor_panel: false,
            monitor_active: false,
            monitor_path: String::new(),
            monitor_entries: Vec::new(),
            monitor_handle: None,
        }
    }

    /// Start a scan of the selected drive or path.
    pub fn start_scan(&mut self, path: std::path::PathBuf) {
        // Reset scan state.
        self.phase = AppPhase::Scanning;
        self.scan_files_found = 0;
        self.scan_dirs_found = 0;
        self.scan_total_size = 0;
        self.scan_current_path = path.to_string_lossy().into_owned();
        self.scan_error_count = 0;
        self.scan_duration = None;
        self.scan_was_cancelled = false;
        self.scan_is_mft = false;
        self.scan_is_elevated = false;
        self.scan_errors.clear();
        self.tree = None;
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
        let mut messages_this_frame = 0usize;
        while messages_this_frame < MAX_MONITOR_MESSAGES_PER_FRAME {
            let msg = match handle.receiver.try_recv() {
                Ok(m) => m,
                Err(_) => break,
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
                Err(_) => break,
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
                    if self.scan_errors.len() < 1000 {
                        self.scan_errors.push((path, message));
                    }
                }
                ScanProgress::Complete {
                    duration,
                    error_count,
                } => {
                    self.scan_error_count = error_count;
                    self.scan_duration = Some(duration);
                    self.phase = AppPhase::Results;

                    // Take ownership of the final tree from the LiveTree.
                    if let Some(lt) = self.live_tree.take() {
                        // Try to unwrap the Arc; if still shared, clone.
                        let tree = parking_lot::RwLock::into_inner(
                            std::sync::Arc::try_unwrap(lt)
                                .unwrap_or_else(|arc| parking_lot::RwLock::new(arc.read().clone())),
                        );
                        self.build_initial_visible_rows(&tree);
                        self.tree = Some(tree);
                    }

                    self.scan_handle = None;
                    return true;
                }
                ScanProgress::Cancelled => {
                    self.scan_was_cancelled = true;
                    self.phase = AppPhase::Results;

                    // Preserve whatever has been scanned so far.
                    if let Some(lt) = self.live_tree.take() {
                        let tree = parking_lot::RwLock::into_inner(
                            std::sync::Arc::try_unwrap(lt)
                                .unwrap_or_else(|arc| parking_lot::RwLock::new(arc.read().clone())),
                        );
                        self.build_initial_visible_rows(&tree);
                        self.tree = Some(tree);
                    }

                    self.scan_handle = None;
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

    /// Build the initial visible rows from the tree roots (post-scan).
    ///
    /// Expands root + its immediate children. Respects [`MAX_VISIBLE_ROWS`] so
    /// drives with millions of direct root-level entries don't allocate an
    /// unbounded Vec.
    fn build_initial_visible_rows(&mut self, tree: &FileTree) {
        self.visible_rows.clear();

        for &root_idx in &tree.roots {
            if self.visible_rows.len() >= MAX_VISIBLE_ROWS {
                break;
            }
            self.visible_rows.push(VisibleRow {
                node_index: root_idx,
                depth: 0,
                is_expanded: true,
            });

            // Expand root's children by default.
            let children = tree.children_sorted_by_size(root_idx);
            for child_idx in children {
                if self.visible_rows.len() >= MAX_VISIBLE_ROWS {
                    break;
                }
                self.visible_rows.push(VisibleRow {
                    node_index: child_idx,
                    depth: 1,
                    is_expanded: false,
                });
            }
        }
    }

    /// Rebuild visible rows from the live tree during scanning.
    ///
    /// Preserves expansion state: any directory that was previously expanded
    /// stays expanded. New directories at depth 0–1 are auto-expanded.
    fn rebuild_live_visible_rows(&mut self, tree: &FileTree) {
        // Remember which nodes were expanded.
        let mut expanded_set: std::collections::HashSet<NodeIndex> = self
            .visible_rows
            .iter()
            .filter(|r| r.is_expanded)
            .map(|r| r.node_index)
            .collect();

        // Always expand roots.
        for &root_idx in &tree.roots {
            expanded_set.insert(root_idx);
        }

        self.visible_rows.clear();

        for &root_idx in &tree.roots {
            self.build_live_rows_recursive(tree, root_idx, 0, &expanded_set);
        }
    }

    /// Recursively build visible rows, respecting expanded state.
    ///
    /// Stops inserting once [`MAX_VISIBLE_ROWS`] is reached so the live tree
    /// view does not allocate unbounded memory on fully-expanded deep trees.
    fn build_live_rows_recursive(
        &mut self,
        tree: &FileTree,
        node_idx: NodeIndex,
        depth: u16,
        expanded: &std::collections::HashSet<NodeIndex>,
    ) {
        // Hard cap: stop adding rows once the limit is reached.
        if self.visible_rows.len() >= MAX_VISIBLE_ROWS {
            return;
        }

        let is_expanded = expanded.contains(&node_idx) && tree.node(node_idx).is_dir;

        self.visible_rows.push(VisibleRow {
            node_index: node_idx,
            depth,
            is_expanded,
        });

        if is_expanded {
            let children = tree.children_sorted_by_size(node_idx);
            for child_idx in children {
                self.build_live_rows_recursive(tree, child_idx, depth + 1, expanded);
            }
        }
    }

    /// Toggle expansion of a node at the given row index in visible_rows.
    ///
    /// Works with both the final results tree and the live tree during scanning.
    pub fn toggle_expand(&mut self, row_index: usize) {
        // Use disjoint field borrows to satisfy the borrow checker:
        // tree/live_tree are borrowed immutably while visible_rows is borrowed mutably.
        if let Some(ref tree) = self.tree {
            toggle_expand_inner(&mut self.visible_rows, row_index, tree);
        } else if let Some(ref lt) = self.live_tree {
            let tree = lt.read();
            toggle_expand_inner(&mut self.visible_rows, row_index, &tree);
        }
    }

    /// Ensure a node is visible in the tree view by expanding all its ancestors.
    /// This is called when the treemap selection changes to sync the tree view.
    pub fn reveal_node_in_tree(&mut self, target: NodeIndex) {
        let tree = if let Some(ref t) = self.tree {
            t.clone()
        } else if let Some(ref lt) = self.live_tree {
            lt.read().clone()
        } else {
            return;
        };

        // Already visible? Just scroll to it.
        if self.visible_rows.iter().any(|r| r.node_index == target) {
            // Find the row index and set scroll offset so it's visible.
            if let Some(pos) = self
                .visible_rows
                .iter()
                .position(|r| r.node_index == target)
            {
                // Set scroll offset to show this row (ROW_HEIGHT = 24.0).
                let row_y = pos as f32 * 24.0;
                // Only scroll if the row is likely off-screen.
                if (row_y - self.tree_scroll_offset).abs() > 600.0 {
                    self.tree_scroll_offset = (row_y - 120.0).max(0.0);
                }
            }
            return;
        }

        // Build ancestor chain from target up to root.
        let mut ancestors: Vec<NodeIndex> = Vec::new();
        let mut cursor = target;
        while let Some(p) = tree.nodes[cursor.idx()].parent {
            ancestors.push(p);
            cursor = p;
        }
        ancestors.reverse(); // root → ... → parent

        // Expand each ancestor in order.
        for ancestor in &ancestors {
            // Find the ancestor in visible_rows.
            if let Some(row_idx) = self
                .visible_rows
                .iter()
                .position(|r| r.node_index == *ancestor)
            {
                if !self.visible_rows[row_idx].is_expanded {
                    toggle_expand_inner(&mut self.visible_rows, row_idx, &tree);
                }
            }
        }

        // Scroll to the target row.
        if let Some(pos) = self
            .visible_rows
            .iter()
            .position(|r| r.node_index == target)
        {
            let row_y = pos as f32 * 24.0;
            self.tree_scroll_offset = (row_y - 120.0).max(0.0);
        }
    }
}

/// Toggle-expand implementation operating on the visible_rows vec directly.
///
/// Free function to avoid `&mut self` / `&self.tree` borrow conflict.
fn toggle_expand_inner(visible_rows: &mut Vec<VisibleRow>, row_index: usize, tree: &FileTree) {
    let row = &visible_rows[row_index];
    let node = tree.node(row.node_index);

    if !node.is_dir {
        return; // files can't be expanded
    }

    if row.is_expanded {
        // COLLAPSE: remove all descendants (rows with depth > this row's depth)
        // that follow consecutively.
        let parent_depth = row.depth;
        let remove_start = row_index + 1;
        let mut remove_end = remove_start;
        while remove_end < visible_rows.len() && visible_rows[remove_end].depth > parent_depth {
            remove_end += 1;
        }
        visible_rows.drain(remove_start..remove_end);
        visible_rows[row_index].is_expanded = false;
    } else {
        // EXPAND: insert sorted children immediately after this row.
        // Respect MAX_VISIBLE_ROWS: only add as many children as headroom allows.
        let node_idx = row.node_index;
        let child_depth = row.depth + 1;
        let children = tree.children_sorted_by_size(node_idx);
        let insert_pos = row_index + 1;
        let headroom = MAX_VISIBLE_ROWS.saturating_sub(visible_rows.len());

        let new_rows: Vec<VisibleRow> = children
            .into_iter()
            .take(headroom)
            .map(|child_idx| VisibleRow {
                node_index: child_idx,
                depth: child_depth,
                is_expanded: false,
            })
            .collect();

        // Splice the children into the visible rows list.
        visible_rows.splice(insert_pos..insert_pos, new_rows);
        visible_rows[row_index].is_expanded = true;
    }
}

impl AppState {
    /// Navigate the treemap into a directory, pushing current root onto back stack.
    pub fn treemap_navigate_to(&mut self, node: NodeIndex) {
        // Determine the current effective root (explicit or the tree's first root).
        let current = self.treemap_root.or_else(|| {
            self.tree
                .as_ref()
                .and_then(|t| t.roots.first().copied())
                .or_else(|| {
                    self.live_tree
                        .as_ref()
                        .and_then(|lt| lt.read().roots.first().copied())
                })
        });
        if let Some(cur) = current {
            if cur != node {
                // Evict oldest entry when the history stack is at capacity.
                if self.treemap_back.len() >= MAX_NAV_HISTORY {
                    self.treemap_back.remove(0);
                }
                self.treemap_back.push(cur);
            }
        }
        self.treemap_forward.clear();
        self.treemap_root = Some(node);
    }

    /// Go back in treemap navigation history.
    pub fn treemap_go_back(&mut self) {
        if let Some(prev) = self.treemap_back.pop() {
            if let Some(cur) = self.treemap_root {
                if self.treemap_forward.len() >= MAX_NAV_HISTORY {
                    self.treemap_forward.remove(0);
                }
                self.treemap_forward.push(cur);
            }
            self.treemap_root = Some(prev);
        }
    }

    /// Go forward in treemap navigation history.
    pub fn treemap_go_forward(&mut self) {
        if let Some(next) = self.treemap_forward.pop() {
            if let Some(cur) = self.treemap_root {
                if self.treemap_back.len() >= MAX_NAV_HISTORY {
                    self.treemap_back.remove(0);
                }
                self.treemap_back.push(cur);
            }
            self.treemap_root = Some(next);
        }
    }

    /// Navigate treemap up to parent directory.
    pub fn treemap_go_up(&mut self, tree: &FileTree) {
        if let Some(root) = self.treemap_root {
            if let Some(parent) = tree.nodes[root.idx()].parent {
                // Cap the back stack consistent with all other nav methods.
                if self.treemap_back.len() >= MAX_NAV_HISTORY {
                    self.treemap_back.remove(0);
                }
                self.treemap_back.push(root);
                self.treemap_forward.clear();
                self.treemap_root = Some(parent);
            }
        }
    }
}
