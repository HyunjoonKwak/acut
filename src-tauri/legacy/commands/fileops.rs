use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOpResult {
    pub success: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<DirEntry>, String> {
    let dir = Path::new(&path);
    if !dir.exists() || !dir.is_dir() {
        return Err("Invalid directory".into());
    }

    let mut entries = Vec::new();

    // Parent directory
    if let Some(parent) = dir.parent() {
        entries.push(DirEntry {
            name: "..".into(),
            path: parent.to_string_lossy().into(),
            is_dir: true,
            size: 0,
            modified: String::new(),
        });
    }

    let mut dir_entries: Vec<DirEntry> = std::fs::read_dir(dir)
        .map_err(|e| format!("Read error: {}", e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();

            // Skip hidden files
            if name.starts_with('.') { return None; }

            let modified = meta.modified().ok()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();

            Some(DirEntry {
                name,
                path: entry.path().to_string_lossy().into(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                modified,
            })
        })
        .collect();

    // Sort: directories first, then alphabetical
    dir_entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    entries.extend(dir_entries);
    Ok(entries)
}

#[tauri::command]
pub async fn move_files(sources: Vec<String>, target_dir: String) -> Result<FileOpResult, String> {
    let target = Path::new(&target_dir);
    if !target.is_dir() {
        return Err("Target is not a directory".into());
    }

    let mut success = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for src in &sources {
        let src_path = Path::new(src);
        let file_name = src_path.file_name().unwrap_or_default();
        let dest = target.join(file_name);

        // Skip if same location
        if src_path.parent() == Some(target) { continue; }

        // Handle name conflict
        let final_dest = resolve_conflict(&dest);

        match std::fs::rename(src_path, &final_dest) {
            Ok(_) => success += 1,
            Err(_) => {
                // Try copy + delete for cross-volume
                match std::fs::copy(src_path, &final_dest) {
                    Ok(_) => {
                        let _ = std::fs::remove_file(src_path);
                        success += 1;
                    }
                    Err(e) => {
                        failed += 1;
                        errors.push(format!("{}: {}", file_name.to_string_lossy(), e));
                    }
                }
            }
        }
    }

    Ok(FileOpResult { success, failed, errors })
}

#[tauri::command]
pub async fn copy_files(sources: Vec<String>, target_dir: String) -> Result<FileOpResult, String> {
    let target = Path::new(&target_dir);
    if !target.is_dir() {
        return Err("Target is not a directory".into());
    }

    let mut success = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for src in &sources {
        let src_path = Path::new(src);
        let file_name = src_path.file_name().unwrap_or_default();
        let dest = target.join(file_name);
        let final_dest = resolve_conflict(&dest);

        match std::fs::copy(src_path, &final_dest) {
            Ok(_) => success += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("{}: {}", file_name.to_string_lossy(), e));
            }
        }
    }

    Ok(FileOpResult { success, failed, errors })
}

#[tauri::command]
pub async fn create_directory(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(|e| format!("Failed: {}", e))
}

