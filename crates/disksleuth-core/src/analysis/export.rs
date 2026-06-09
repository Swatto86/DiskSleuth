/// CSV export of a scanned [`FileTree`].
///
/// Writes one row per node (files and directories) so the output can be
/// filtered/pivoted in a spreadsheet. Directory rows carry the aggregated
/// size and descendant file count computed by `aggregate_sizes`.
use crate::model::size::format_size;
use crate::model::{FileTree, NodeIndex};
use std::path::Path;

/// Write every node of `tree` to `path` as CSV.
///
/// Columns: full path, type (`file` / `dir` / `error`), logical size in
/// bytes, allocated size in bytes, human-readable size, descendant file
/// count (directories only), and last-modified time (RFC 3339, when known).
///
/// Returns the number of data rows written (excluding the header).
pub fn export_tree_csv(tree: &FileTree, path: &Path) -> anyhow::Result<u64> {
    let file = std::fs::File::create(path)?;
    let mut writer = csv::Writer::from_writer(std::io::BufWriter::new(file));

    writer.write_record([
        "path",
        "type",
        "size_bytes",
        "allocated_bytes",
        "size_display",
        "files",
        "modified",
    ])?;

    let mut rows = 0u64;
    for i in 0..tree.len() {
        let node = &tree.nodes[i];
        let kind = if node.is_error {
            "error"
        } else if node.is_dir {
            "dir"
        } else {
            "file"
        };
        let files = if node.is_dir {
            node.descendant_count.to_string()
        } else {
            String::new()
        };
        let modified = node
            .modified
            .map(|m| chrono::DateTime::<chrono::Local>::from(m).to_rfc3339())
            .unwrap_or_default();

        writer.write_record([
            tree.full_path(NodeIndex::new(i)).as_str(),
            kind,
            &node.size.to_string(),
            &node.allocated_size.to_string(),
            &format_size(node.size),
            &files,
            &modified,
        ])?;
        rows += 1;
    }

    writer.flush()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_node::FileNode;
    use compact_str::CompactString;

    fn sample_tree() -> FileTree {
        let mut tree = FileTree::with_capacity(4);
        let root = tree.add_root(CompactString::new("C:"));
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("Users"), Some(root)));
        tree.add_child(root, dir);
        let mut file = FileNode::new_file(CompactString::new("a.txt"), 1024, Some(dir));
        file.modified = Some(std::time::SystemTime::now());
        let file = tree.add_node(file);
        tree.add_child(dir, file);
        tree.aggregate_sizes();
        tree
    }

    /// Every node becomes a data row and the header carries all columns.
    #[test]
    fn export_writes_header_and_all_rows() {
        let tree = sample_tree();
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("export.csv");

        let rows = export_tree_csv(&tree, &out).expect("export must succeed");
        assert_eq!(rows, tree.len() as u64);

        let content = std::fs::read_to_string(&out).unwrap();
        let mut lines = content.lines();
        assert_eq!(
            lines.next().unwrap(),
            "path,type,size_bytes,allocated_bytes,size_display,files,modified"
        );
        assert_eq!(content.lines().count(), tree.len() + 1);
        assert!(content.contains("C:\\Users\\a.txt"));
        assert!(content.contains("1024"));
    }

    /// An empty tree exports just the header without erroring.
    #[test]
    fn export_empty_tree() {
        let tree = FileTree::with_capacity(0);
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("empty.csv");

        let rows = export_tree_csv(&tree, &out).expect("export must succeed");
        assert_eq!(rows, 0);
        assert_eq!(std::fs::read_to_string(&out).unwrap().lines().count(), 1);
    }

    /// Exporting to an unwritable location returns an error, not a panic.
    #[test]
    fn export_to_invalid_path_errors() {
        let tree = sample_tree();
        let result = export_tree_csv(&tree, Path::new("/nonexistent_dir_xyz/out.csv"));
        assert!(result.is_err());
    }
}
