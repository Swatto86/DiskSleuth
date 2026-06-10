/// Analysis modules — post-scan algorithms for insights.
pub mod age;
pub mod duplicates;
pub mod export;
pub mod file_types;
pub mod history;
pub mod top_files;

pub use age::{find_stale_files, StaleFile};
pub use duplicates::{
    collect_candidates, find_duplicates, find_duplicates_among, DuplicateCandidate, DuplicateFile,
    DuplicateGroup, DEFAULT_MIN_DUPLICATE_SIZE,
};
pub use export::{export_tree_csv, export_tree_json};
pub use file_types::{analyse_file_types, categorise_extension, CategoryStats, FileCategory};
pub use history::{compare_snapshots, DirDelta, ScanSnapshot};
pub use top_files::{top_files, LargestFile};