// TreeSize-like folder analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderAnalysis {
    pub path: String,
    pub name: String,
    pub total_size: u64,
    pub file_count: u64,
    pub folder_count: u64,
    pub children: Vec<FolderAnalysis>,
    pub type_stats: Vec<TypeStat>,
    pub largest_files: Vec<LargeFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeStat {
    pub ext: String,
    pub count: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFile {
    pub name: String,
    pub path: String,
    pub size: u64,
}

#[tauri::command]
pub async fn analyze_folder(path: String, depth: Option<u32>) -> Result<FolderAnalysis, String> {
    let max_depth = depth.unwrap_or(2);
    analyze_recursive(Path::new(&path), max_depth).ok_or_else(|| "Analysis failed".into())
}

fn analyze_recursive(dir: &Path, depth: u32) -> Option<FolderAnalysis> {
    if !dir.is_dir() { return None; }

    let name = dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());

    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut children = Vec::new();
    let mut type_map: HashMap<String, (u64, u64)> = HashMap::new(); // ext -> (count, size)
    let mut large_files: Vec<LargeFile> = Vec::new();

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name.starts_with('.') { continue; }

        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        if is_dir {
            folder_count += 1;
            if depth > 0 {
                if let Some(child) = analyze_recursive(&entry.path(), depth - 1) {
                    total_size += child.total_size;
                    file_count += child.file_count;
                    folder_count += child.folder_count;
                    // Merge child type stats into parent
                    for ts in &child.type_stats {
                        let stat = type_map.entry(ts.ext.clone()).or_insert((0, 0));
                        stat.0 += ts.count;
                        stat.1 += ts.size;
                    }
                    // Merge child large files
                    large_files.extend(child.largest_files.iter().cloned());
                    children.push(child);
                }
            } else {
                let (s, f, ext_map, lf) = quick_size_with_stats(&entry.path());
                total_size += s;
                file_count += f;
                for (ext, (c, sz)) in ext_map {
                    let stat = type_map.entry(ext).or_insert((0, 0));
                    stat.0 += c;
                    stat.1 += sz;
                }
                large_files.extend(lf);
            }
        } else {
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            total_size += size;
            file_count += 1;

            let ext = entry.path().extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "other".into());
            let stat = type_map.entry(ext).or_insert((0, 0));
            stat.0 += 1;
            stat.1 += size;

            if size > 10_000_000 { // > 10MB
                large_files.push(LargeFile {
                    name: entry_name,
                    path: entry.path().to_string_lossy().into(),
                    size,
                });
            }
        }
    }

    // Sort children by size (largest first)
    children.sort_by(|a, b| b.total_size.cmp(&a.total_size));

    // Sort type stats by size
    let mut type_stats: Vec<TypeStat> = type_map.into_iter()
        .map(|(ext, (count, size))| TypeStat { ext, count, size })
        .collect();
    type_stats.sort_by(|a, b| b.size.cmp(&a.size));

    // Top 20 largest files
    large_files.sort_by(|a, b| b.size.cmp(&a.size));
    large_files.truncate(20);

    Some(FolderAnalysis {
        path: dir.to_string_lossy().into(),
        name,
        total_size,
        file_count,
        folder_count,
        children,
        type_stats,
        largest_files: large_files,
    })
}

fn quick_size_with_stats(dir: &Path) -> (u64, u64, HashMap<String, (u64, u64)>, Vec<LargeFile>) {
    let mut size = 0u64;
    let mut count = 0u64;
    let mut type_map: HashMap<String, (u64, u64)> = HashMap::new();
    let mut large_files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') { continue; }
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    let (s, c, em, lf) = quick_size_with_stats(&entry.path());
                    size += s;
                    count += c;
                    for (ext, (ec, es)) in em {
                        let stat = type_map.entry(ext).or_insert((0, 0));
                        stat.0 += ec;
                        stat.1 += es;
                    }
                    large_files.extend(lf);
                } else {
                    let fsize = meta.len();
                    size += fsize;
                    count += 1;
                    let ext = entry.path().extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_else(|| "other".into());
                    let stat = type_map.entry(ext).or_insert((0, 0));
                    stat.0 += 1;
                    stat.1 += fsize;
                    if fsize > 10_000_000 {
                        large_files.push(LargeFile {
                            name, path: entry.path().to_string_lossy().into(), size: fsize,
                        });
                    }
                }
            }
        }
    }
    (size, count, type_map, large_files)
}

// Deep folder tree with individual files for tree view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    pub file_type: String,
    pub children: Vec<TreeNode>,
    pub file_count: u64,
    pub folder_count: u64,
}

const QUICK_COUNT_MAX_DEPTH: u32 = 50;

