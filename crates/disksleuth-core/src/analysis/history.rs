/// Scan history snapshots and comparison.
///
/// A [`ScanSnapshot`] is a compact summary of a completed scan: overall
/// totals plus per-directory sizes down to a bounded depth. Keeping the
/// directory map small (instead of retaining whole multi-million-node
/// `FileTree`s) lets a frontend hold many snapshots in memory and answer
/// "where did my space go since the last scan?" by diffing two of them.
use crate::model::{FileTree, NodeIndex};
use std::collections::HashMap;
use std::time::Duration;

/// Directory depth captured in a snapshot (root children = depth 1).
pub const SNAPSHOT_DEPTH: u16 = 3;

/// Upper bound on directories recorded per snapshot.
///
/// Breadth-first collection means shallow (most interesting) directories are
/// always kept when the cap truncates a very wide tree.
pub const MAX_SNAPSHOT_DIRS: usize = 20_000;

/// Compact summary of one completed scan.
#[derive(Debug, Clone)]
pub struct ScanSnapshot {
    /// The path that was scanned (e.g. `C:\` or `D:\Projects`).
    pub root_path: String,
    /// Wall-clock time the scan finished.
    pub taken: chrono::DateTime<chrono::Local>,
    /// Total logical size across all roots.
    pub total_size: u64,
    /// Total number of files.
    pub file_count: u64,
    /// How long the scan took.
    pub duration: Duration,
    /// Aggregated size per directory, keyed by path relative to the scan
    /// root (e.g. `Users\Alice`). Bounded by depth and entry count.
    pub dir_sizes: Vec<(String, u64)>,
}

impl ScanSnapshot {
    /// Capture a snapshot of an aggregated tree.
    ///
    /// `root_path` should be the path handed to the scanner so snapshots of
    /// the same location can be matched up later regardless of how the root
    /// node is labelled.
    pub fn from_tree(tree: &FileTree, root_path: String, duration: Duration) -> Self {
        let mut dir_sizes = Vec::new();

        // Breadth-first over directories so the shallowest entries win when
        // the MAX_SNAPSHOT_DIRS cap kicks in.
        let mut queue: std::collections::VecDeque<(NodeIndex, u16, String)> =
            tree.roots.iter().map(|&r| (r, 0, String::new())).collect();

        while let Some((idx, depth, rel_path)) = queue.pop_front() {
            if dir_sizes.len() >= MAX_SNAPSHOT_DIRS {
                break;
            }
            let node = tree.node(idx);
            if depth > 0 {
                dir_sizes.push((rel_path.clone(), node.size));
            }
            if depth >= SNAPSHOT_DEPTH {
                continue;
            }
            let mut child = node.first_child;
            while let Some(c) = child {
                let child_node = tree.node(c);
                if child_node.is_dir && !child_node.is_deleted {
                    let child_rel = if rel_path.is_empty() {
                        child_node.name.to_string()
                    } else {
                        format!("{}\\{}", rel_path, child_node.name)
                    };
                    queue.push_back((c, depth + 1, child_rel));
                }
                child = child_node.next_sibling;
            }
        }

        Self {
            root_path,
            taken: chrono::Local::now(),
            total_size: tree.total_size,
            file_count: tree.file_count,
            duration,
            dir_sizes,
        }
    }
}

/// Size change of one directory between two snapshots.
#[derive(Debug, Clone)]
pub struct DirDelta {
    /// Path relative to the scan root.
    pub path: String,
    /// Size in the older snapshot (`None` = directory did not exist).
    pub old_size: Option<u64>,
    /// Size in the newer snapshot (`None` = directory was removed).
    pub new_size: Option<u64>,
}

impl DirDelta {
    /// Signed size change in bytes (new − old).
    pub fn delta(&self) -> i128 {
        self.new_size.unwrap_or(0) as i128 - self.old_size.unwrap_or(0) as i128
    }
}

