/// Duplicate file detection — size grouping, then 4 KB prefix hash, then
/// full-content hash.
///
/// Strategy:
/// 1. Group candidate files by size — files with unique sizes cannot be
///    duplicates, which eliminates the vast majority without any I/O.
/// 2. Hash the first 4 KB of each remaining file to split size groups.
/// 3. Hash the full content of files whose prefix still matches.
///
/// Hashing uses 128-bit FNV-1a. Combined with the equal-size requirement, an
/// accidental collision is astronomically unlikely, and the dependency-free
/// implementation keeps the binary small. Hard-linked files appear as
/// duplicates (they have identical content by definition).
///
/// Size groups are hashed in parallel via rayon. The work is cancellable and
/// reports progress through a callback, so a frontend can run it on a
/// background thread without blocking.
use crate::model::{FileTree, NodeIndex};
use rayon::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Default minimum file size considered for duplicate detection (1 MB).
///
/// Hashing millions of tiny files costs far more time than the space they
/// could ever reclaim; callers may lower this for thorough scans.
pub const DEFAULT_MIN_DUPLICATE_SIZE: u64 = 1024 * 1024;

/// Bytes hashed in the cheap prefix pass.
const PREFIX_LEN: usize = 4096;

/// Buffer size for streaming full-content hashing.
const READ_BUF_LEN: usize = 64 * 1024;

/// A file eligible for duplicate detection, with its path pre-resolved so
/// hashing needs no access to the originating [`FileTree`].
#[derive(Debug, Clone)]
pub struct DuplicateCandidate {
    pub index: NodeIndex,
    pub size: u64,
    pub path: PathBuf,
}

/// One file inside a [`DuplicateGroup`].
#[derive(Debug, Clone)]
pub struct DuplicateFile {
    pub index: NodeIndex,
    pub path: String,
}

/// A group of files with identical size and content.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    /// Size of each file in the group.
    pub size: u64,
    /// All files in this duplicate group (always ≥ 2 entries).
    pub files: Vec<DuplicateFile>,
}

impl DuplicateGroup {
    /// Bytes that deleting all but one copy would reclaim.
    pub fn wasted_bytes(&self) -> u64 {
        self.size * (self.files.len() as u64 - 1)
    }
}

/// Collect files worth hashing: non-deleted, non-error regular files of at
/// least `min_size` bytes whose size occurs more than once in the tree.
///
/// Pure tree walk — no file I/O. The returned candidates own their full
/// paths, so [`find_duplicates_among`] can run on another thread without
/// borrowing the tree.
pub fn collect_candidates(tree: &FileTree, min_size: u64) -> Vec<DuplicateCandidate> {
    let min_size = min_size.max(1); // zero-byte files are never useful matches
    let mut size_counts: HashMap<u64, u32> = HashMap::new();
    for node in &tree.nodes {
        if !node.is_dir && !node.is_error && !node.is_deleted && node.size >= min_size {
            *size_counts.entry(node.size).or_insert(0) += 1;
        }
    }

    tree.nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            !node.is_dir
                && !node.is_error
                && !node.is_deleted
                && node.size >= min_size
                && size_counts.get(&node.size).copied().unwrap_or(0) > 1
        })
        .map(|(i, node)| DuplicateCandidate {
            index: NodeIndex::new(i),
            size: node.size,
            path: PathBuf::from(tree.full_path(NodeIndex::new(i))),
        })
        .collect()
}

