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
pub fn categorise_extension(ext: &str) -> FileCategory {
    match ext.to_ascii_lowercase().as_str() {
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
    let mut map: HashMap<FileCategory, CategoryStats> = HashMap::new();

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
