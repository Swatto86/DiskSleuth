/// Top-N largest files analysis.
///
/// Extracts the largest individual files from a scanned `FileTree`
/// for display in the analysis panel.
use crate::model::{FileTree, NodeIndex};

/// A single entry in the "largest files" results.
pub struct LargestFile {
    /// Index into the file tree arena.
    pub index: NodeIndex,
    /// Full path (reconstructed).
    pub path: String,
    /// File size in bytes.
    pub size: u64,
}

/// Get the top N largest files from the tree.
///
/// The tree pre-computes `largest_files` during aggregation, so this
/// function simply reads from that cache and enriches with full paths.
pub fn top_files(tree: &FileTree, n: usize) -> Vec<LargestFile> {
    tree.largest_files
        .iter()
        .take(n)
        .map(|&idx| LargestFile {
            index: idx,
            path: tree.full_path(idx),
            size: tree.node(idx).size,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_node::FileNode;
    use compact_str::CompactString;

    /// Build a small tree with three files of different sizes, aggregate, and
    /// confirm that `top_files` returns them in descending-size order.
    #[test]
    fn top_files_sorted_descending() {
        let mut tree = FileTree::with_capacity(5);
        let root = tree.add_root(CompactString::new("C:"));

        let small = tree.add_node(FileNode::new_file(
            CompactString::new("small.txt"),
            100,
            Some(root),
        ));
        tree.add_child(root, small);

        let big = tree.add_node(FileNode::new_file(
            CompactString::new("big.bin"),
            9_000,
            Some(root),
        ));
        tree.add_child(root, big);

        let medium = tree.add_node(FileNode::new_file(
            CompactString::new("medium.dat"),
            500,
            Some(root),
        ));
        tree.add_child(root, medium);

        tree.aggregate_sizes();

        let top = top_files(&tree, 10);
        assert_eq!(top.len(), 3, "should return all three files");
        assert_eq!(top[0].size, 9_000, "largest first");
        assert_eq!(top[1].size, 500);
        assert_eq!(top[2].size, 100, "smallest last");
    }

    /// `top_files` must respect the `n` limit.
    #[test]
    fn top_files_respects_n() {
        let mut tree = FileTree::with_capacity(5);
        let root = tree.add_root(CompactString::new("C:"));
        for i in 0..5u64 {
            let f = tree.add_node(FileNode::new_file(
                CompactString::new(format!("f{i}.bin")),
                i * 100 + 100,
                Some(root),
            ));
            tree.add_child(root, f);
        }
        tree.aggregate_sizes();

        let top = top_files(&tree, 2);
        assert_eq!(top.len(), 2);
        // The two largest files (400 and 500 bytes relative to base).
        assert!(top[0].size >= top[1].size, "must be descending");
    }

    /// `top_files` on an empty tree returns an empty vec — no panic.
    #[test]
    fn top_files_empty_tree() {
        let tree = FileTree::with_capacity(0);
        let top = top_files(&tree, 5);
        assert!(top.is_empty());
    }

    /// `n = 0` returns an empty vec without modifying the tree.
    #[test]
    fn top_files_n_zero() {
        let mut tree = FileTree::with_capacity(2);
        let root = tree.add_root(CompactString::new("C:"));
        let f = tree.add_node(FileNode::new_file(
            CompactString::new("x.bin"),
            1024,
            Some(root),
        ));
        tree.add_child(root, f);
        tree.aggregate_sizes();

        let top = top_files(&tree, 0);
        assert!(top.is_empty());
    }

    /// Full paths include the drive root and all intermediate directories.
    #[test]
    fn top_files_full_path() {
        let mut tree = FileTree::with_capacity(4);
        let root = tree.add_root(CompactString::new("C:"));
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("Users"), Some(root)));
        tree.add_child(root, dir);
        let f = tree.add_node(FileNode::new_file(
            CompactString::new("readme.txt"),
            200,
            Some(dir),
        ));
        tree.add_child(dir, f);
        tree.aggregate_sizes();

        let top = top_files(&tree, 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].path, "C:\\Users\\readme.txt");
    }
}
