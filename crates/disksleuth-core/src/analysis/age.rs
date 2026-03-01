/// File age analysis — find old/stale files that haven't been modified recently.
use crate::model::{FileTree, NodeIndex};
use std::time::{Duration, SystemTime};

/// A file identified as old/stale.
pub struct StaleFile {
    pub index: NodeIndex,
    pub path: String,
    pub size: u64,
    pub last_modified: SystemTime,
    pub age_days: u64,
}

/// Find files not modified in the last `min_age_days` days,
/// sorted by size descending.
///
/// Returns an empty vec immediately when `max_results == 0`, which also
/// avoids an integer underflow (`max_results - 1` wrapping to `usize::MAX`)
/// that would panic inside `select_nth_unstable_by`.
pub fn find_stale_files(tree: &FileTree, min_age_days: u64, max_results: usize) -> Vec<StaleFile> {
    // Guard: requesting zero results is always satisfiable trivially, and
    // prevents the `max_results - 1` subtraction below from underflowing.
    if max_results == 0 {
        return Vec::new();
    }

    let now = SystemTime::now();
    let threshold = Duration::from_secs(min_age_days * 24 * 3600);

    let mut stale: Vec<StaleFile> = tree
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, node)| {
            if node.is_dir {
                return None;
            }
            let modified = node.modified?;
            let age = now.duration_since(modified).ok()?;
            if age >= threshold {
                Some(StaleFile {
                    index: NodeIndex::new(i),
                    path: tree.full_path(NodeIndex::new(i)),
                    size: node.size,
                    last_modified: modified,
                    age_days: age.as_secs() / 86400,
                })
            } else {
                None
            }
        })
        .collect();

    // Partial sort: O(n) select + O(k log k) sort of top-k, vs O(n log n) full sort.
    if stale.len() > max_results {
        stale.select_nth_unstable_by(max_results - 1, |a, b| b.size.cmp(&a.size));
        stale.truncate(max_results);
    }
    stale.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_node::FileNode;
    use compact_str::CompactString;

    /// Sets `modified` to a timestamp `days` days in the past.
    fn file_with_age(
        name: &str,
        size: u64,
        parent: crate::model::NodeIndex,
        days_ago: u64,
    ) -> FileNode {
        let mut node = FileNode::new_file(CompactString::new(name), size, Some(parent));
        let age = Duration::from_secs(days_ago * 24 * 3600 + 1);
        node.modified = Some(SystemTime::now() - age);
        node
    }

    /// A file whose `modified` timestamp is before the threshold must be returned.
    #[test]
    fn stale_file_older_than_threshold_is_returned() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));

        let old = file_with_age("old.log", 500, root, 400); // 400 days old
        let old_idx = tree.add_node(old);
        tree.add_child(root, old_idx);
        tree.aggregate_sizes();

        let result = find_stale_files(&tree, 365, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].size, 500);
        assert!(result[0].age_days >= 400);
    }

    /// A file newer than the threshold must NOT appear in results.
    #[test]
    fn recent_file_is_excluded() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));

        let new_node = file_with_age("new.txt", 1024, root, 10); // 10 days old
        let new_idx = tree.add_node(new_node);
        tree.add_child(root, new_idx);
        tree.aggregate_sizes();

        let result = find_stale_files(&tree, 365, 10);
        assert!(
            result.is_empty(),
            "10-day-old file should not be stale at 365-day threshold"
        );
    }

    /// Results must be sorted by size descending so the largest stale file
    /// appears first.
    #[test]
    fn stale_files_sorted_by_size_descending() {
        let mut tree = FileTree::with_capacity(5);
        let root = tree.add_root(CompactString::new("C:"));

        let small = file_with_age("small.bak", 100, root, 400);
        let si = tree.add_node(small);
        tree.add_child(root, si);

        let big = file_with_age("big.bak", 5_000, root, 400);
        let bi = tree.add_node(big);
        tree.add_child(root, bi);

        tree.aggregate_sizes();

        let result = find_stale_files(&tree, 365, 10);
        assert_eq!(result.len(), 2);
        assert!(result[0].size >= result[1].size, "must be descending");
        assert_eq!(result[0].size, 5_000);
    }

    /// `max_results` truncates the output list.
    #[test]
    fn max_results_is_respected() {
        let mut tree = FileTree::with_capacity(6);
        let root = tree.add_root(CompactString::new("C:"));

        for i in 0..5u64 {
            let n = file_with_age(&format!("f{i}.bak"), (i + 1) * 100, root, 400);
            let idx = tree.add_node(n);
            tree.add_child(root, idx);
        }
        tree.aggregate_sizes();

        let result = find_stale_files(&tree, 365, 2);
        assert_eq!(result.len(), 2, "must be capped at max_results");
    }

    /// Directories must not appear in stale file results.
    #[test]
    fn directories_are_excluded() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));

        // A directory node with no modified time — should never appear.
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("OldDir"), Some(root)));
        tree.add_child(root, dir);
        tree.aggregate_sizes();

        let result = find_stale_files(&tree, 0, 100);
        assert!(result.is_empty(), "directories must be excluded");
    }

    /// Regression test: `max_results == 0` must return an empty vec without
    /// panicking.  Previously, `max_results - 1` would underflow (usize wraps
    /// to `usize::MAX`) and `select_nth_unstable_by(usize::MAX, …)` would
    /// panic with an out-of-bounds index.
    #[test]
    fn zero_max_results_does_not_panic() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));

        let old = file_with_age("old.log", 1024, root, 400);
        let idx = tree.add_node(old);
        tree.add_child(root, idx);
        tree.aggregate_sizes();

        // Must NOT panic and must return an empty vec.
        let result = find_stale_files(&tree, 0, 0);
        assert!(result.is_empty(), "max_results=0 must always return empty");
    }

    /// A file without a `modified` timestamp (e.g. an error placeholder)
    /// must be silently ignored, not panic.
    #[test]
    fn file_without_modified_is_ignored() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));

        // new_file() sets modified = None by default.
        let f = tree.add_node(FileNode::new_file(
            CompactString::new("no_time.bin"),
            999,
            Some(root),
        ));
        tree.add_child(root, f);
        tree.aggregate_sizes();

        // Should not panic; file has no timestamp so it is skipped.
        let result = find_stale_files(&tree, 0, 100);
        assert!(result.is_empty());
    }
}
