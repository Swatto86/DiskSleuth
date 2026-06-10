/// Treemap navigation history (back / forward / up).
use super::{AppState, MAX_NAV_HISTORY};
use disksleuth_core::model::NodeIndex;

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
                    self.treemap_back.pop_front();
                }
                self.treemap_back.push_back(cur);
            }
        }
        self.treemap_forward.clear();
        self.treemap_root = Some(node);
    }

    /// Go back in treemap navigation history.
    pub fn treemap_go_back(&mut self) {
        if let Some(prev) = self.treemap_back.pop_back() {
            if let Some(cur) = self.treemap_root {
                if self.treemap_forward.len() >= MAX_NAV_HISTORY {
                    self.treemap_forward.pop_front();
                }
                self.treemap_forward.push_back(cur);
            }
            self.treemap_root = Some(prev);
        }
    }

    /// Go forward in treemap navigation history.
    pub fn treemap_go_forward(&mut self) {
        if let Some(next) = self.treemap_forward.pop_back() {
            if let Some(cur) = self.treemap_root {
                if self.treemap_back.len() >= MAX_NAV_HISTORY {
                    self.treemap_back.pop_front();
                }
                self.treemap_back.push_back(cur);
            }
            self.treemap_root = Some(next);
        }
    }

    /// Navigate treemap up to parent directory.
    ///
    /// Reads the parent index from the stored tree directly, eliminating
    /// the need for an external `&FileTree` parameter (and the clone that
    /// was previously required at the call site in `app.rs`).
    pub fn treemap_go_up(&mut self) {
        let parent_opt = match self.treemap_root {
            None => return,
            Some(root) => {
                if let Some(ref tree) = self.tree {
                    tree.nodes[root.idx()].parent
                } else if let Some(ref lt) = self.live_tree {
                    lt.read().nodes[root.idx()].parent
                } else {
                    None
                }
            }
        };
        if let Some(parent) = parent_opt {
            let cur = self.treemap_root.unwrap();
            if self.treemap_back.len() >= MAX_NAV_HISTORY {
                self.treemap_back.pop_front();
            }
            self.treemap_back.push_back(cur);
            self.treemap_forward.clear();
            self.treemap_root = Some(parent);
        }
    }
}
