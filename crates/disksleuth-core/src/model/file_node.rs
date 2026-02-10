/// A single node in the arena-allocated file tree.
///
/// Nodes are stored in a flat `Vec<FileNode>` for cache-friendly traversal.
/// Parent-child relationships use indices rather than pointers, which also
/// makes serialisation trivial and avoids reference-counting overhead.

use compact_str::CompactString;
use std::time::SystemTime;

/// Lightweight index into the arena `Vec<FileNode>`.
///
/// Uses `u32` to keep nodes small — supports up to ~4 billion nodes,
/// which is more than enough for any real filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeIndex(pub u32);

impl NodeIndex {
    /// Create a new `NodeIndex` from a `usize`, panicking if it exceeds `u32::MAX`.
    #[inline]
    pub fn new(index: usize) -> Self {
        debug_assert!(index <= u32::MAX as usize, "NodeIndex overflow");
        Self(index as u32)
    }

    /// Return the index as a `usize` for Vec indexing.
    #[inline]
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

/// A single file or directory in the tree.
///
/// Stored in a flat arena (`Vec<FileNode>`) for cache efficiency.
/// Children are linked via a `first_child` / `next_sibling` linked list
/// so that no separate `Vec<NodeIndex>` allocation is needed per node.
#[derive(Debug, Clone)]
pub struct FileNode {
    /// File or directory name only (NOT the full path).
    /// Full paths are reconstructed on-demand by walking up via `parent`.
    pub name: CompactString,

    /// Logical file size in bytes.
    /// For directories this is the sum of all descendant file sizes,
    /// computed in a single bottom-up pass after scanning.
    pub size: u64,

    /// Space allocated on disk (cluster-rounded). May differ from `size`
    /// due to NTFS compression, sparse files, or cluster rounding.
    pub allocated_size: u64,

    /// `true` if this node represents a directory.
    pub is_dir: bool,

    /// Index of the parent node. `None` for root drives / scan roots.
    pub parent: Option<NodeIndex>,

    /// First child (directories only). Children form a singly-linked list
    /// via [`next_sibling`].
    pub first_child: Option<NodeIndex>,

    /// Next sibling under the same parent.
    pub next_sibling: Option<NodeIndex>,

    /// Total number of descendant *files* (not directories).
    /// Used for the "X files in folder" display.
    pub descendant_count: u64,

    /// Last-modified timestamp, used for age analysis.
    pub modified: Option<SystemTime>,

    /// Pre-computed percentage of the parent's size (0.0–100.0).
    /// Calculated in the aggregation pass so rendering doesn't repeat the division.
    pub percent_of_parent: f32,

    /// `true` if this node could not be read (e.g. access denied).
    /// The node stays in the tree so users can see where errors occurred.
    pub is_error: bool,
}

impl FileNode {
    /// Create a new file node with the given name and size.
    pub fn new_file(name: CompactString, size: u64, parent: Option<NodeIndex>) -> Self {
        Self {
            name,
            size,
            allocated_size: size,
            is_dir: false,
            parent,
            first_child: None,
            next_sibling: None,
            descendant_count: 0,
            modified: None,
            percent_of_parent: 0.0,
            is_error: false,
        }
    }

    /// Create a new directory node.
    pub fn new_dir(name: CompactString, parent: Option<NodeIndex>) -> Self {
        Self {
            name,
            size: 0,
            allocated_size: 0,
            is_dir: true,
            parent,
            first_child: None,
            next_sibling: None,
            descendant_count: 0,
            modified: None,
            percent_of_parent: 0.0,
            is_error: false,
        }
    }

    /// Create an error placeholder node (e.g. access denied directory).
    pub fn new_error(name: CompactString, is_dir: bool, parent: Option<NodeIndex>) -> Self {
        Self {
            name,
            size: 0,
            allocated_size: 0,
            is_dir,
            parent,
            first_child: None,
            next_sibling: None,
            descendant_count: 0,
            modified: None,
            percent_of_parent: 0.0,
            is_error: true,
        }
    }
}
