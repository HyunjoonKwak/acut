use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use walkdir::WalkDir;

use crate::core::hasher;

/// Global cancellation flag checked during sync execution.
pub static SYNC_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Chunk size for file copy operations (1 MB).
const COPY_CHUNK_SIZE: usize = 1024 * 1024;
static COPY_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTask {
    pub id: String,
    pub name: String,
    pub source_dir: String,
    pub target_dir: String,
    pub exclusion_patterns: Vec<String>,
    pub verify_checksum: bool,
    pub detect_orphans: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileOp {
    pub source: String,
    pub target: String,
    pub file_size: u64,
    /// One of `"new"`, `"modified"`, `"size_changed"`.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub source_path: String,
    pub target_path: String,
    /// ISO-8601 formatted modification time of the source file.
    pub source_modified: String,
    /// ISO-8601 formatted modification time of the target file.
    pub target_modified: String,
    pub source_size: u64,
    pub target_size: u64,
    /// Whether the two files have identical xxHash content.
    pub content_identical: bool,
    /// One of `"pending"`, `"force_copy"`, `"rename_copy"`, `"skip"`.
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub files_to_copy: Vec<SyncFileOp>,
    pub files_to_update: Vec<SyncFileOp>,
    pub conflicts: Vec<SyncConflict>,
    pub orphan_files: Vec<String>,
    pub total_bytes: u64,
    pub total_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub files_copied: usize,
    pub files_updated: usize,
    pub files_skipped: usize,
    pub bytes_transferred: u64,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub current_file: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a `std::time::SystemTime` as an ISO-8601 string via `chrono`.
fn format_system_time(t: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = t.into();
    datetime.to_rfc3339()
}

/// Return `true` if `relative_path` matches any of the glob `patterns`.
///
/// Each pattern is compiled via the `glob` crate's `Pattern` and matched
/// against the *relative* path (using forward slashes for consistency).
fn is_excluded(relative_path: &str, patterns: &[String]) -> bool {
    // Normalise to forward slashes so patterns work cross-platform.
    let normalised = relative_path.replace('\\', "/");

    for raw_pattern in patterns {
        if let Ok(pattern) = glob::Pattern::new(raw_pattern) {
            let opts = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            };
            if pattern.matches_with(&normalised, opts) {
                return true;
            }
            // Also check against just the file-name component so that a
            // simple pattern like "*.tmp" matches "subdir/foo.tmp".
            if let Some(file_name) = Path::new(&normalised).file_name().and_then(|n| n.to_str()) {
                if pattern.matches_with(file_name, opts) {
                    return true;
                }
            }
        }
    }

    false
}

/// Copy to a temporary sibling, verify it, and atomically replace `target`
/// only after the complete copy is durable.
///
/// Returns the total number of bytes copied.
fn copy_file_chunked(
    source: &Path,
    target: &Path,
    progress: &mut impl FnMut(u64),
) -> Result<u64, String> {
    let src_file = fs::File::open(source)
        .map_err(|e| format!("Failed to open source {}: {}", source.display(), e))?;
    let parent = target
        .parent()
        .ok_or_else(|| format!("Target has no parent: {}", target.display()))?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("copy");
    let (temp_path, dst_file) = (0..100)
        .find_map(|_| {
            let seq = COPY_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".{name}.acut-copy-{}-{seq}.tmp",
                std::process::id()
            ));
            match fs::File::options().write(true).create_new(true).open(&path) {
                Ok(file) => Some(Ok((path, file))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(format!(
                    "Failed to create temporary target {}: {e}",
                    path.display()
                ))),
            }
        })
        .ok_or_else(|| {
            format!(
                "Could not allocate a temporary target beside {}",
                target.display()
            )
        })??;

    struct RemoveOnDrop(Option<PathBuf>);
    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            if let Some(path) = self.0.take() {
                let _ = fs::remove_file(path);
            }
        }
    }
    let mut cleanup = RemoveOnDrop(Some(temp_path.clone()));

    let mut reader = BufReader::with_capacity(COPY_CHUNK_SIZE, src_file);
    let mut writer = BufWriter::with_capacity(COPY_CHUNK_SIZE, dst_file);

    let mut total_written: u64 = 0;
    let mut buf = vec![0u8; COPY_CHUNK_SIZE];

    loop {
        let bytes_read = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error on {}: {}", source.display(), e))?;
        if bytes_read == 0 {
            break;
        }
        writer
            .write_all(&buf[..bytes_read])
            .map_err(|e| format!("Write error on {}: {}", temp_path.display(), e))?;
        total_written += bytes_read as u64;
        progress(total_written);
    }

    writer
        .flush()
        .map_err(|e| format!("Flush error on {}: {}", temp_path.display(), e))?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|e| format!("Sync error on {}: {}", temp_path.display(), e))?;

    // Preserve the original modification time.
    if let Ok(src_meta) = fs::metadata(source) {
        if let Ok(mtime) = src_meta.modified() {
            let _ = filetime_set(&temp_path, mtime);
        }
    }

    let source_hash = hasher::xxhash_file(source)
        .ok_or_else(|| format!("Failed to hash source {}", source.display()))?;
    let copied_hash = hasher::xxhash_file(&temp_path)
        .ok_or_else(|| format!("Failed to hash temporary copy {}", temp_path.display()))?;
    if source_hash != copied_hash {
        return Err(format!(
            "Checksum mismatch while copying {}",
            source.display()
        ));
    }

    fs::rename(&temp_path, target).map_err(|e| {
        format!(
            "Failed to atomically replace {} with {}: {e}",
            target.display(),
            temp_path.display()
        )
    })?;
    cleanup.0.take();

    Ok(total_written)
}

