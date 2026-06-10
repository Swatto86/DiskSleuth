/// Visible-row management for the virtualised tree view: building, live
/// rebuilds, expansion, column sorting, and keyboard selection movement.
use super::{AppState, VisibleRow, MAX_VISIBLE_ROWS};
use disksleuth_core::model::{FileTree, NodeIndex, SortKey};
use std::collections::HashSet;

impl AppState {
    /// Build the initial visible rows from the tree roots (post-scan).
    ///
    /// Expands root + its immediate children. Respects [`MAX_VISIBLE_ROWS`] so
    /// drives with millions of direct root-level entries don't allocate an
    /// unbounded Vec.
    pub(super) fn build_initial_visible_rows(&mut self, tree: &FileTree) {
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
            let children = tree.children_sorted(root_idx, self.sort_key, self.sort_ascending);
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
    /// stays expanded. Roots are always expanded.
    pub(super) fn rebuild_live_visible_rows(&mut self, tree: &FileTree) {
        rebuild_rows_preserving_expansion(
            &mut self.visible_rows,
            tree,
            self.sort_key,
            self.sort_ascending,
        );
    }

    /// Toggle expansion of a node at the given row index in visible_rows.
    ///
    /// Works with both the final results tree and the live tree during scanning.
    pub fn toggle_expand(&mut self, row_index: usize) {
        // Use disjoint field borrows to satisfy the borrow checker:
        // tree/live_tree are borrowed immutably while visible_rows is borrowed mutably.
        let (key, asc) = (self.sort_key, self.sort_ascending);
        if let Some(ref tree) = self.tree {
            toggle_expand_inner(&mut self.visible_rows, row_index, tree, key, asc);
        } else if let Some(ref lt) = self.live_tree {
            let tree = lt.read();
            toggle_expand_inner(&mut self.visible_rows, row_index, &tree, key, asc);
        }
    }

    /// Change the active sort column.
    ///
    /// Clicking the active column again flips the direction; switching
    /// columns applies that column's natural default (names ascending,
    /// sizes/counts descending). Rows are rebuilt preserving expansion.
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort_key == key {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_key = key;
            self.sort_ascending = matches!(key, SortKey::Name);
        }
        self.resort_visible_rows();
        self.scroll_to_selected = self.selected_node.is_some();
    }

    /// Rebuild the visible rows with the current sort, preserving expansion.
    pub fn resort_visible_rows(&mut self) {
        let (key, asc) = (self.sort_key, self.sort_ascending);
        if let Some(ref tree) = self.tree {
            rebuild_rows_preserving_expansion(&mut self.visible_rows, tree, key, asc);
        } else if let Some(lt) = self.live_tree.clone() {
            let tree = lt.read();
            rebuild_rows_preserving_expansion(&mut self.visible_rows, &tree, key, asc);
        }
    }

    /// Row index of the currently selected node, if it is visible.
    pub fn selected_row_index(&self) -> Option<usize> {
        let selected = self.selected_node?;
        self.visible_rows
            .iter()
            .position(|r| r.node_index == selected)
    }

    /// Move the selection `delta` rows (keyboard ↑/↓, j/k, PgUp/PgDn,
    /// Home/End map to large deltas).
    ///
    /// With no current selection, movement starts from just before the first
    /// row (downward) or just after the last (upward), so ↓ selects the
    /// first row and ↑ the last.
    pub fn move_selection(&mut self, delta: isize) {
        if self.visible_rows.is_empty() {
            return;
        }
        let last = self.visible_rows.len() as isize - 1;
        let start = match self.selected_row_index() {
            Some(i) => i as isize,
            None if delta >= 0 => -1,
            None => last + 1,
        };
        let next = start.saturating_add(delta).clamp(0, last) as usize;
        self.selected_node = Some(self.visible_rows[next].node_index);
        self.scroll_to_selected = true;
    }

    /// Keyboard → / l: expand the selected directory, or step into its
    /// first child when already expanded.
    pub fn expand_selection(&mut self) {
        let Some(row) = self.selected_row_index() else {
            return;
        };
        if self.visible_rows[row].is_expanded {
            let child = row + 1;
            if child < self.visible_rows.len()
                && self.visible_rows[child].depth == self.visible_rows[row].depth + 1
            {
                self.selected_node = Some(self.visible_rows[child].node_index);
                self.scroll_to_selected = true;
            }
        } else {
            self.toggle_expand(row); // no-op for files
        }
    }

    /// Keyboard ← / h: collapse the selected directory, or jump to the
    /// parent when already collapsed (or a file).
    pub fn collapse_selection(&mut self) {
        let Some(row) = self.selected_row_index() else {
            return;
        };
        if self.visible_rows[row].is_expanded {
            self.toggle_expand(row);
        } else {
            let depth = self.visible_rows[row].depth;
            if depth == 0 {
                return;
            }
            if let Some(parent_row) = (0..row).rev().find(|&i| self.visible_rows[i].depth < depth) {
                self.selected_node = Some(self.visible_rows[parent_row].node_index);
                self.scroll_to_selected = true;
            }
        }
    }

