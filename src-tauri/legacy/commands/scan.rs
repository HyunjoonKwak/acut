use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::{metadata, scanner};
use crate::db::queries::{self, MediaExif, MediaFile};
use crate::db::Database;

static SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub total: usize,
    pub current: usize,
    pub current_file: String,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub total_files: usize,
    pub new_files: usize,
    pub skipped: usize,
    pub cancelled: bool,
}

#[tauri::command]
pub async fn cancel_scan() -> Result<(), String> {
    SCAN_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

// Phase 0: Fast file list
#[tauri::command]
pub async fn scan_directory(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    config: State<'_, Arc<tokio::sync::RwLock<crate::core::config::AppConfig>>>,
    path: String,
) -> Result<ScanResult, String> {
    SCAN_CANCELLED.store(false, Ordering::SeqCst);

    let dir = Path::new(&path);
    if !dir.exists() || !dir.is_dir() {
        return Err("Invalid directory path".to_string());
    }

    app.emit("scan-progress", ScanProgress {
        total: 0, current: 0,
        current_file: "파일 탐색 중...".into(),
        phase: "scanning".into(),
    }).ok();

    let scan_cfg = config.read().await.scan.clone();
    let files = scanner::scan_directory(dir, &scan_cfg);
    let total = files.len();
    let mut new_files = 0usize;
    let mut skipped = 0usize;
    let mut cancelled = false;

    for (chunk_idx, chunk) in files.chunks(500).enumerate() {
        if SCAN_CANCELLED.load(Ordering::SeqCst) { cancelled = true; break; }

        let db_ref = db.inner().clone();
        db_ref.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for scanned in chunk {
                let path_str = scanned.path.to_string_lossy().to_string();
                if queries::is_file_unchanged(&tx, &path_str, &scanned.modified_at, scanned.file_size as i64)? {
                    skipped += 1;
                    continue;
                }
                let media = MediaFile {
                    id: uuid::Uuid::new_v4().to_string(),
                    file_path: path_str.clone(), original_path: path_str,
                    file_name: scanned.file_name.clone(),
                    file_size: scanned.file_size as i64,
                    mime_type: scanner::get_mime_type(&scanned.path),
                    sha256_hash: None, quick_hash: None, width: None, height: None,
                    media_type: scanned.media_type.as_str().into(),
                    created_at: scanned.created_at.clone(),
                    modified_at: scanned.modified_at.clone(),
                    scanned_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                    source_type: "local".into(), thumbnail: None, scan_phase: 0, date_taken: None,
                };
                queries::insert_media_phase0(&tx, &media)?;
                new_files += 1;
            }
            tx.commit()
        }).map_err(|e| format!("DB: {}", e))?;

        let done = ((chunk_idx + 1) * 500).min(total);
        app.emit("scan-progress", ScanProgress {
            total, current: done,
            current_file: chunk.last().map(|f| f.file_name.clone()).unwrap_or_default(),
            phase: "scanning".into(),
        }).ok();
    }

    app.emit("scan-progress", ScanProgress {
        total, current: total, current_file: String::new(),
        phase: if cancelled { "cancelled" } else { "phase0_complete" }.into(),
    }).ok();

    Ok(ScanResult { total_files: total, new_files, skipped, cancelled })
}

// Thumbnails live as JPEG files here instead of base64 blobs in SQLite:
// keeps media_files rows small so list queries stay fast at scale, and
// lets the webview load them through the asset protocol.
pub fn thumbs_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("thumbs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// Phase 1: Process ONE batch (50 files) and return immediately
// Frontend calls this in a loop with setInterval
// This way get_media_list can run between batches
#[tauri::command]
pub async fn process_phase1(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
) -> Result<(usize, usize, usize), String> {
    let db_ref = db.inner().clone();
    let thumb_dir = thumbs_dir(&app);

    let batch: Vec<(String, String, String)> = db_ref
        .with_conn(|conn| queries::get_phase0_files(conn, 50))
        .map_err(|e| format!("DB: {}", e))?;

    if batch.is_empty() {
        let (done, total): (i64, i64) = db_ref.with_conn(|conn| {
            let d = conn.query_row("SELECT COUNT(*) FROM media_files WHERE scan_phase > 0", [], |r| r.get(0)).unwrap_or(0);
            let t = conn.query_row("SELECT COUNT(*) FROM media_files", [], |r| r.get(0)).unwrap_or(0);
            Ok((d, t))
        }).unwrap_or((0, 0));
        return Ok((0, done as usize, total as usize));
    }

    // Process in parallel with rayon: EXIF + dimensions + thumbnail
    let results: Vec<_> = batch
        .par_iter()
        .filter_map(|(id, file_path, media_type)| {
            let path = Path::new(file_path);
            let thumb_path = thumb_dir.join(format!("{}.jpg", id));
            let mut width = None;
            let mut height = None;
            let mut exif_out = None;
            let mut thumbnail = None;

            if path.exists() && media_type == "image" {
                let exif = metadata::extract_exif(path);
                width = exif.width;
                height = exif.height;
                if width.is_none() {
                    if let Ok(dims) = image::image_dimensions(path) {
                        width = Some(dims.0 as i32);
                        height = Some(dims.1 as i32);
                    }
                }
                exif_out = Some(MediaExif {
                    media_id: id.clone(),
                    date_taken: exif.date_taken, camera_make: exif.camera_make,
                    camera_model: exif.camera_model, gps_latitude: exif.gps_latitude,
                    gps_longitude: exif.gps_longitude, orientation: exif.orientation,
                });
                thumbnail = make_thumbnail_fast(path, &thumb_path);
            } else if path.exists() && media_type == "video" {
                thumbnail = make_video_thumbnail(path, &thumb_path);
            }

            Some((id.clone(), width, height, exif_out, thumbnail))
        })
        .collect();

    let count = results.len();

    db_ref.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        for (id, w, h, exif, thumb) in &results {
            queries::update_media_phase1(&tx, id, *w, *h, thumb.as_deref())?;
            if let Some(e) = exif {
                queries::insert_media_exif(&tx, e)?;
            }
        }
        tx.commit()
    }).map_err(|e| format!("DB: {}", e))?;

    let (done, total_files): (i64, i64) = db_ref.with_conn(|conn| {
        let done = conn.query_row("SELECT COUNT(*) FROM media_files WHERE scan_phase > 0", [], |r| r.get(0)).unwrap_or(0);
        let total = conn.query_row("SELECT COUNT(*) FROM media_files", [], |r| r.get(0)).unwrap_or(0);
        Ok((done, total))
    }).unwrap_or((0, 0));

    Ok((count, done as usize, total_files as usize))
}

