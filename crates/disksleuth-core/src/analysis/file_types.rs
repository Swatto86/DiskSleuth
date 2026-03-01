/// File type categorisation based on file extensions.
///
/// Groups files into broad categories (Documents, Media, Code, Archives,
/// System, Other) and computes size/count totals per category.
use crate::model::FileTree;
use std::collections::HashMap;

/// Broad file type categories for visual grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Documents,
    Images,
    Video,
    Audio,
    Archives,
    Code,
    Executables,
    System,
    Other,
}

impl FileCategory {
    /// Human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::Images => "Images",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Archives => "Archives",
            Self::Code => "Code",
            Self::Executables => "Executables",
            Self::System => "System",
            Self::Other => "Other",
        }
    }
}

/// Size and count totals for a single file category.
#[derive(Debug, Default, Clone)]
pub struct CategoryStats {
    pub category: Option<FileCategory>,
    pub total_size: u64,
    pub file_count: u64,
}

/// Categorise a file extension into a broad category.
///
/// Zero-heap-allocation hot path: extensions are lowercased into a fixed-size
/// stack buffer (`[u8; 16]`) rather than allocating a `String`.  File
/// extensions longer than 16 bytes are treated as `Other`.
pub fn categorise_extension(ext: &str) -> FileCategory {
    // Fast rejection: any extension longer than 16 bytes is definitely `Other`.
    let bytes = ext.as_bytes();
    if bytes.len() > 16 {
        return FileCategory::Other;
    }

    // Lowercase into a stack buffer — zero heap allocation.
    let mut lower = [0u8; 16];
    for (dest, &src) in lower.iter_mut().zip(bytes.iter()) {
        *dest = src.to_ascii_lowercase();
    }
    let lower_str = match std::str::from_utf8(&lower[..bytes.len()]) {
        Ok(s) => s,
        Err(_) => return FileCategory::Other,
    };

    match lower_str {
        // Documents
        "doc" | "docx" | "pdf" | "txt" | "rtf" | "odt" | "xls" | "xlsx" | "ppt" | "pptx"
        | "csv" | "md" | "epub" => FileCategory::Documents,
        // Images
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff" | "tif"
        | "psd" | "raw" | "cr2" | "nef" | "heic" | "heif" => FileCategory::Images,
        // Video
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" | "3gp" => {
            FileCategory::Video
        }
        // Audio
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "wma" | "m4a" | "opus" => FileCategory::Audio,
        // Archives
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "cab" | "iso" | "dmg" => {
            FileCategory::Archives
        }
        // Code
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "c" | "cpp" | "h" | "hpp" | "cs" | "java"
        | "go" | "rb" | "php" | "swift" | "kt" | "scala" | "html" | "css" | "scss" | "json"
        | "xml" | "yaml" | "yml" | "toml" | "sql" | "sh" | "bat" | "ps1" => FileCategory::Code,
        // Executables
        "exe" | "msi" | "dll" | "so" | "dylib" | "app" | "com" | "scr" => FileCategory::Executables,
        // System
        "sys" | "drv" | "inf" | "cat" | "log" | "etl" | "dat" | "reg" | "tmp" | "bak" => {
            FileCategory::System
        }
        _ => FileCategory::Other,
    }
}

