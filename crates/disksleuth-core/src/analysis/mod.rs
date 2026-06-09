/// Analysis modules — post-scan algorithms for insights.
pub mod age;
pub mod duplicates;
pub mod export;
pub mod file_types;
pub mod top_files;

pub use export::export_tree_csv;
pub use file_types::{analyse_file_types, categorise_extension, CategoryStats, FileCategory};
pub use top_files::{top_files, LargestFile};