/// Hash the candidates and group files with identical content.
///
/// `progress(files_done, files_total)` is invoked as candidates finish
/// hashing (from rayon worker threads, so it must be `Sync`). Setting
/// `cancel` stops the remaining work; groups completed so far are returned.
/// Unreadable files are silently skipped. Groups are sorted by reclaimable
/// (wasted) bytes, largest first.
pub fn find_duplicates_among(
    candidates: Vec<DuplicateCandidate>,
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize) + Sync,
) -> Vec<DuplicateGroup> {
    let total = candidates.len();
    let done = AtomicUsize::new(0);

    // Group by size.
    let mut by_size: HashMap<u64, Vec<DuplicateCandidate>> = HashMap::new();
    for c in candidates {
        by_size.entry(c.size).or_default().push(c);
    }

    let size_groups: Vec<Vec<DuplicateCandidate>> = by_size.into_values().collect();

    let mut groups: Vec<DuplicateGroup> = size_groups
        .into_par_iter()
        .flat_map_iter(|group| {
            if cancel.load(Ordering::Relaxed) {
                // Report skipped files so the progress bar still completes.
                done.fetch_add(group.len(), Ordering::Relaxed);
                return Vec::new().into_iter();
            }

            // Pass 2: split the size group by prefix hash.
            let mut by_prefix: HashMap<u128, Vec<DuplicateCandidate>> = HashMap::new();
            let mut unreadable = 0usize;
            for c in group {
                match hash_prefix(&c.path) {
                    Some(h) => by_prefix.entry(h).or_default().push(c),
                    None => unreadable += 1,
                }
            }
            done.fetch_add(unreadable, Ordering::Relaxed);

            // Pass 3: full-content hash for prefix-matching files.
            let mut result: Vec<DuplicateGroup> = Vec::new();
            for (_, subgroup) in by_prefix {
                if subgroup.len() < 2 {
                    let n = done.fetch_add(subgroup.len(), Ordering::Relaxed) + subgroup.len();
                    progress(n, total);
                    continue;
                }
                let mut by_content: HashMap<(u128, u64), Vec<DuplicateFile>> = HashMap::new();
                for c in subgroup {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(content_key) = hash_full(&c.path) {
                        by_content
                            .entry(content_key)
                            .or_default()
                            .push(DuplicateFile {
                                index: c.index,
                                path: c.path.to_string_lossy().into_owned(),
                            });
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(n, total);
                }
                result.extend(
                    by_content
                        .into_iter()
                        .filter(|(_, files)| files.len() >= 2)
                        .map(|((_, size), files)| DuplicateGroup { size, files }),
                );
            }
            result.into_iter()
        })
        .collect();

    groups.sort_unstable_by_key(|g| std::cmp::Reverse(g.wasted_bytes()));
    groups
}

/// Convenience wrapper: collect candidates from `tree` and hash them on the
/// calling thread. Frontends with a UI should prefer running
/// [`find_duplicates_among`] on a background thread instead.
pub fn find_duplicates(tree: &FileTree, min_size: u64, cancel: &AtomicBool) -> Vec<DuplicateGroup> {
    find_duplicates_among(collect_candidates(tree, min_size), cancel, |_, _| {})
}

// ── FNV-1a 128-bit ──────────────────────────────────────────────────────────

const FNV_OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
const FNV_PRIME: u128 = 0x0000000001000000000000000000013B;

#[inline]
fn fnv1a_update(mut hash: u128, data: &[u8]) -> u128 {
    for &byte in data {
        hash ^= byte as u128;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Hash the first [`PREFIX_LEN`] bytes of a file. `None` if unreadable.
fn hash_prefix(path: &std::path::Path) -> Option<u128> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; PREFIX_LEN];
    let mut filled = 0usize;
    while filled < PREFIX_LEN {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    Some(fnv1a_update(FNV_OFFSET, &buf[..filled]))
}

/// Hash the entire file content. Returns `(hash, bytes_read)` so files that
/// changed size since the scan can never collide with each other. `None` if
/// unreadable.
fn hash_full(path: &std::path::Path) -> Option<(u128, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; READ_BUF_LEN];
    let mut hash = FNV_OFFSET;
    let mut total = 0u64;
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hash = fnv1a_update(hash, &buf[..n]);
                total += n as u64;
            }
            Err(_) => return None,
        }
    }
    Some((hash, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_node::FileNode;
    use compact_str::CompactString;
    use std::sync::atomic::AtomicBool;

    /// Write `content` to `dir/name` and add a matching node to the tree.
    fn add_disk_file(
        tree: &mut FileTree,
        root: NodeIndex,
        dir: &std::path::Path,
        name: &str,
        content: &[u8],
    ) -> NodeIndex {
        std::fs::write(dir.join(name), content).unwrap();
        let idx = tree.add_node(FileNode::new_file(
            CompactString::new(name),
            content.len() as u64,
            Some(root),
        ));
        tree.add_child(root, idx);
        idx
    }

    /// Build a tree whose root path is the real temp directory so
    /// `full_path` resolves to genuine on-disk files.
    fn tree_for(dir: &std::path::Path) -> (FileTree, NodeIndex) {
        let mut tree = FileTree::with_capacity(8);
        let root = tree.add_root(CompactString::new(dir.to_string_lossy().as_ref()));
        (tree, root)
    }

    /// Two identical files are grouped; a same-size-different-content file
    /// and a unique-size file are not.
    #[test]
    fn detects_identical_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());

        let payload = vec![0xABu8; 8192];
        let a = add_disk_file(&mut tree, root, tmp.path(), "a.bin", &payload);
        let b = add_disk_file(&mut tree, root, tmp.path(), "b.bin", &payload);
        let mut different = payload.clone();
        *different.last_mut().unwrap() = 0xCD; // same size, different tail
        add_disk_file(&mut tree, root, tmp.path(), "c.bin", &different);
        add_disk_file(&mut tree, root, tmp.path(), "d.bin", &[1, 2, 3]);
        tree.aggregate_sizes();

        let groups = find_duplicates(&tree, 1, &AtomicBool::new(false));
        assert_eq!(groups.len(), 1, "exactly one duplicate group expected");
        assert_eq!(groups[0].size, 8192);
        assert_eq!(groups[0].wasted_bytes(), 8192);
        let mut indices: Vec<NodeIndex> = groups[0].files.iter().map(|f| f.index).collect();
        indices.sort();
        assert_eq!(indices, vec![a, b]);
    }

    /// Files with an identical 4 KB prefix but different content beyond it
    /// must NOT be reported as duplicates (full-content pass catches them).
    #[test]
    fn equal_prefix_different_tail_not_grouped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());

        let mut x = vec![0u8; 16384];
        let mut y = vec![0u8; 16384];
        x[10_000] = 1;
        y[10_000] = 2;
        add_disk_file(&mut tree, root, tmp.path(), "x.bin", &x);
        add_disk_file(&mut tree, root, tmp.path(), "y.bin", &y);
        tree.aggregate_sizes();

        let groups = find_duplicates(&tree, 1, &AtomicBool::new(false));
        assert!(groups.is_empty(), "prefix-only matches must be rejected");
    }

    /// `min_size` excludes small files from consideration entirely.
    #[test]
    fn min_size_filters_candidates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());
        add_disk_file(&mut tree, root, tmp.path(), "s1.txt", b"same");
        add_disk_file(&mut tree, root, tmp.path(), "s2.txt", b"same");
        tree.aggregate_sizes();

        assert!(collect_candidates(&tree, 1024).is_empty());
        assert_eq!(collect_candidates(&tree, 1).len(), 2);
    }

    /// Files whose size is unique are never candidates (no I/O needed).
    #[test]
    fn unique_sizes_are_not_candidates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());
        add_disk_file(&mut tree, root, tmp.path(), "one.bin", &[0; 100]);
        add_disk_file(&mut tree, root, tmp.path(), "two.bin", &[0; 200]);
        tree.aggregate_sizes();

        assert!(collect_candidates(&tree, 1).is_empty());
    }

    /// A pre-set cancel flag yields no groups (and no panic).
    #[test]
    fn cancelled_search_returns_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());
        let payload = vec![7u8; 4096];
        add_disk_file(&mut tree, root, tmp.path(), "p.bin", &payload);
        add_disk_file(&mut tree, root, tmp.path(), "q.bin", &payload);
        tree.aggregate_sizes();

        let groups = find_duplicates(&tree, 1, &AtomicBool::new(true));
        assert!(groups.is_empty());
    }

    /// Progress reaches `(total, total)` on an uncancelled run.
    #[test]
    fn progress_reports_completion() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mut tree, root) = tree_for(tmp.path());
        let payload = vec![9u8; 2048];
        add_disk_file(&mut tree, root, tmp.path(), "m.bin", &payload);
        add_disk_file(&mut tree, root, tmp.path(), "n.bin", &payload);
        tree.aggregate_sizes();

        let max_seen = AtomicUsize::new(0);
        let candidates = collect_candidates(&tree, 1);
        let total = candidates.len();
        find_duplicates_among(candidates, &AtomicBool::new(false), |done, t| {
            assert_eq!(t, total);
            max_seen.fetch_max(done, Ordering::Relaxed);
        });
        assert_eq!(max_seen.load(Ordering::Relaxed), total);
    }
}
