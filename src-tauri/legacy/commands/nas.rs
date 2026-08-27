use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::commands::organize::extract_date_folder;
use crate::core::hasher;
use crate::db::{queries, Database};
use crate::nas::{self, DsmClient, NasEntry};

pub struct NasState(pub tokio::sync::Mutex<Option<DsmClient>>);

impl Default for NasState {
    fn default() -> Self {
        Self(tokio::sync::Mutex::new(None))
    }
}

static UPLOAD_CANCELLED: AtomicBool = AtomicBool::new(false);
static UPLOAD_ACTIVE: AtomicBool = AtomicBool::new(false);

// Clears the active flag even on early returns / panics inside nas_upload
struct UploadActiveGuard;

impl Drop for UploadActiveGuard {
    fn drop(&mut self) {
        UPLOAD_ACTIVE.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NasConfig {
    pub url: String,
    pub account: String,
    pub verify_tls: bool,
    pub dest_root: String,
    pub organize_by_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NasStatus {
    pub connected: bool,
    pub account: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NasUploadProgress {
    pub phase: String,
    pub total: usize,
    pub current: usize,
    pub current_file: String,
    pub uploaded: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedUpload {
    pub file_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NasUploadResult {
    pub total: usize,
    pub uploaded: usize,
    pub skipped: usize,
    pub failed: Vec<FailedUpload>,
    pub cancelled: bool,
    /// Files uploaded fine but the local ledger write failed; they may be
    /// re-uploaded next time because skip detection won't see them.
    pub ledger_failures: usize,
}

#[tauri::command]
pub async fn nas_get_config(db: State<'_, Arc<Database>>) -> Result<NasConfig, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            Ok(NasConfig {
                url: queries::get_setting(conn, "nas_url")?.unwrap_or_default(),
                account: queries::get_setting(conn, "nas_account")?.unwrap_or_default(),
                // Secure by default: verification stays on unless explicitly disabled
                verify_tls: queries::get_setting(conn, "nas_verify_tls")?.as_deref() != Some("0"),
                dest_root: queries::get_setting(conn, "nas_dest_root")?.unwrap_or_default(),
                organize_by_date: queries::get_setting(conn, "nas_organize_by_date")?
                    .as_deref()
                    != Some("0"),
            })
        })
        .map_err(|e| format!("DB: {}", e))
}

#[tauri::command]
pub async fn nas_connect(
    db: State<'_, Arc<Database>>,
    state: State<'_, NasState>,
    url: String,
    account: String,
    password: String,
    otp_code: Option<String>,
    verify_tls: bool,
) -> Result<NasStatus, String> {
    let client =
        DsmClient::connect(&url, &account, &password, otp_code.as_deref(), verify_tls).await?;

    let normalized_url = client.base_url().to_string();
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            queries::set_setting(conn, "nas_url", &normalized_url)?;
            queries::set_setting(conn, "nas_account", &account)?;
            queries::set_setting(conn, "nas_verify_tls", if verify_tls { "1" } else { "0" })?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    let status = NasStatus {
        connected: true,
        account: Some(client.account.clone()),
        url: Some(normalized_url),
    };
    *state.0.lock().await = Some(client);
    Ok(status)
}

#[tauri::command]
pub async fn nas_disconnect(state: State<'_, NasState>) -> Result<(), String> {
    if let Some(client) = state.0.lock().await.take() {
        client.logout().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn nas_status(state: State<'_, NasState>) -> Result<NasStatus, String> {
    let guard = state.0.lock().await;
    Ok(match guard.as_ref() {
        Some(client) => NasStatus {
            connected: true,
            account: Some(client.account.clone()),
            url: Some(client.base_url().to_string()),
        },
        None => NasStatus {
            connected: false,
            account: None,
            url: None,
        },
    })
}

async fn client_snapshot(state: &NasState) -> Result<DsmClient, String> {
    state
        .0
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| "NAS에 연결되어 있지 않습니다".to_string())
}

#[tauri::command]
pub async fn nas_list_folders(
    state: State<'_, NasState>,
    path: Option<String>,
) -> Result<Vec<NasEntry>, String> {
    let client = client_snapshot(&state).await?;
    match path.as_deref().filter(|p| !p.is_empty()) {
        Some(folder) => client.list_folders(folder).await,
        None => client.list_shares().await,
    }
}

#[tauri::command]
pub async fn nas_create_folder(
    state: State<'_, NasState>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let client = client_snapshot(&state).await?;
    client.create_folder(&parent, &name).await
}

#[tauri::command]
pub async fn nas_cancel_upload() -> Result<(), String> {
    UPLOAD_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn nas_uploaded_media_ids(db: State<'_, Arc<Database>>) -> Result<Vec<String>, String> {
    let db_ref = db.inner().clone();
    db_ref
        .with_conn(queries::get_uploaded_media_ids)
        .map_err(|e| format!("DB: {}", e))
}

struct UploadItem {
    media_id: String,
    file_path: String,
    file_name: String,
    sha256: Option<String>,
    best_date: String,
}

fn load_upload_items(
    conn: &rusqlite::Connection,
    media_ids: &Option<Vec<String>>,
    exclude_bcuts: bool,
) -> Result<Vec<UploadItem>, rusqlite::Error> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ids) = media_ids {
        let placeholders: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        conditions.push(format!("mf.id IN ({})", placeholders.join(",")));
        for id in ids {
            params.push(Box::new(id.clone()));
        }
    }
    if exclude_bcuts {
        conditions.push(
            "NOT EXISTS (
                SELECT 1 FROM bcut_members bm
                JOIN bcut_groups bg ON bm.group_id = bg.id
                WHERE bm.media_id = mf.id AND bm.is_best = 0 AND bg.status = 'pending'
            )"
            .to_string(),
        );
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let query = format!(
        "SELECT mf.id, mf.file_path, mf.file_name, mf.sha256_hash, mf.modified_at, me.date_taken
         FROM media_files mf
         LEFT JOIN media_exif me ON mf.id = me.media_id
         {}
         ORDER BY COALESCE(me.date_taken, mf.modified_at) ASC",
        where_clause
    );

    let mut stmt = conn.prepare(&query)?;
    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let modified_at: String = row.get(4)?;
            let date_taken: Option<String> = row.get(5)?;
            Ok(UploadItem {
                media_id: row.get(0)?,
                file_path: row.get(1)?,
                file_name: row.get(2)?,
                sha256: row.get(3)?,
                best_date: date_taken.unwrap_or(modified_at),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn file_mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

#[tauri::command]
pub async fn nas_upload(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    state: State<'_, NasState>,
    media_ids: Option<Vec<String>>,
    dest_root: String,
    organize_by_date: bool,
    exclude_bcuts: bool,
    skip_uploaded: bool,
    overwrite: bool,
) -> Result<NasUploadResult, String> {
    let client = client_snapshot(&state).await?;
    let dest_root = dest_root.trim_end_matches('/').to_string();
    if dest_root.is_empty() {
        return Err("업로드 대상 폴더를 선택해주세요".to_string());
    }
    if UPLOAD_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("이미 업로드가 진행 중입니다".to_string());
    }
    let _active = UploadActiveGuard;
    UPLOAD_CANCELLED.store(false, Ordering::SeqCst);

    let db_ref = db.inner().clone();
    db_ref
        .with_conn(|conn| {
            queries::set_setting(conn, "nas_dest_root", &dest_root)?;
            queries::set_setting(
                conn,
                "nas_organize_by_date",
                if organize_by_date { "1" } else { "0" },
            )?;
            Ok(())
        })
        .map_err(|e| format!("DB: {}", e))?;

    let items = db_ref
        .with_conn(|conn| load_upload_items(conn, &media_ids, exclude_bcuts))
        .map_err(|e| format!("DB: {}", e))?;

    let mut uploaded_shas: std::collections::HashSet<String> = db_ref
        .with_conn(queries::get_uploaded_sha256s)
        .map_err(|e| format!("DB: {}", e))?
        .into_iter()
        .collect();
    let uploaded_ids: std::collections::HashSet<String> = db_ref
        .with_conn(queries::get_uploaded_media_ids)
        .map_err(|e| format!("DB: {}", e))?
        .into_iter()
        .collect();

    let total = items.len();
    let mut uploaded = 0usize;
    let mut skipped = 0usize;
    let mut ledger_failures = 0usize;
    let mut failed: Vec<FailedUpload> = Vec::new();
    let mut cancelled = false;

    for (i, item) in items.iter().enumerate() {
        if UPLOAD_CANCELLED.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }

        app.emit(
            "nas-upload-progress",
            NasUploadProgress {
                phase: "uploading".to_string(),
                total,
                current: i + 1,
                current_file: item.file_name.clone(),
                uploaded,
                skipped,
                failed: failed.len(),
            },
        )
        .ok();

        let path = Path::new(&item.file_path);
        if !path.exists() {
            failed.push(FailedUpload {
                file_name: item.file_name.clone(),
                error: "원본 파일을 찾을 수 없습니다".to_string(),
            });
            continue;
        }

        // Ensure we have a content hash for the upload ledger
        let sha256 = match &item.sha256 {
            Some(s) => Some(s.clone()),
            None => {
                let hash_path = item.file_path.clone();
                let computed =
                    tokio::task::spawn_blocking(move || hasher::full_hash(Path::new(&hash_path)))
                        .await
                        .ok()
                        .flatten();
                if let Some(sha) = &computed {
                    let (id, sha) = (item.media_id.clone(), sha.clone());
                    db_ref
                        .with_conn(|conn| queries::update_media_sha256(conn, &id, &sha))
                        .ok();
                }
                computed
            }
        };

        if skip_uploaded {
            let already = uploaded_ids.contains(&item.media_id)
                || sha256
                    .as_ref()
                    .map(|s| uploaded_shas.contains(s))
                    .unwrap_or(false);
            if already {
                skipped += 1;
                continue;
            }
        }

        let dest_dir = if organize_by_date {
            format!("{}/{}", dest_root, extract_date_folder(&item.best_date))
        } else {
            dest_root.clone()
        };

        match client
            .upload_file(
                &dest_dir,
                path,
                &item.file_name,
                file_mtime_ms(path),
                overwrite,
            )
            .await
        {
            Ok(()) => {
                uploaded += 1;
                let remote_path = format!("{}/{}", dest_dir, item.file_name);
                if let Err(e) = db_ref.with_conn(|conn| {
                    queries::insert_nas_upload(
                        conn,
                        &item.media_id,
                        sha256.as_deref(),
                        &item.file_path,
                        &remote_path,
                    )
                }) {
                    log::warn!("nas_uploads 원장 기록 실패 ({}): {}", item.file_name, e);
                    ledger_failures += 1;
                }
                if let Some(sha) = sha256 {
                    uploaded_shas.insert(sha);
                }
            }
            Err(e) => {
                match nas::error_code(&e) {
                    // 1805 = same-name file already on NAS with overwrite off
                    Some(1805) => skipped += 1,
                    // 105/119 = session expired; every remaining file would
                    // fail the same way, so stop early
                    Some(105) | Some(119) => {
                        failed.push(FailedUpload {
                            file_name: item.file_name.clone(),
                            error: format!("{} — 세션이 만료되어 남은 업로드를 중단했습니다", e),
                        });
                        cancelled = true;
                        break;
                    }
                    _ => failed.push(FailedUpload {
                        file_name: item.file_name.clone(),
                        error: e,
                    }),
                }
            }
        }
    }

    app.emit(
        "nas-upload-progress",
        NasUploadProgress {
            phase: if cancelled { "cancelled" } else { "complete" }.to_string(),
            total,
            current: total,
            current_file: String::new(),
            uploaded,
            skipped,
            failed: failed.len(),
        },
    )
    .ok();

    Ok(NasUploadResult {
        total,
        uploaded,
        skipped,
        failed,
        cancelled,
        ledger_failures,
    })
}