/// Compare two snapshots directory-by-directory.
///
/// Returns only directories whose size changed (including added/removed
/// ones), sorted by absolute change descending. Callers should ensure both
/// snapshots cover the same `root_path` for the result to be meaningful.
pub fn compare_snapshots(old: &ScanSnapshot, new: &ScanSnapshot) -> Vec<DirDelta> {
    let old_map: HashMap<&str, u64> = old
        .dir_sizes
        .iter()
        .map(|(p, s)| (p.as_str(), *s))
        .collect();
    let new_map: HashMap<&str, u64> = new
        .dir_sizes
        .iter()
        .map(|(p, s)| (p.as_str(), *s))
        .collect();

    let mut deltas: Vec<DirDelta> = Vec::new();
    for (path, &new_size) in &new_map {
        let old_size = old_map.get(path).copied();
        if old_size != Some(new_size) {
            deltas.push(DirDelta {
                path: (*path).to_string(),
                old_size,
                new_size: Some(new_size),
            });
        }
    }
    for (path, &old_size) in &old_map {
        if !new_map.contains_key(path) {
            deltas.push(DirDelta {
                path: (*path).to_string(),
                old_size: Some(old_size),
                new_size: None,
            });
        }
    }

    deltas.sort_unstable_by_key(|d| std::cmp::Reverse(d.delta().unsigned_abs()));
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_node::FileNode;
    use compact_str::CompactString;

    /// root -> a(dir) -> deep(dir) -> file(100); root -> b(dir) -> file(50)
    fn sample_tree() -> FileTree {
        let mut tree = FileTree::with_capacity(8);
        let root = tree.add_root(CompactString::new("C:"));
        let a = tree.add_node(FileNode::new_dir(CompactString::new("a"), Some(root)));
        tree.add_child(root, a);
        let deep = tree.add_node(FileNode::new_dir(CompactString::new("deep"), Some(a)));
        tree.add_child(a, deep);
        let f1 = tree.add_node(FileNode::new_file(
            CompactString::new("f1.bin"),
            100,
            Some(deep),
        ));
        tree.add_child(deep, f1);
        let b = tree.add_node(FileNode::new_dir(CompactString::new("b"), Some(root)));
        tree.add_child(root, b);
        let f2 = tree.add_node(FileNode::new_file(
            CompactString::new("f2.bin"),
            50,
            Some(b),
        ));
        tree.add_child(b, f2);
        tree.aggregate_sizes();
        tree
    }

    /// Snapshot records totals plus relative directory paths with sizes.
    #[test]
    fn snapshot_captures_dirs_and_totals() {
        let tree = sample_tree();
        let snap = ScanSnapshot::from_tree(&tree, "C:\\".into(), Duration::from_secs(1));

        assert_eq!(snap.total_size, 150);
        assert_eq!(snap.file_count, 2);
        assert_eq!(snap.root_path, "C:\\");

        let map: HashMap<_, _> = snap.dir_sizes.iter().cloned().collect();
        assert_eq!(map.get("a").copied(), Some(100));
        assert_eq!(map.get("b").copied(), Some(50));
        assert_eq!(map.get("a\\deep").copied(), Some(100));
        assert!(!map.contains_key(""), "root itself is not an entry");
    }

    /// Comparison reports grown, shrunk, added, and removed directories,
    /// ordered by absolute delta.
    #[test]
    fn compare_reports_changes_sorted() {
        let tree = sample_tree();
        let old = ScanSnapshot::from_tree(&tree, "C:\\".into(), Duration::ZERO);

        let mut new = old.clone();
        new.dir_sizes = vec![
            ("a".into(), 500),        // grew by 400
            ("a\\deep".into(), 500),  // grew by 400
            ("fresh".into(), 10_000), // added
        ]; // "b" removed (-50)

        let deltas = compare_snapshots(&old, &new);
        assert_eq!(deltas.len(), 4);
        assert_eq!(deltas[0].path, "fresh");
        assert_eq!(deltas[0].delta(), 10_000);
        assert!(deltas
            .iter()
            .any(|d| d.path == "b" && d.new_size.is_none() && d.delta() == -50));
    }

    /// Identical snapshots produce no deltas.
    #[test]
    fn compare_identical_is_empty() {
        let tree = sample_tree();
        let snap = ScanSnapshot::from_tree(&tree, "C:\\".into(), Duration::ZERO);
        assert!(compare_snapshots(&snap, &snap).is_empty());
    }

    /// Directories below SNAPSHOT_DEPTH are not recorded.
    #[test]
    fn snapshot_respects_depth_cap() {
        let mut tree = FileTree::with_capacity(8);
        let root = tree.add_root(CompactString::new("C:"));
        let mut parent = root;
        for name in ["d1", "d2", "d3", "d4"] {
            let d = tree.add_node(FileNode::new_dir(CompactString::new(name), Some(parent)));
            tree.add_child(parent, d);
            parent = d;
        }
        tree.aggregate_sizes();

        let snap = ScanSnapshot::from_tree(&tree, "C:\\".into(), Duration::ZERO);
        let deepest = snap
            .dir_sizes
            .iter()
            .map(|(p, _)| p.matches('\\').count() + 1)
            .max()
            .unwrap();
        assert_eq!(deepest as u16, SNAPSHOT_DEPTH);
    }
}
