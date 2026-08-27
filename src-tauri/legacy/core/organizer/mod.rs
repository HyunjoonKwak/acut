use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::db::queries::UndoEntry;

/// Chunk size for file copy operations (1 MB).
const COPY_CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MoveOperation {
    pub source: PathBuf,
    pub target: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct OrganizeResult {
    pub moved: Vec<MoveOperation>,
    pub failed: Vec<(MoveOperation, String)>,
}

pub fn execute_moves(operations: &[MoveOperation]) -> OrganizeResult {
    let mut moved = Vec::new();
    let mut failed = Vec::new();

    for op in operations {
        if let Some(parent) = op.target.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                failed.push((op.clone(), format!("Failed to create directory: {}", e)));
                continue;
            }
        }

        match fs::rename(&op.source, &op.target) {
            Ok(_) => moved.push(op.clone()),
            Err(e) => {
                // Try copy + delete for cross-device moves
                match fs::copy(&op.source, &op.target) {
                    Ok(_) => {
                        if let Err(e) = fs::remove_file(&op.source) {
                            failed.push((
                                op.clone(),
                                format!("Copied but failed to remove source: {}", e),
                            ));
                        } else {
                            moved.push(op.clone());
                        }
                    }
                    Err(_) => {
                        failed.push((op.clone(), format!("Failed to move file: {}", e)));
                    }
                }
            }
        }
    }

    OrganizeResult { moved, failed }
}

pub fn execute_moves_with_progress(
    operations: &[MoveOperation],
    progress: impl Fn(usize, usize, &str),
) -> OrganizeResult {
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    let total = operations.len();

    for (idx, op) in operations.iter().enumerate() {
        let file_name = op.source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| op.source.to_string_lossy().into_owned());
        progress(idx + 1, total, &file_name);

        if let Some(parent) = op.target.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                failed.push((op.clone(), format!("Failed to create directory: {}", e)));
                continue;
            }
        }

        // Try fast rename first (same device)
        match fs::rename(&op.source, &op.target) {
            Ok(_) => {
                moved.push(op.clone());
            }
            Err(_) => {
                // Cross-device: use chunked copy with 1MB buffers
                match copy_file_chunked_organizer(&op.source, &op.target) {
                    Ok(_) => {
                        if let Err(e) = fs::remove_file(&op.source) {
                            failed.push((
                                op.clone(),
                                format!("Copied but failed to remove source: {}", e),
                            ));
                        } else {
                            moved.push(op.clone());
                        }
                    }
                    Err(e) => {
                        failed.push((op.clone(), format!("Failed to move file: {}", e)));
                    }
                }
            }
        }
    }

    OrganizeResult { moved, failed }
}

/// Copy a file in 1 MB chunks (for cross-device moves in the organizer).
fn copy_file_chunked_organizer(source: &Path, target: &Path) -> Result<u64, String> {
    let src_file = fs::File::open(source)
        .map_err(|e| format!("Failed to open source {}: {}", source.display(), e))?;
    let dst_file = fs::File::create(target)
        .map_err(|e| format!("Failed to create target {}: {}", target.display(), e))?;

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
            .map_err(|e| format!("Write error on {}: {}", target.display(), e))?;
        total_written += bytes_read as u64;
    }

    writer
        .flush()
        .map_err(|e| format!("Flush error on {}: {}", target.display(), e))?;

    Ok(total_written)
}

pub fn move_to_trash(path: &Path, trash_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(trash_dir).map_err(|e| format!("Failed to create trash dir: {}", e))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let trash_name = format!("{}_{}", timestamp, file_name);
    let trash_path = trash_dir.join(&trash_name);

    fs::rename(path, &trash_path)
        .or_else(|_| fs::copy(path, &trash_path).and_then(|_| fs::remove_file(path)))
        .map_err(|e| format!("Failed to trash file: {}", e))?;

    Ok(trash_path)
}

pub fn undo_moves(entries: &[UndoEntry]) -> Vec<(String, Result<(), String>)> {
    let mut results = Vec::new();

    for entry in entries.iter().rev() {
        match entry.operation.as_str() {
            "move" | "move_dir" => {
                if let (Some(source), Some(target)) = (&entry.source_path, &entry.target_path) {
                    let result = fs::rename(target, source)
                        .or_else(|_| {
                            fs::copy(target, source).and_then(|_| fs::remove_file(target))
                        })
                        .map_err(|e| format!("Undo failed: {}", e));
                    results.push((entry.id.clone(), result));
                }
            }
            "delete" => {
                if let (Some(source), Some(target)) = (&entry.source_path, &entry.target_path) {
                    let result = fs::rename(target, source)
                        .map_err(|e| format!("Restore from trash failed: {}", e));
                    results.push((entry.id.clone(), result));
                }
            }
            _ => {}
        }
    }

    results
}