/// Compute per-category size and count stats for the entire tree.
pub fn analyse_file_types(tree: &FileTree) -> Vec<CategoryStats> {
    // There are exactly 9 categories — pre-size to avoid rehashing.
    let mut map: HashMap<FileCategory, CategoryStats> = HashMap::with_capacity(9);

    for node in &tree.nodes {
        if node.is_dir {
            continue;
        }

        let ext = node.name.rsplit('.').next().unwrap_or("");
        let cat = categorise_extension(ext);

        let entry = map.entry(cat).or_insert_with(|| CategoryStats {
            category: Some(cat),
            total_size: 0,
            file_count: 0,
        });
        entry.total_size += node.size;
        entry.file_count += 1;
    }

    let mut results: Vec<CategoryStats> = map.into_values().collect();
    results.sort_by(|a, b| b.total_size.cmp(&a.total_size));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{file_node::FileNode, FileTree};
    use compact_str::CompactString;

    // ── categorise_extension ─────────────────────────────────────────────

    #[test]
    fn categorise_known_image_extensions() {
        for ext in &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "heic"] {
            assert_eq!(
                categorise_extension(ext),
                FileCategory::Images,
                "expected Images for .{ext}"
            );
        }
    }

    #[test]
    fn categorise_known_code_extensions() {
        for ext in &["rs", "py", "js", "ts", "c", "cpp", "go", "toml"] {
            assert_eq!(
                categorise_extension(ext),
                FileCategory::Code,
                "expected Code for .{ext}"
            );
        }
    }

    #[test]
    fn categorise_known_archive_extensions() {
        for ext in &["zip", "rar", "7z", "tar", "gz", "iso"] {
            assert_eq!(
                categorise_extension(ext),
                FileCategory::Archives,
                "expected Archives for .{ext}"
            );
        }
    }

    #[test]
    fn categorise_unknown_extension_returns_other() {
        assert_eq!(categorise_extension("xyz"), FileCategory::Other);
        assert_eq!(categorise_extension(""), FileCategory::Other);
    }

    /// Extension matching must be case-insensitive so "JPG" == "jpg".
    #[test]
    fn categorise_case_insensitive() {
        assert_eq!(categorise_extension("JPG"), FileCategory::Images);
        assert_eq!(categorise_extension("RS"), FileCategory::Code);
        assert_eq!(categorise_extension("ZIP"), FileCategory::Archives);
    }

    // ── analyse_file_types ───────────────────────────────────────────────

    /// A tree with two .rs files and one .png file should produce two
    /// non-zero categories: Code (total 200 B) and Images (100 B).
    #[test]
    fn analyse_aggregates_by_category() {
        let mut tree = FileTree::with_capacity(5);
        let root = tree.add_root(CompactString::new("C:"));

        let rs1 = tree.add_node(FileNode::new_file(
            CompactString::new("main.rs"),
            100,
            Some(root),
        ));
        tree.add_child(root, rs1);

        let rs2 = tree.add_node(FileNode::new_file(
            CompactString::new("lib.rs"),
            100,
            Some(root),
        ));
        tree.add_child(root, rs2);

        let img = tree.add_node(FileNode::new_file(
            CompactString::new("logo.png"),
            100,
            Some(root),
        ));
        tree.add_child(root, img);

        tree.aggregate_sizes();

        let stats = analyse_file_types(&tree);

        // Find Code and Images entries.
        let code = stats
            .iter()
            .find(|s| s.category == Some(FileCategory::Code))
            .expect("Code category missing");
        let images = stats
            .iter()
            .find(|s| s.category == Some(FileCategory::Images))
            .expect("Images category missing");

        assert_eq!(code.file_count, 2, "two .rs files");
        assert_eq!(code.total_size, 200);
        assert_eq!(images.file_count, 1);
        assert_eq!(images.total_size, 100);
    }

    /// Directories must not contribute to category stats.
    #[test]
    fn analyse_skips_directories() {
        let mut tree = FileTree::with_capacity(3);
        let root = tree.add_root(CompactString::new("C:"));
        let dir = tree.add_node(FileNode::new_dir(CompactString::new("src"), Some(root)));
        tree.add_child(root, dir);
        tree.aggregate_sizes();

        let stats = analyse_file_types(&tree);
        // The tree has only a root dir and one child dir — no files.
        assert!(
            stats.is_empty(),
            "expected no category stats when there are no files"
        );
    }

    /// An empty tree must return an empty result without panicking.
    #[test]
    fn analyse_empty_tree() {
        let tree = FileTree::with_capacity(0);
        let stats = analyse_file_types(&tree);
        assert!(stats.is_empty());
    }

    /// Results must be sorted by total_size descending so the largest
    /// category appears first.
    #[test]
    fn analyse_sorted_by_size_descending() {
        let mut tree = FileTree::with_capacity(4);
        let root = tree.add_root(CompactString::new("C:"));

        // .zip is 1000 bytes — Archives
        let z = tree.add_node(FileNode::new_file(
            CompactString::new("big.zip"),
            1_000,
            Some(root),
        ));
        tree.add_child(root, z);

        // .rs is only 10 bytes — Code
        let r = tree.add_node(FileNode::new_file(
            CompactString::new("small.rs"),
            10,
            Some(root),
        ));
        tree.add_child(root, r);

        tree.aggregate_sizes();

        let stats = analyse_file_types(&tree);
        assert!(stats.len() >= 2);
        assert!(
            stats[0].total_size >= stats[1].total_size,
            "must be descending"
        );
        assert_eq!(stats[0].category, Some(FileCategory::Archives));
    }
}
