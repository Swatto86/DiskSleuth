/// Duplicate file detection (size-first, then partial hash, then full hash).
///
/// Phase 2 feature — stub for now with the public API defined.

use crate::model::{FileTree, NodeIndex};

/// A group of files that are duplicates of each other.
#[derive(Debug)]
pub struct DuplicateGroup {
    /// Size of each file in the group.
    pub size: u64,
    /// Indices of all files in this duplicate group.
    pub files: Vec<NodeIndex>,
}

/// Find duplicate files in the tree.
///
/// Strategy (Phase 2 implementation):
/// 1. Group files by size — files with unique sizes cannot be duplicates.
/// 2. For size-matching groups, read first 4KB and hash to eliminate false positives.
/// 3. For remaining candidates, hash the full file content.
///
/// Currently returns an empty vec (Phase 2 stub).
pub fn find_duplicates(_tree: &FileTree) -> Vec<DuplicateGroup> {
    // Phase 2: implement hash-based duplicate detection.
    Vec::new()
}