fn format_modified(meta: &std::fs::Metadata) -> String {
    meta.modified()
        .ok()
        .map(|t| {
            chrono::DateTime::<chrono::Utc>::from(t)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_folder_tree(path: String, max_depth: Option<u32>) -> Result<TreeNode, String> {
    let dir = Path::new(&path);
    if !dir.exists() || !dir.is_dir() {
        return Err("Invalid directory path".into());
    }
    let depth = max_depth.unwrap_or(5);
    build_tree_node(dir, depth).ok_or_else(|| "Failed to read folder".into())
}

fn build_tree_node(path: &Path, depth: u32) -> Option<TreeNode> {
    // Use symlink_metadata to avoid following symlinks (prevents infinite loops)
    let meta = std::fs::symlink_metadata(path).ok()?;

    // Skip symlinks entirely
    if meta.file_type().is_symlink() {
        return None;
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let modified = format_modified(&meta);

    if !meta.is_dir() {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        return Some(TreeNode {
            name,
            path: path.to_string_lossy().into(),
            is_dir: false,
            size: meta.len(),
            modified,
            file_type: classify_file_type(&ext),
            children: Vec::new(),
            file_count: 0,
            folder_count: 0,
        });
    }

    let entries = std::fs::read_dir(path).ok()?;
    let mut children = Vec::new();
    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut folder_count = 0u64;

    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name.starts_with('.') {
            continue;
        }

        let entry_meta = match std::fs::symlink_metadata(&entry.path()) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip symlinks
        if entry_meta.file_type().is_symlink() {
            continue;
        }

        if entry_meta.is_dir() {
            folder_count += 1;
            if depth > 0 {
                if let Some(child) = build_tree_node(&entry.path(), depth - 1) {
                    total_size += child.size;
                    file_count += child.file_count;
                    folder_count += child.folder_count;
                    children.push(child);
                }
            } else {
                let (s, fc) = quick_count(&entry.path(), QUICK_COUNT_MAX_DEPTH);
                let entry_modified = format_modified(&entry_meta);
                total_size += s;
                file_count += fc;
                children.push(TreeNode {
                    name: entry_name,
                    path: entry.path().to_string_lossy().into(),
                    is_dir: true,
                    size: s,
                    modified: entry_modified,
                    file_type: "folder".into(),
                    children: Vec::new(),
                    file_count: fc,
                    folder_count: 0,
                });
            }
        } else {
            let fsize = entry_meta.len();
            total_size += fsize;
            file_count += 1;
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let entry_modified = format_modified(&entry_meta);
            children.push(TreeNode {
                name: entry_name,
                path: entry.path().to_string_lossy().into(),
                is_dir: false,
                size: fsize,
                modified: entry_modified,
                file_type: classify_file_type(&ext),
                children: Vec::new(),
                file_count: 0,
                folder_count: 0,
            });
        }
    }

    // Sort: folders first (by name), then files (by name)
    children.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Some(TreeNode {
        name,
        path: path.to_string_lossy().into(),
        is_dir: true,
        size: total_size,
        modified,
        file_type: "folder".into(),
        children,
        file_count,
        folder_count,
    })
}

fn quick_count(dir: &Path, max_depth: u32) -> (u64, u64) {
    if max_depth == 0 {
        return (0, 0);
    }
    let mut size = 0u64;
    let mut count = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if let Ok(meta) = std::fs::symlink_metadata(&entry.path()) {
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    let (s, c) = quick_count(&entry.path(), max_depth - 1);
                    size += s;
                    count += c;
                } else {
                    size += meta.len();
                    count += 1;
                }
            }
        }
    }
    (size, count)
}

fn classify_file_type(ext: &str) -> String {
    match ext {
        "jpg" | "jpeg" | "png" | "heic" | "webp" | "gif" | "bmp" | "tiff" | "tif" | "raw"
        | "cr2" | "nef" | "arw" | "dng" | "svg" | "ico" => "image".into(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "wmv" | "flv" => "video".into(),
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => "audio".into(),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "csv"
        | "md" => "document".into(),
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "dmg" | "iso" => "archive".into(),
        _ => "other".into(),
    }
}

// Scan for folders with YYYYMMDD date format and suggest renames to YYYY-MM-DD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateFolderMatch {
    pub path: String,
    pub current_name: String,
    pub suggested_name: String,
    pub has_conflict: bool,
    pub conflict_file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateFolderScanResult {
    pub matches: Vec<DateFolderMatch>,
    pub total_scanned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameResult {
    pub success: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

fn parse_yyyymmdd_prefix(name: &str) -> Option<(u32, u32, u32, &str)> {
    // Check first 8 chars are ASCII digits (safe for multibyte strings)
    let bytes = name.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    if !bytes[..8].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }

    // Safe to slice since we confirmed ASCII-only for first 8 bytes
    let date_part = &name[..8];
    let year: u32 = date_part[..4].parse().ok()?;
    let month: u32 = date_part[4..6].parse().ok()?;
    let day: u32 = date_part[6..8].parse().ok()?;

    // Basic date validation
    if year < 1900 || year > 2099 || month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    // Already has hyphens (YYYY-MM-DD) -> skip
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        return None;
    }

    let suffix = &name[8..];
    Some((year, month, day, suffix))
}

#[tauri::command]
pub async fn scan_date_folders(path: String, recursive: bool) -> Result<DateFolderScanResult, String> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err("Invalid directory".into());
    }

    let mut matches = Vec::new();
    let mut total_scanned = 0usize;
    scan_date_folders_recursive(root, recursive, &mut matches, &mut total_scanned);

    Ok(DateFolderScanResult { matches, total_scanned })
}

