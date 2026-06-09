/// Arena-backed file tree with O(n) bottom-up size aggregation.
///
/// All nodes live in a single `Vec<FileNode>`. Relationships between nodes
/// use `NodeIndex` (a thin `u32` wrapper) rather than heap pointers, giving
/// cache-friendly traversal and trivial serialisation.
use super::file_node::{FileNode, NodeIndex};
use compact_str::CompactString;

/// The complete file tree produced by a scan.
#[derive(Debug, Clone)]
pub struct FileTree {
    /// Arena: every node in a flat, cache-friendly vector.
    pub nodes: Vec<FileNode>,

    /// Root node indices — one per scanned drive or folder.
    pub roots: Vec<NodeIndex>,

    /// Total logical size across all roots.
    pub total_size: u64,

    /// Indices of the N largest individual files, sorted descending by size.
    pub largest_files: Vec<NodeIndex>,

    /// Cached count of non-directory nodes (regular files + error file nodes).
    ///
    /// Set once per aggregation pass so the render thread can display file
    /// counts without iterating millions of nodes every frame.
    pub file_count: u64,
}

impl FileTree {
    /// Create an empty tree with pre-allocated capacity.
    ///
    /// `estimated_nodes` should be a rough upper bound (e.g. 1_000_000
    /// for a typical C: drive). The arena will grow if needed, but
    /// pre-allocation avoids repeated re-allocation during scanning.
    pub fn with_capacity(estimated_nodes: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(estimated_nodes),
            roots: Vec::new(),
            total_size: 0,
            largest_files: Vec::new(),
            file_count: 0,
        }
    }

    /// Allocate a new node in the arena and return its index.
    pub fn add_node(&mut self, node: FileNode) -> NodeIndex {
        let idx = NodeIndex::new(self.nodes.len());
        self.nodes.push(node);
        idx
    }

    /// Add a root directory to the tree.
    pub fn add_root(&mut self, name: CompactString) -> NodeIndex {
        let node = FileNode::new_dir(name, None);
        let idx = self.add_node(node);
        self.roots.push(idx);
        idx
    }

    /// Attach `child` as a child of `parent`, prepending to the sibling list.
    ///
    /// This is O(1) — new children are inserted at the head of the linked list.
    pub fn add_child(&mut self, parent: NodeIndex, child: NodeIndex) {
        let old_first = self.nodes[parent.idx()].first_child;
        self.nodes[child.idx()].next_sibling = old_first;
        self.nodes[child.idx()].parent = Some(parent);
        self.nodes[parent.idx()].first_child = Some(child);
    }

    /// Lightweight aggregation pass used during live scanning.
    ///
    /// Performs the same bottom-up size/count/percentage roll-up as
    /// [`aggregate_sizes`] but deliberately skips the expensive
    /// `compute_largest_files` sort pass (O(n log n) over all file nodes).
    /// Call this every N entries while the scan is running; call the full
    /// [`aggregate_sizes`] only once on the completed tree.
    pub fn aggregate_sizes_live(&mut self) {
        self.aggregate_sizes_inner(false);
    }

    /// Compute sizes, descendant counts, and percentages in a bottom-up pass.
    ///
    /// When every node's parent precedes it in the arena (the parallel walker
    /// guarantees this by construction), a single cache-friendly *reverse*
    /// index pass rolls children up before their parents. Trees built from
    /// MFT records do **not** guarantee that order — NTFS reuses record
    /// numbers, so a child's record can precede its parent's — and fall back
    /// to an explicit-stack post-order traversal. Both paths are O(n).
    ///
    /// Safe to call repeatedly (e.g. during a live scan) — directory sizes
    /// are reset before each pass so values don't accumulate.
    ///
    /// Calls `compute_largest_files` after aggregation. For incremental live
    /// use, prefer [`aggregate_sizes_live`] to avoid the O(n log n) sort.
    pub fn aggregate_sizes(&mut self) {
        self.aggregate_sizes_inner(true);
    }

    /// Internal implementation shared by [`aggregate_sizes`] and
    /// [`aggregate_sizes_live`].
    fn aggregate_sizes_inner(&mut self, compute_largest: bool) {
        // Reset directory aggregation fields and simultaneously count files
        // and verify the parent-before-child arena ordering. Both piggy-back
        // on the O(n) reset loop so no dedicated extra pass is needed.
        let mut file_count = 0u64;
        let mut parents_precede_children = true;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if node.is_dir {
                node.size = 0;
                node.allocated_size = 0;
                node.descendant_count = 0;
            } else {
                file_count += 1;
            }
            if let Some(parent_idx) = node.parent {
                if parent_idx.idx() >= i {
                    parents_precede_children = false;
                }
            }
        }
        self.file_count = file_count;

        if parents_precede_children {
            // Reverse pass: children before parents.
            for i in (0..self.nodes.len()).rev() {
                let node = &self.nodes[i];
                if !node.is_dir {
                    // Leaf file — nothing to sum, but propagate to parent.
                    let size = node.size;
                    let alloc = node.allocated_size;
                    if let Some(parent_idx) = node.parent {
                        self.nodes[parent_idx.idx()].size += size;
                        self.nodes[parent_idx.idx()].allocated_size += alloc;
                        self.nodes[parent_idx.idx()].descendant_count += 1;
                    }
                } else {
                    // Directory — its size/count are already accumulated from children.
                    // Propagate upward to its own parent.
                    let size = self.nodes[i].size;
                    let alloc = self.nodes[i].allocated_size;
                    let desc = self.nodes[i].descendant_count;
                    if let Some(parent_idx) = self.nodes[i].parent {
                        self.nodes[parent_idx.idx()].size += size;
                        self.nodes[parent_idx.idx()].allocated_size += alloc;
                        self.nodes[parent_idx.idx()].descendant_count += desc;
                    }
                }
            }
        } else {
            self.aggregate_post_order();
        }

        // Compute percent_of_parent for every node.
        for i in 0..self.nodes.len() {
            let parent_size = self.nodes[i]
                .parent
                .map(|p| self.nodes[p.idx()].size)
                .unwrap_or(self.nodes[i].size); // roots use own size as denominator

            self.nodes[i].percent_of_parent = if parent_size > 0 {
                (self.nodes[i].size as f64 / parent_size as f64 * 100.0) as f32
            } else {
                0.0
            };
        }

        // Total size across all roots.
        self.total_size = self.roots.iter().map(|r| self.nodes[r.idx()].size).sum();

        // Build top-N largest files list — skipped during live incremental scans
        // because sorting all file indices is O(n log n) and too expensive to run
        // every N entries while the scan thread is actively inserting nodes.
        if compute_largest {
            self.compute_largest_files(100);
        }
    }

    /// Bottom-up roll-up for trees whose arena order does not satisfy the
    /// parent-before-child invariant (e.g. MFT scans on volumes where record
    /// numbers were reused).
    ///
    /// Walks each root with an explicit-stack post-order traversal over the
    /// `first_child` / `next_sibling` links, accumulating file sizes into
    /// their directory and propagating directory totals to the parent as the
    /// stack unwinds. O(n); the stack depth is bounded by the tree depth.
    fn aggregate_post_order(&mut self) {
        let roots = self.roots.clone();
        // Stack entries: (node arena index, next child to visit).
        let mut stack: Vec<(usize, Option<NodeIndex>)> = Vec::with_capacity(64);
        for root in roots {
            let r = root.idx();
            if !self.nodes[r].is_dir {
                continue; // roots are always directories; be defensive
            }
            stack.push((r, self.nodes[r].first_child));
            while let Some(&(dir_idx, cursor)) = stack.last() {
                match cursor {
                    Some(child) => {
                        let c = child.idx();
                        // Advance the cursor before descending.
                        stack.last_mut().expect("stack is non-empty").1 =
                            self.nodes[c].next_sibling;
                        if self.nodes[c].is_dir {
                            stack.push((c, self.nodes[c].first_child));
                        } else {
                            self.nodes[dir_idx].size += self.nodes[c].size;
                            self.nodes[dir_idx].allocated_size += self.nodes[c].allocated_size;
                            self.nodes[dir_idx].descendant_count += 1;
                        }
                    }
                    None => {
                        // All children processed — pop and propagate this
                        // directory's totals into the parent on the stack.
                        stack.pop();
                        if let Some(&(parent_idx, _)) = stack.last() {
                            let size = self.nodes[dir_idx].size;
                            let alloc = self.nodes[dir_idx].allocated_size;
                            let desc = self.nodes[dir_idx].descendant_count;
                            self.nodes[parent_idx].size += size;
                            self.nodes[parent_idx].allocated_size += alloc;
                            self.nodes[parent_idx].descendant_count += desc;
                        }
                    }
                }
            }
        }
    }

    /// Find the N largest individual files by size.
    ///
    /// Uses `select_nth_unstable_by` (O(n) average) to bring the top-N
    /// elements to the front, then sorts only those N elements (O(k log k)).
    /// This is significantly faster than a full O(n log n) sort when n >> k.
    fn compute_largest_files(&mut self, n: usize) {
        if n == 0 {
            self.largest_files.clear();
            return;
        }

        let mut file_indices: Vec<NodeIndex> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.is_dir)
            .map(|(i, _)| NodeIndex::new(i))
            .collect();

        if file_indices.len() <= n {
            // Smaller than the requested cap — full sort is fine.
            file_indices
                .sort_unstable_by(|a, b| self.nodes[b.idx()].size.cmp(&self.nodes[a.idx()].size));
        } else {
            // Partial selection: O(n) average to move top-n to the front,
            // then O(n log n) only on the small top-n slice.
            let pivot = n - 1;
            file_indices.select_nth_unstable_by(pivot, |a, b| {
                // Descending: larger files come first.
                self.nodes[b.idx()].size.cmp(&self.nodes[a.idx()].size)
            });
            file_indices.truncate(n);
            file_indices
                .sort_unstable_by(|a, b| self.nodes[b.idx()].size.cmp(&self.nodes[a.idx()].size));
        }

        self.largest_files = file_indices;
    }

    /// Reconstruct the full path for a node by walking up to the root.
    pub fn full_path(&self, index: NodeIndex) -> String {
        let mut segments = Vec::new();
        let mut current = Some(index);
        while let Some(idx) = current {
            segments.push(self.nodes[idx.idx()].name.as_str());
            current = self.nodes[idx.idx()].parent;
        }
        segments.reverse();

        // Join with backslash for Windows paths.

        // If the first segment looks like a drive letter (e.g. "C:"),
        // ensure we have C:\ not C:\\ at the start.
        segments.join("\\")
    }

    /// Get direct children of a node as a collected Vec, sorted by size descending.
    pub fn children_sorted_by_size(&self, parent: NodeIndex) -> Vec<NodeIndex> {
        let mut children = Vec::new();
        let mut child = self.nodes[parent.idx()].first_child;
        while let Some(idx) = child {
            children.push(idx);
            child = self.nodes[idx.idx()].next_sibling;
        }
        // Directories first, then by size descending.
        children.sort_unstable_by(|a, b| {
            let a_node = &self.nodes[a.idx()];
            let b_node = &self.nodes[b.idx()];
            b_node
                .is_dir
                .cmp(&a_node.is_dir)
                .then(b_node.size.cmp(&a_node.size))
        });
        children
    }

    /// Get direct children of a node (unsorted).
    pub fn children(&self, parent: NodeIndex) -> Vec<NodeIndex> {
        let mut children = Vec::new();
        let mut child = self.nodes[parent.idx()].first_child;
        while let Some(idx) = child {
            children.push(idx);
            child = self.nodes[idx.idx()].next_sibling;
        }
        children
    }

    /// Get the node at the given index.
    #[inline]
    pub fn node(&self, index: NodeIndex) -> &FileNode {
        &self.nodes[index.idx()]
    }

    /// Total number of nodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the tree contains no nodes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_aggregation() {
        let mut tree = FileTree::with_capacity(10);

        // Build: root -> dir -> (file_a: 100, file_b: 200)
        let root = tree.add_root(CompactString::new("C:"));
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("Users"), Some(root)));
        tree.add_child(root, dir);

        let file_a = tree.add_node(FileNode::new_file(
            CompactString::new("a.txt"),
            100,
            Some(dir),
        ));
        tree.add_child(dir, file_a);

        let file_b = tree.add_node(FileNode::new_file(
            CompactString::new("b.txt"),
            200,
            Some(dir),
        ));
        tree.add_child(dir, file_b);

        tree.aggregate_sizes();

        assert_eq!(tree.node(dir).size, 300);
        assert_eq!(tree.node(root).size, 300);
        assert_eq!(tree.node(dir).descendant_count, 2);
        assert_eq!(tree.node(root).descendant_count, 2);
        assert_eq!(tree.total_size, 300);
    }

    /// Regression test: trees built from MFT records can allocate a child in
    /// the arena *before* its parent (NTFS reuses record numbers). The old
    /// reverse-index pass propagated a directory's total to its parent before
    /// all of the directory's own children had been added, silently dropping
    /// their sizes from every ancestor.
    #[test]
    fn test_aggregation_with_child_before_parent_in_arena() {
        let mut tree = FileTree::with_capacity(8);
        let root = tree.add_root(CompactString::new("C:")); // arena idx 0

        // Allocate bottom-up so every child has a LOWER arena index than its
        // parent, then wire the hierarchy: root -> outer -> inner -> files.
        let deep_file = tree.add_node(FileNode::new_file(
            CompactString::new("deep.bin"),
            500,
            None,
        )); // idx 1
        let inner = tree.add_node(FileNode::new_dir(CompactString::new("inner"), None)); // idx 2
        let outer = tree.add_node(FileNode::new_dir(CompactString::new("outer"), None)); // idx 3
        let shallow_file = tree.add_node(FileNode::new_file(
            CompactString::new("shallow.txt"),
            250,
            None,
        )); // idx 4

        tree.add_child(root, outer);
        tree.add_child(outer, inner);
        tree.add_child(inner, deep_file);
        tree.add_child(outer, shallow_file);

        tree.aggregate_sizes();

        assert_eq!(tree.node(inner).size, 500);
        assert_eq!(tree.node(outer).size, 750);
        assert_eq!(tree.node(root).size, 750);
        assert_eq!(tree.node(inner).descendant_count, 1);
        assert_eq!(tree.node(outer).descendant_count, 2);
        assert_eq!(tree.node(root).descendant_count, 2);
        assert_eq!(tree.total_size, 750);
        assert_eq!(tree.file_count, 2);
    }

    /// Both aggregation strategies (reverse pass and post-order fallback)
    /// must produce identical results on the same logical tree.
    #[test]
    fn test_aggregation_order_independent() {
        // In-order arena: parent-first (uses the fast reverse pass).
        let mut in_order = FileTree::with_capacity(8);
        let root_a = in_order.add_root(CompactString::new("C:"));
        let dir_a = in_order.add_node(FileNode::new_dir(CompactString::new("d"), None));
        in_order.add_child(root_a, dir_a);
        let f1_a = in_order.add_node(FileNode::new_file(CompactString::new("a"), 100, None));
        in_order.add_child(dir_a, f1_a);
        let f2_a = in_order.add_node(FileNode::new_file(CompactString::new("b"), 200, None));
        in_order.add_child(root_a, f2_a);
        in_order.aggregate_sizes();

        // Out-of-order arena: same hierarchy, children allocated first
        // (forces the post-order fallback).
        let mut out_of_order = FileTree::with_capacity(8);
        let root_b = out_of_order.add_root(CompactString::new("C:"));
        let f1_b = out_of_order.add_node(FileNode::new_file(CompactString::new("a"), 100, None));
        let f2_b = out_of_order.add_node(FileNode::new_file(CompactString::new("b"), 200, None));
        let dir_b = out_of_order.add_node(FileNode::new_dir(CompactString::new("d"), None));
        out_of_order.add_child(root_b, dir_b);
        out_of_order.add_child(dir_b, f1_b);
        out_of_order.add_child(root_b, f2_b);
        out_of_order.aggregate_sizes();

        assert_eq!(in_order.total_size, out_of_order.total_size);
        assert_eq!(in_order.file_count, out_of_order.file_count);
        assert_eq!(in_order.node(root_a).size, out_of_order.node(root_b).size);
        assert_eq!(in_order.node(dir_a).size, out_of_order.node(dir_b).size);
        assert_eq!(
            in_order.node(root_a).descendant_count,
            out_of_order.node(root_b).descendant_count
        );
    }

    #[test]
    fn test_full_path() {
        let mut tree = FileTree::with_capacity(4);
        let root = tree.add_root(CompactString::new("C:"));
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("Users"), Some(root)));
        tree.add_child(root, dir);
        let file = tree.add_node(FileNode::new_file(
            CompactString::new("test.txt"),
            50,
            Some(dir),
        ));
        tree.add_child(dir, file);

        assert_eq!(tree.full_path(file), "C:\\Users\\test.txt");
    }

    #[test]
    fn test_children_sorted() {
        let mut tree = FileTree::with_capacity(5);
        let root = tree.add_root(CompactString::new("C:"));

        let small = tree.add_node(FileNode::new_file(
            CompactString::new("small.txt"),
            10,
            Some(root),
        ));
        tree.add_child(root, small);

        let big = tree.add_node(FileNode::new_file(
            CompactString::new("big.bin"),
            1000,
            Some(root),
        ));
        tree.add_child(root, big);

        let dir = tree.add_node(FileNode::new_dir(CompactString::new("folder"), Some(root)));
        tree.add_child(root, dir);

        let sorted = tree.children_sorted_by_size(root);
        // Directory first, then big file, then small file.
        assert_eq!(sorted[0], dir);
        assert_eq!(sorted[1], big);
        assert_eq!(sorted[2], small);
    }
}
