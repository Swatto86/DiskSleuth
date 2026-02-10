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
