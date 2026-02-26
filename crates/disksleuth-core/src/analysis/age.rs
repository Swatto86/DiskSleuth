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
pub fn find_stale_files(tree: &FileTree, min_age_days: u64, max_results: usize) -> Vec<StaleFile> {
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

    stale.sort_by(|a, b| b.size.cmp(&a.size));
    stale.truncate(max_results);
    stale
}
