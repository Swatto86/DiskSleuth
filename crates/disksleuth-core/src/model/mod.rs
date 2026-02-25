/// Data model for the DiskSleuth file tree.
///
/// Re-exports the arena-allocated tree structure and supporting types.
pub mod file_node;
pub mod file_tree;
pub mod size;

pub use file_node::{FileNode, NodeIndex};
pub use file_tree::FileTree;