fn is_valid_thumb(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

// Write a 256px JPEG thumbnail to `out`; returns the file path on success.
// sips first (macOS hardware accelerated), pure-Rust `image` as fallback so
// the pipeline also works without sips (e.g. non-macOS builds).
fn make_thumbnail_fast(path: &Path, out: &Path) -> Option<String> {
    let sips_ok = std::process::Command::new("sips")
        .args(["--resampleHeightWidthMax", "256", "-s", "format", "jpeg",
               "-s", "formatOptions", "80", path.to_str()?, "--out", out.to_str()?])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if sips_ok && is_valid_thumb(out) {
        return Some(out.to_string_lossy().to_string());
    }
    let _ = std::fs::remove_file(out);

    // Fallback: pure Rust (JPEG has no alpha, so flatten to RGB first)
    let img = image::open(path).ok()?;
    let thumb = image::DynamicImage::ImageRgb8(img.thumbnail(256, 256).to_rgb8());
    let file = std::fs::File::create(out).ok()?;
    let mut writer = std::io::BufWriter::new(file);
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 80);
    if thumb.write_with_encoder(enc).is_err() {
        let _ = std::fs::remove_file(out);
        return None;
    }
    Some(out.to_string_lossy().to_string())
}

// Find ffmpeg binary (Tauri app may not have /opt/homebrew/bin in PATH)
pub fn find_ffmpeg() -> &'static str {
    use std::sync::OnceLock;
    static FFMPEG: OnceLock<String> = OnceLock::new();
    FFMPEG.get_or_init(|| {
        let candidates = [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ];
        for c in &candidates {
            if Path::new(c).exists() {
                return c.to_string();
            }
        }
        "ffmpeg".to_string()
    })
}

// Video thumbnail: capture frame at 1 second using ffmpeg, written to `out`
fn make_video_thumbnail(path: &Path, out: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();

    let output = std::process::Command::new(find_ffmpeg())
        .args([
            "-ss", "1",
            "-i", &path_str,
            "-frames:v", "1",
            "-vf", "scale=256:-1",
            "-q:v", "5",
            "-update", "1",
            "-y",
        ])
        .arg(out.to_string_lossy().as_ref())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if (output.status.success() || out.exists()) && is_valid_thumb(out) {
        return Some(out.to_string_lossy().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    log::warn!("ffmpeg failed for {}: {}", path_str, stderr.lines().last().unwrap_or("unknown"));
    let _ = std::fs::remove_file(out);
    None
}

// On-demand thumbnails for duplicate comparison
#[tauri::command]
pub async fn generate_thumbnails_for(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    file_ids: Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    let db_ref = db.inner().clone();
    let thumb_dir = thumbs_dir(&app);

    let files: Vec<(String, String, String)> = db_ref.with_conn(|conn| {
        let placeholders: Vec<String> = file_ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1)).collect();
        let query = format!(
            "SELECT id, file_path, media_type FROM media_files WHERE id IN ({}) AND thumbnail IS NULL",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&query)?;
        let params: Vec<&dyn rusqlite::ToSql> = file_ids.iter()
            .map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }).map_err(|e| format!("DB: {}", e))?;

    let mut results = Vec::new();
    for (id, file_path, media_type) in &files {
        let path = Path::new(file_path);
        let thumb_path = thumb_dir.join(format!("{}.jpg", id));
        let thumb = if media_type == "video" {
            make_video_thumbnail(path, &thumb_path)
        } else {
            make_thumbnail_fast(path, &thumb_path)
        };
        if let Some(thumb) = thumb {
            db_ref.with_conn(|conn| {
                conn.execute("UPDATE media_files SET thumbnail = ?1 WHERE id = ?2",
                    rusqlite::params![thumb, id])
            }).ok();
            results.push((id.clone(), thumb));
        }
    }

    Ok(results)
}