fn scan_date_folders_recursive(
    dir: &Path,
    recursive: bool,
    matches: &mut Vec<DateFolderMatch>,
    total_scanned: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !meta.is_dir() {
            continue;
        }

        *total_scanned += 1;

        if let Some((year, month, day, suffix)) = parse_yyyymmdd_prefix(&name) {
            let suggested = format!("{:04}-{:02}-{:02}{}", year, month, day, suffix);
            let dest = dir.join(&suggested);
            let (has_conflict, conflict_file_count) = if dest.is_dir() {
                let count = std::fs::read_dir(&dest)
                    .map(|rd| rd.flatten().count() as u64)
                    .unwrap_or(0);
                (true, count)
            } else {
                (false, 0)
            };
            matches.push(DateFolderMatch {
                path: entry.path().to_string_lossy().into(),
                current_name: name.clone(),
                suggested_name: suggested,
                has_conflict,
                conflict_file_count,
            });
        }

        if recursive {
            scan_date_folders_recursive(&entry.path(), true, matches, total_scanned);
        }
    }
}

// Move all contents from src folder into dest folder, then remove empty src
fn merge_folder_contents(src: &Path, dest: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let entry_name = entry.file_name();
        let target = dest.join(&entry_name);

        if target.exists() {
            // Conflict within merge: add suffix to avoid overwrite
            let resolved = resolve_conflict(&target);
            std::fs::rename(entry.path(), &resolved).map_err(|e| e.to_string())?;
        } else {
            std::fs::rename(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    // Remove the now-empty source folder
    std::fs::remove_dir(src).map_err(|e| format!("remove empty src: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn rename_date_folders(folders: Vec<DateFolderMatch>, merge: bool) -> Result<RenameResult, String> {
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    for item in &folders {
        let src = Path::new(&item.path);
        let parent = match src.parent() {
            Some(p) => p,
            None => {
                failed += 1;
                errors.push(format!("{}: no parent directory", item.current_name));
                continue;
            }
        };
        let dest = parent.join(&item.suggested_name);

        if dest.exists() {
            if !merge {
                failed += 1;
                errors.push(format!("{}: target already exists", item.suggested_name));
                continue;
            }
            // Merge: move all contents from src into dest
            match merge_folder_contents(src, &dest) {
                Ok(_) => success += 1,
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{}: merge failed - {}", item.current_name, e));
                }
            }
        } else {
            match std::fs::rename(src, &dest) {
                Ok(_) => success += 1,
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{}: {}", item.current_name, e));
                }
            }
        }
    }

    Ok(RenameResult { success, failed, errors })
}

fn resolve_conflict(dest: &Path) -> std::path::PathBuf {
    if !dest.exists() { return dest.to_path_buf(); }

    let stem = dest.file_stem().unwrap_or_default().to_string_lossy();
    let ext = dest.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = dest.parent().unwrap();

    for i in 1..1000 {
        let new_name = format!("{} ({}){}", stem, i, ext);
        let new_path = parent.join(&new_name);
        if !new_path.exists() { return new_path; }
    }
    dest.to_path_buf()
}

// Trash files by path (for photo reviewer - no DB cleanup needed)
#[tauri::command]
pub async fn trash_review_files(file_paths: Vec<String>) -> Result<FileOpResult, String> {
    let result = crate::core::trash::trash_via_finder(&file_paths);
    Ok(FileOpResult {
        success: result.success,
        failed: result.failed,
        errors: result.errors,
    })
}

// ---------------------------------------------------------------------------
// Folder-level operations (A-Cut) — copy / move / rename / trash
// ---------------------------------------------------------------------------

use crate::core::undo as undo_core;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

/// Recursively copy a directory tree, counting files copied.
fn copy_dir_all(src: &Path, dst: &Path, copied: &mut usize) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target, copied)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            *copied += 1;
        }
    }
    Ok(())
}