/// Best-effort helper to set a file's modification time on supported platforms.
fn filetime_set(path: &Path, mtime: std::time::SystemTime) -> Result<(), String> {
    // We use `fs::File::set_modified` which is available on stable Rust >=1.75
    // via the `File::set_modified` method (stabilised in 1.75).
    let file = fs::File::options()
        .write(true)
        .open(path)
        .map_err(|e| format!("{}", e))?;
    file.set_modified(mtime).map_err(|e| format!("{}", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// Compare `source_dir` and `target_dir`, producing a [`SyncPlan`] that
/// describes what a subsequent [`execute_sync`] call would do.
///
/// This is a *dry-run*: nothing on disk is modified.
pub fn plan_sync(task: &SyncTask) -> Result<SyncPlan, String> {
    let source_root = PathBuf::from(&task.source_dir);
    let target_root = PathBuf::from(&task.target_dir);

    if !source_root.is_dir() {
        return Err(format!(
            "Source directory does not exist: {}",
            source_root.display()
        ));
    }

    let mut files_to_copy: Vec<SyncFileOp> = Vec::new();
    let mut files_to_update: Vec<SyncFileOp> = Vec::new();
    let mut conflicts: Vec<SyncConflict> = Vec::new();
    let mut total_bytes: u64 = 0;

    // Collect all source relative paths (for orphan detection later).
    let mut source_relative_paths: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Walk source tree -------------------------------------------------
    for entry in WalkDir::new(&source_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let abs_source = entry.path();
        let rel_path = abs_source
            .strip_prefix(&source_root)
            .map_err(|e| format!("strip_prefix error: {}", e))?;
        let rel_str = rel_path.to_string_lossy().to_string();

        // Exclusion check
        if is_excluded(&rel_str, &task.exclusion_patterns) {
            continue;
        }

        source_relative_paths.insert(rel_str.clone());

        let abs_target = target_root.join(rel_path);
        let src_meta = fs::metadata(abs_source)
            .map_err(|e| format!("Cannot stat source {}: {}", abs_source.display(), e))?;
        let src_size = src_meta.len();
        let src_mtime = src_meta
            .modified()
            .map_err(|e| format!("Cannot read mtime of {}: {}", abs_source.display(), e))?;

        if abs_target.exists() {
            let tgt_meta = fs::metadata(&abs_target)
                .map_err(|e| format!("Cannot stat target {}: {}", abs_target.display(), e))?;
            let tgt_size = tgt_meta.len();
            let tgt_mtime = tgt_meta
                .modified()
                .map_err(|e| format!("Cannot read mtime of {}: {}", abs_target.display(), e))?;

            if tgt_mtime > src_mtime {
                // Conflict: target is newer than source.
                let content_identical = if task.verify_checksum {
                    let src_hash = hasher::xxhash_file(abs_source);
                    let tgt_hash = hasher::xxhash_file(&abs_target);
                    src_hash.is_some() && tgt_hash.is_some() && src_hash == tgt_hash
                } else {
                    // Without checksum, compare by size as a heuristic.
                    src_size == tgt_size
                };

                conflicts.push(SyncConflict {
                    source_path: abs_source.to_string_lossy().to_string(),
                    target_path: abs_target.to_string_lossy().to_string(),
                    source_modified: format_system_time(src_mtime),
                    target_modified: format_system_time(tgt_mtime),
                    source_size: src_size,
                    target_size: tgt_size,
                    content_identical,
                    resolution: "pending".to_string(),
                });
            } else if src_size != tgt_size {
                // Different size -> update.
                files_to_update.push(SyncFileOp {
                    source: abs_source.to_string_lossy().to_string(),
                    target: abs_target.to_string_lossy().to_string(),
                    file_size: src_size,
                    reason: "size_changed".to_string(),
                });
                total_bytes += src_size;
            } else if src_mtime > tgt_mtime {
                // Source is newer -> update.
                files_to_update.push(SyncFileOp {
                    source: abs_source.to_string_lossy().to_string(),
                    target: abs_target.to_string_lossy().to_string(),
                    file_size: src_size,
                    reason: "modified".to_string(),
                });
                total_bytes += src_size;
            }
            // else: identical mtime & size -> nothing to do.
        } else {
            // Target does not exist -> new file.
            files_to_copy.push(SyncFileOp {
                source: abs_source.to_string_lossy().to_string(),
                target: abs_target.to_string_lossy().to_string(),
                file_size: src_size,
                reason: "new".to_string(),
            });
            total_bytes += src_size;
        }
    }

    // Orphan detection -------------------------------------------------
    let mut orphan_files: Vec<String> = Vec::new();

    if task.detect_orphans && target_root.is_dir() {
        for entry in WalkDir::new(&target_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let abs_target = entry.path();
            let rel_path = abs_target
                .strip_prefix(&target_root)
                .map_err(|e| format!("strip_prefix error: {}", e))?;
            let rel_str = rel_path.to_string_lossy().to_string();

            if is_excluded(&rel_str, &task.exclusion_patterns) {
                continue;
            }

            if !source_relative_paths.contains(&rel_str) {
                orphan_files.push(abs_target.to_string_lossy().to_string());
            }
        }
    }

    let total_files = files_to_copy.len() + files_to_update.len();

    Ok(SyncPlan {
        files_to_copy,
        files_to_update,
        conflicts,
        orphan_files,
        total_bytes,
        total_files,
    })
}

// ---------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------

/// Execute a previously computed [`SyncPlan`].
///
/// Files in `files_to_copy` and `files_to_update` are transferred using 1 MB
/// chunked copies. Conflicts are handled according to their `resolution`
/// field.  The global [`SYNC_CANCELLED`] flag is checked between each file;
/// set it to `true` from another thread to abort the sync gracefully.
///
/// `progress_callback` is invoked periodically with the current
/// [`SyncProgress`] so that the frontend can display a progress bar.
pub fn execute_sync(plan: &SyncPlan, progress_callback: impl Fn(SyncProgress)) -> SyncResult {
    // Reset cancellation flag at start.
    SYNC_CANCELLED.store(false, Ordering::SeqCst);

    let mut result = SyncResult {
        files_copied: 0,
        files_updated: 0,
        files_skipped: 0,
        bytes_transferred: 0,
        errors: Vec::new(),
        cancelled: false,
    };

    // Merge all operations into a single work list so we can report a single
    // progress counter.
    struct WorkItem {
        source: String,
        target: String,
        file_size: u64,
        is_update: bool,
    }

    let mut work: Vec<WorkItem> = Vec::new();

    for op in &plan.files_to_copy {
        work.push(WorkItem {
            source: op.source.clone(),
            target: op.target.clone(),
            file_size: op.file_size,
            is_update: false,
        });
    }

    for op in &plan.files_to_update {
        work.push(WorkItem {
            source: op.source.clone(),
            target: op.target.clone(),
            file_size: op.file_size,
            is_update: true,
        });
    }

    // Include resolved conflicts.
    for conflict in &plan.conflicts {
        match conflict.resolution.as_str() {
            "force_copy" => {
                work.push(WorkItem {
                    source: conflict.source_path.clone(),
                    target: conflict.target_path.clone(),
                    file_size: conflict.source_size,
                    is_update: true,
                });
            }
            "rename_copy" => {
                // Rename the existing target to .bak, then copy the source.
                let bak_path = format!("{}.bak", conflict.target_path);
                if let Err(e) = fs::rename(&conflict.target_path, &bak_path) {
                    result.errors.push(format!(
                        "Failed to rename {} -> {}: {}",
                        conflict.target_path, bak_path, e
                    ));
                    continue;
                }
                work.push(WorkItem {
                    source: conflict.source_path.clone(),
                    target: conflict.target_path.clone(),
                    file_size: conflict.source_size,
                    is_update: true,
                });
            }
            "skip" | "pending" => {
                result.files_skipped += 1;
            }
            other => {
                result.errors.push(format!(
                    "Unknown conflict resolution '{}' for {}",
                    other, conflict.source_path
                ));
                result.files_skipped += 1;
            }
        }
    }

    let total_items = work.len();
    let bytes_total = work.iter().map(|w| w.file_size).sum::<u64>();
    let mut bytes_done_global: u64 = 0;

    // Process each work item -------------------------------------------
    for (idx, item) in work.iter().enumerate() {
        // Check cancellation.
        if SYNC_CANCELLED.load(Ordering::SeqCst) {
            result.cancelled = true;
            break;
        }

        // Emit progress at the start of each file.
        progress_callback(SyncProgress {
            phase: if item.is_update {
                "updating".to_string()
            } else {
                "copying".to_string()
            },
            current: idx + 1,
            total: total_items,
            current_file: item.source.clone(),
            bytes_done: bytes_done_global,
            bytes_total,
        });

        let source_path = Path::new(&item.source);
        let target_path = Path::new(&item.target);

        // Ensure parent directories exist.
        if let Some(parent) = target_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                result.errors.push(format!(
                    "Cannot create directory {}: {}",
                    parent.display(),
                    e
                ));
                result.files_skipped += 1;
                continue;
            }
        }

        // Chunked copy with per-file progress.
        let bytes_before = bytes_done_global;
        let mut file_progress = |bytes_written: u64| {
            let updated_global = bytes_before + bytes_written;
            progress_callback(SyncProgress {
                phase: if item.is_update {
                    "updating".to_string()
                } else {
                    "copying".to_string()
                },
                current: idx + 1,
                total: total_items,
                current_file: item.source.clone(),
                bytes_done: updated_global,
                bytes_total,
            });
        };

        match copy_file_chunked(source_path, target_path, &mut file_progress) {
            Ok(bytes) => {
                bytes_done_global += bytes;
                result.bytes_transferred += bytes;
                if item.is_update {
                    result.files_updated += 1;
                } else {
                    result.files_copied += 1;
                }
            }
            Err(e) => {
                result.errors.push(e);
                result.files_skipped += 1;
            }
        }
    }

    // Final progress event.
    progress_callback(SyncProgress {
        phase: if result.cancelled {
            "cancelled".to_string()
        } else {
            "done".to_string()
        },
        current: total_items,
        total: total_items,
        current_file: String::new(),
        bytes_done: bytes_done_global,
        bytes_total,
    });

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded_glob_star() {
        let patterns = vec!["*.tmp".to_string(), "thumbs.db".to_string()];
        assert!(is_excluded("foo.tmp", &patterns));
        assert!(is_excluded("sub/dir/foo.tmp", &patterns));
        assert!(is_excluded("thumbs.db", &patterns));
        assert!(!is_excluded("photo.jpg", &patterns));
    }

    #[test]
    fn test_is_excluded_directory_glob() {
        let patterns = vec![".git/**".to_string()];
        assert!(is_excluded(".git/config", &patterns));
        assert!(is_excluded(".git/objects/abc", &patterns));
        assert!(!is_excluded("src/main.rs", &patterns));
    }

    #[test]
    fn test_is_excluded_empty_patterns() {
        let patterns: Vec<String> = vec![];
        assert!(!is_excluded("anything.txt", &patterns));
    }

    #[test]
    fn chunked_copy_replaces_an_existing_target_only_after_success() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        let target = dir.path().join("target.bin");
        fs::write(&source, vec![7u8; COPY_CHUNK_SIZE + 17]).unwrap();
        fs::write(&target, b"old backup").unwrap();

        let copied = copy_file_chunked(&source, &target, &mut |_| {}).unwrap();
        assert_eq!(copied, COPY_CHUNK_SIZE as u64 + 17);
        assert_eq!(fs::read(&target).unwrap(), fs::read(&source).unwrap());
        assert_eq!(
            fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().contains("acut-copy"))
                .count(),
            0,
            "임시 파일이 남으면 안 된다"
        );
    }

    #[test]
    fn a_failed_copy_keeps_the_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.bin");
        fs::write(&target, b"known good backup").unwrap();
        assert!(copy_file_chunked(&dir.path().join("missing.bin"), &target, &mut |_| {}).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"known good backup");
    }
}