    /// Ensure a node is visible in the tree view by expanding all its
    /// ancestors, then request a scroll to it. Called when the treemap
    /// selection changes (selection sync) and after analysis-window jumps.
    pub fn reveal_node_in_tree(&mut self, target: NodeIndex) {
        let (key, asc) = (self.sort_key, self.sort_ascending);
        if let Some(ref tree) = self.tree {
            reveal_node_inner(&mut self.visible_rows, target, tree, key, asc);
        } else if let Some(ref lt) = self.live_tree {
            let guard = lt.read();
            reveal_node_inner(&mut self.visible_rows, target, &guard, key, asc);
        }
        self.scroll_to_selected = true;
    }
}

/// Rebuild `visible_rows` from the tree roots via an explicit-stack DFS,
/// keeping previously expanded directories expanded (roots always are).
///
/// An explicit stack (not recursion) prevents call-stack exhaustion on
/// deeply nested directory trees, and [`MAX_VISIBLE_ROWS`] bounds memory on
/// fully-expanded multi-million-node trees.
fn rebuild_rows_preserving_expansion(
    visible_rows: &mut Vec<VisibleRow>,
    tree: &FileTree,
    key: SortKey,
    ascending: bool,
) {
    // Remember which nodes were expanded.
    //
    // Pre-size the HashSet to the upper bound of expanded rows + roots
    // so the initial inserts never trigger a rehash.  The lower bound is
    // 8 so tiny trees (first few hundred nodes) also get a good allocation.
    let approx_expanded =
        visible_rows.iter().filter(|r| r.is_expanded).count().max(8) + tree.roots.len();
    let mut expanded: HashSet<NodeIndex> = HashSet::with_capacity(approx_expanded);
    expanded.extend(
        visible_rows
            .iter()
            .filter(|r| r.is_expanded)
            .map(|r| r.node_index),
    );
    expanded.extend(tree.roots.iter().copied());

    visible_rows.clear();

    // DFS stack: children pushed in reverse so the first child pops first,
    // preserving top-to-bottom visual order.
    let mut stack: Vec<(NodeIndex, u16)> = Vec::with_capacity(64);
    for &root_idx in tree.roots.iter().rev() {
        stack.push((root_idx, 0));
    }

    while let Some((current_idx, depth)) = stack.pop() {
        if visible_rows.len() >= MAX_VISIBLE_ROWS {
            return;
        }

        let is_expanded = expanded.contains(&current_idx) && tree.node(current_idx).is_dir;
        visible_rows.push(VisibleRow {
            node_index: current_idx,
            depth,
            is_expanded,
        });

        if is_expanded {
            let next_depth = depth.saturating_add(1);
            for child_idx in tree
                .children_sorted(current_idx, key, ascending)
                .into_iter()
                .rev()
            {
                stack.push((child_idx, next_depth));
            }
        }
    }
}

/// Toggle-expand implementation operating on the visible_rows vec directly.
///
/// Free function to avoid `&mut self` / `&self.tree` borrow conflict.
fn toggle_expand_inner(
    visible_rows: &mut Vec<VisibleRow>,
    row_index: usize,
    tree: &FileTree,
    key: SortKey,
    ascending: bool,
) {
    if row_index >= visible_rows.len() {
        return;
    }
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
        let children = tree.children_sorted(node_idx, key, ascending);
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

/// Expand ancestors of `target` so it becomes visible.
///
/// Free function to allow disjoint borrows (`&mut visible_rows` vs borrowing
/// `AppState.tree` immutably).
fn reveal_node_inner(
    visible_rows: &mut Vec<VisibleRow>,
    target: NodeIndex,
    tree: &FileTree,
    key: SortKey,
    ascending: bool,
) {
    // Already visible? Nothing to expand.
    if visible_rows.iter().any(|r| r.node_index == target) {
        return;
    }

    // Build ancestor chain from target up to root.
    let mut ancestors: Vec<NodeIndex> = Vec::new();
    let mut cursor = target;
    while let Some(p) = tree.nodes[cursor.idx()].parent {
        ancestors.push(p);
        cursor = p;
    }
    ancestors.reverse(); // root → ... → direct parent

    // Expand each ancestor in order so the target becomes reachable.
    for ancestor in &ancestors {
        if let Some(row_idx) = visible_rows.iter().position(|r| r.node_index == *ancestor) {
            if !visible_rows[row_idx].is_expanded {
                toggle_expand_inner(visible_rows, row_idx, tree, key, ascending);
            }
        }
    }
}