/// Keep library rows pointing at the right place after a move/rename.
fn sync_db_paths(db: &Arc<Database>, src: &str, dst: &str, is_dir: bool) {
    let _ = db.with_conn(|conn| {
        if is_dir {
            let old_prefix = format!("{}/", src.trim_end_matches('/'));
            let new_prefix = format!("{}/", dst.trim_end_matches('/'));
            conn.execute(
                "UPDATE media_files
                 SET file_path = ?2 || SUBSTR(file_path, LENGTH(?1) + 1)
                 WHERE file_path LIKE ?1 || '%'",
                rusqlite::params![old_prefix, new_prefix],
            )?;
            conn.execute(
                "UPDATE source_folders
                 SET path = ?2 || SUBSTR(path, LENGTH(?1) + 1)
                 WHERE path LIKE ?1 || '%'",
                rusqlite::params![old_prefix, new_prefix],
            )?;
            conn.execute(
                "UPDATE source_folders SET path = ?2 WHERE path = ?1",
                rusqlite::params![src, dst],
            )?;
        } else {
            conn.execute(
                "UPDATE media_files SET file_path = ?2 WHERE file_path = ?1",
                rusqlite::params![src, dst],
            )?;
        }
        Ok(())
    });
}

/// Recursively copy a directory (or a single file) into `target_dir`,
/// resolving name conflicts. Returns the number of files copied.
#[tauri::command]
pub async fn copy_directory(source: String, target_dir: String) -> Result<FileOpResult, String> {
    let src = Path::new(&source);
    let target = Path::new(&target_dir);
    if !src.exists() {
        return Err("원본 경로가 존재하지 않습니다".into());
    }
    if !target.is_dir() {
        return Err("대상이 폴더가 아닙니다".into());
    }
    if src.is_dir() && target.starts_with(src) {
        return Err("대상 폴더가 원본 안에 있어 복사할 수 없습니다".into());
    }
    let file_name = src.file_name().ok_or("잘못된 원본 경로입니다")?;
    let dest = resolve_conflict(&target.join(file_name));

    let mut copied = 0usize;
    let result = if src.is_dir() {
        copy_dir_all(src, &dest, &mut copied)
    } else {
        std::fs::copy(src, &dest).map(|_| {
            copied += 1;
        })
    };

    match result {
        Ok(()) => Ok(FileOpResult {
            success: copied,
            failed: 0,
            errors: Vec::new(),
        }),
        Err(e) => Err(format!("복사 실패: {}", e)),
    }
}

/// Move files or folders into `target_dir`. Cross-volume moves fall back to
/// copy + delete. Every move is recorded in the undo journal, and library
/// paths follow automatically.
#[tauri::command]
pub async fn move_paths(
    db: State<'_, Arc<Database>>,
    sources: Vec<String>,
    target_dir: String,
) -> Result<FileOpResult, String> {
    let target = Path::new(&target_dir);
    if !target.is_dir() {
        return Err("대상이 폴더가 아닙니다".into());
    }
    let db_ref = db.inner().clone();

    let batch_id = uuid::Uuid::new_v4().to_string();
    let batch_name = "이동";
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut seq: i32 = 0;

    for src_str in &sources {
        let src = Path::new(src_str);
        if !src.exists() {
            failed += 1;
            errors.push(format!("{}: 경로가 존재하지 않습니다", src_str));
            continue;
        }
        let is_dir = src.is_dir();
        if is_dir && target.starts_with(src) {
            failed += 1;
            errors.push(format!("{}: 대상이 원본 안에 있습니다", src_str));
            continue;
        }
        let Some(name) = src.file_name() else {
            failed += 1;
            errors.push(format!("{}: 잘못된 경로", src_str));
            continue;
        };
        let dest = resolve_conflict(&target.join(name));
        let dest_str = dest.to_string_lossy().to_string();

        let moved = std::fs::rename(src, &dest).or_else(|_| {
            // Cross-volume fallback
            if is_dir {
                let mut copied = 0usize;
                copy_dir_all(src, &dest, &mut copied)?;
                std::fs::remove_dir_all(src)
            } else {
                std::fs::copy(src, &dest)?;
                std::fs::remove_file(src)
            }
        });

        match moved {
            Ok(()) => {
                success += 1;
                let op = if is_dir { "move_dir" } else { "move" };
                let _ = db_ref.with_conn(|conn| {
                    undo_core::record_operation(
                        conn,
                        &batch_id,
                        batch_name,
                        seq,
                        op,
                        Some(src_str),
                        Some(&dest_str),
                        None,
                    )?;
                    Ok(())
                });
                seq += 1;
                sync_db_paths(&db_ref, src_str, &dest_str, is_dir);
            }
            Err(e) => {
                failed += 1;
                errors.push(format!("{}: {}", src_str, e));
            }
        }
    }

    Ok(FileOpResult {
        success,
        failed,
        errors,
    })
}

