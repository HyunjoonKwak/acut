use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif", "avif", "raw",
    "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "pef", "sr2", "raf",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "avi", "mkv", "wmv", "flv", "webm", "m4v", "3gp", "mts", "m2ts",
];

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub media_type: MediaType,
    pub created_at: String,
    pub modified_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaType {
    Image,
    Video,
}

impl MediaType {
    pub fn as_str(&self) -> &str {
        match self {
            MediaType::Image => "image",
            MediaType::Video => "video",
        }
    }
}

use crate::core::config::ScanConfig;

/// Scan a directory tree for media files.
/// System/junk directories are pruned per the user-editable `ScanConfig`
/// (defaults cover Library, System, .app bundles, node_modules, …) so a
/// whole-disk scan (e.g. adding "Macintosh HD" as a source) stays practical.
pub fn scan_directory(dir: &Path, exclude: &ScanConfig) -> Vec<ScannedFile> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            // Never prune the root the user explicitly picked
            if e.depth() == 0 || !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
            if exclude
                .exclude_dir_names
                .iter()
                .any(|n| n == name.as_ref())
            {
                return false;
            }
            if exclude
                .exclude_suffixes
                .iter()
                .any(|s| !s.is_empty() && name.ends_with(s.as_str()))
            {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        // Skip hidden/system files
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.starts_with("._")
            || file_name.starts_with('.')
            || file_name == "Thumbs.db"
            || file_name == "desktop.ini"
        {
            continue;
        }

        // Skip files inside hidden directories
        let path_str = path.to_string_lossy();
        if path_str.contains("/.") || path_str.contains("__MACOSX") {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            let media_type = if IMAGE_EXTENSIONS.contains(&ext_lower.as_str()) {
                Some(MediaType::Image)
            } else if VIDEO_EXTENSIONS.contains(&ext_lower.as_str()) {
                Some(MediaType::Video)
            } else {
                None
            };

            if let Some(media_type) = media_type {
                let metadata = std::fs::metadata(path).ok();
                let file_size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

                let created_at = metadata
                    .as_ref()
                    .and_then(|m| m.created().ok())
                    .map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t)
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();

                let modified_at = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t)
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();

                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                files.push(ScannedFile {
                    path: path.to_path_buf(),
                    file_name,
                    file_size,
                    media_type,
                    created_at,
                    modified_at,
                });
            }
        }
    }

    files
}

pub fn get_mime_type(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg".to_string(),
            "png" => "image/png".to_string(),
            "gif" => "image/gif".to_string(),
            "bmp" => "image/bmp".to_string(),
            "webp" => "image/webp".to_string(),
            "tiff" | "tif" => "image/tiff".to_string(),
            "heic" | "heif" => "image/heif".to_string(),
            "avif" => "image/avif".to_string(),
            "mp4" => "video/mp4".to_string(),
            "mov" => "video/quicktime".to_string(),
            "avi" => "video/x-msvideo".to_string(),
            "mkv" => "video/x-matroska".to_string(),
            "webm" => "video/webm".to_string(),
            other => format!("application/{}", other),
        })
}
