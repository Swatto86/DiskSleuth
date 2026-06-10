/// CSV and JSON export of a scanned [`FileTree`].
///
/// Both formats write one row/object per node (files and directories) so the
/// output can be filtered/pivoted in a spreadsheet or consumed by scripts.
/// Directory entries carry the aggregated size and descendant file count
/// computed by `aggregate_sizes`. Tombstoned (deleted) nodes are skipped.
use crate::model::size::format_size;
use crate::model::{FileNode, FileTree, NodeIndex};
use std::io::Write;
use std::path::Path;

fn node_kind(node: &FileNode) -> &'static str {
    if node.is_error {
        "error"
    } else if node.is_dir {
        "dir"
    } else {
        "file"
    }
}

fn node_modified_rfc3339(node: &FileNode) -> Option<String> {
    node.modified
        .map(|m| chrono::DateTime::<chrono::Local>::from(m).to_rfc3339())
}

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
        if node.is_deleted {
            continue;
        }
        let files = if node.is_dir {
            node.descendant_count.to_string()
        } else {
            String::new()
        };

        writer.write_record([
            tree.full_path(NodeIndex::new(i)).as_str(),
            node_kind(node),
            &node.size.to_string(),
            &node.allocated_size.to_string(),
            &format_size(node.size),
            &files,
            &node_modified_rfc3339(node).unwrap_or_default(),
        ])?;
        rows += 1;
    }

    writer.flush()?;
    Ok(rows)
}

/// One node in the JSON export.
#[derive(serde::Serialize)]
struct JsonNode<'a> {
    path: String,
    kind: &'a str,
    size_bytes: u64,
    allocated_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<String>,
}

/// Write `tree` to `path` as a single JSON document.
///
/// Layout: `{"generated", "total_size_bytes", "file_count", "nodes": [...]}`
/// where `nodes` is a flat array (one object per node, full path included).
/// A flat array streams to disk without recursion, so arbitrarily deep trees
/// can never overflow the stack.
///
/// Returns the number of node objects written.
pub fn export_tree_json(tree: &FileTree, path: &Path) -> anyhow::Result<u64> {
    let file = std::fs::File::create(path)?;
    let mut out = std::io::BufWriter::new(file);

    write!(
        out,
        "{{\"generated\":{},\"total_size_bytes\":{},\"file_count\":{},\"nodes\":[",
        serde_json::to_string(&chrono::Local::now().to_rfc3339())?,
        tree.total_size,
        tree.file_count,
    )?;

    let mut rows = 0u64;
    for i in 0..tree.len() {
        let node = &tree.nodes[i];
        if node.is_deleted {
            continue;
        }
        if rows > 0 {
            out.write_all(b",")?;
        }
        serde_json::to_writer(
            &mut out,
            &JsonNode {
                path: tree.full_path(NodeIndex::new(i)),
                kind: node_kind(node),
                size_bytes: node.size,
                allocated_bytes: node.allocated_size,
                files: node.is_dir.then_some(node.descendant_count),
                modified: node_modified_rfc3339(node),
            },
        )?;
        rows += 1;
    }

    out.write_all(b"]}")?;
    out.flush()?;
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
        assert!(export_tree_csv(&tree, Path::new("/nonexistent_dir_xyz/out.csv")).is_err());
        assert!(export_tree_json(&tree, Path::new("/nonexistent_dir_xyz/out.json")).is_err());
    }

    /// JSON export produces a parseable document with all nodes and header
    /// metadata.
    #[test]
    fn export_json_is_valid_and_complete() {
        let tree = sample_tree();
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("export.json");

        let rows = export_tree_json(&tree, &out).expect("export must succeed");
        assert_eq!(rows, tree.len() as u64);

        let content = std::fs::read_to_string(&out).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&content).expect("must be valid JSON");
        assert_eq!(doc["total_size_bytes"], 1024);
        assert_eq!(doc["file_count"], 1);
        let nodes = doc["nodes"].as_array().expect("nodes array");
        assert_eq!(nodes.len(), tree.len());
        assert!(nodes
            .iter()
            .any(|n| n["path"] == "C:\\Users\\a.txt" && n["size_bytes"] == 1024));
        // Directory rows carry a file count; file rows omit it.
        let dir = nodes.iter().find(|n| n["kind"] == "dir").unwrap();
        assert_eq!(dir["files"], 1);
        let file = nodes.iter().find(|n| n["kind"] == "file").unwrap();
        assert!(file.get("files").is_none());
    }

    /// Tombstoned (deleted) nodes are excluded from both export formats.
    #[test]
    fn export_skips_deleted_nodes() {
        let mut tree = sample_tree();
        let users_dir = tree.children(tree.roots[0])[0];
        tree.remove_subtree(users_dir);
        tree.aggregate_sizes();

        let tmp = tempfile::TempDir::new().unwrap();
        let csv_rows = export_tree_csv(&tree, &tmp.path().join("t.csv")).unwrap();
        let json_rows = export_tree_json(&tree, &tmp.path().join("t.json")).unwrap();
        assert_eq!(csv_rows, 1, "only the root survives");
        assert_eq!(json_rows, 1);
    }

    /// An empty tree yields an empty (but valid) JSON nodes array.
    #[test]
    fn export_json_empty_tree() {
        let tree = FileTree::with_capacity(0);
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("empty.json");
        assert_eq!(export_tree_json(&tree, &out).unwrap(), 0);
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 0);
    }
}