/// Rename a single file or folder in place. Returns the new full path.
#[tauri::command]
pub async fn rename_path(
    db: State<'_, Arc<Database>>,
    path: String,
    new_name: String,
) -> Result<String, String> {
    let src = Path::new(&path);
    if !src.exists() {
        return Err("경로가 존재하지 않습니다".into());
    }
    let trimmed = new_name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\0')
        || trimmed == "."
        || trimmed == ".."
    {
        return Err("사용할 수 없는 이름입니다".into());
    }
    let parent = src.parent().ok_or("최상위 경로는 이름을 바꿀 수 없습니다")?;
    let dest = parent.join(trimmed);
    if dest.exists() {
        return Err("같은 이름의 항목이 이미 있습니다".into());
    }

    let is_dir = src.is_dir();
    std::fs::rename(src, &dest).map_err(|e| format!("이름 변경 실패: {}", e))?;
    let dest_str = dest.to_string_lossy().to_string();

    let db_ref = db.inner().clone();
    let batch_id = uuid::Uuid::new_v4().to_string();
    let op = if is_dir { "move_dir" } else { "move" };
    let _ = db_ref.with_conn(|conn| {
        undo_core::record_operation(
            conn,
            &batch_id,
            "이름 변경",
            0,
            op,
            Some(&path),
            Some(&dest_str),
            None,
        )?;
        Ok(())
    });
    sync_db_paths(&db_ref, &path, &dest_str, is_dir);

    Ok(dest_str)
}

/// Move files or folders to the macOS Trash. Library rows under the trashed
/// paths are purged — the Finder Trash remains the safety net.
#[tauri::command]
pub async fn trash_paths(
    db: State<'_, Arc<Database>>,
    paths: Vec<String>,
) -> Result<FileOpResult, String> {
    let result = crate::core::trash::trash_via_finder(&paths);
    let db_ref = db.inner().clone();

    // Following the existing convention: the first `success` paths succeeded
    let trashed: Vec<&String> = paths.iter().take(result.success).collect();
    if !trashed.is_empty() {
        let _ = db_ref.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for p in &trashed {
                let prefix = format!("{}/%", p.trim_end_matches('/'));
                tx.execute(
                    "DELETE FROM duplicate_members WHERE media_id IN
                     (SELECT id FROM media_files WHERE file_path = ?1 OR file_path LIKE ?2)",
                    rusqlite::params![p, prefix],
                )?;
                tx.execute(
                    "DELETE FROM bcut_members WHERE media_id IN
                     (SELECT id FROM media_files WHERE file_path = ?1 OR file_path LIKE ?2)",
                    rusqlite::params![p, prefix],
                )?;
                tx.execute(
                    "DELETE FROM media_files WHERE file_path = ?1 OR file_path LIKE ?2",
                    rusqlite::params![p, prefix],
                )?;
                tx.execute(
                    "DELETE FROM source_folders WHERE path = ?1 OR path LIKE ?2",
                    rusqlite::params![p, prefix],
                )?;
            }
            tx.commit()?;
            Ok(())
        });
    }

    Ok(FileOpResult {
        success: result.success,
        failed: result.failed,
        errors: result.errors,
    })
}
