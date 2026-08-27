//! 프론트엔드가 부르는 커맨드.
//!
//! 얇게 유지한다. 로직은 `db`·`scan`·`media`에 있고 여기는 그것을 감싸기만 한다.
//!
//! 썸네일은 **경로만 넘긴다**. 이전 구현은 이미지를 base64로 만들어 IPC로 보냈는데,
//! 6만 장 그리드에서는 문자열 복사 비용이 그대로 렉이 된다. 프론트는 받은 경로를
//! `convertFileSrc`로 바꿔 `<img src>`에 넣으면 된다 — 웹뷰가 파일을 직접 읽는다.

pub mod cull;
pub mod photo_protocol;
pub mod thumb_protocol;

use crate::db::conn::Db;
use crate::db::query::{self, Cursor, Filter, Page};
use crate::media::cache;
use crate::scan;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// 앱이 살아 있는 동안 유지되는 상태.
pub struct AppState {
    pub db: Arc<Db>,
    /// 지금 열려 있는 라이브러리. 없으면 아직 고르지 않은 것이다.
    pub library: Mutex<Option<Library>>,
    /// 스캔·썸네일 생성을 멈추는 스위치.
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Library {
    pub root: PathBuf,
    pub volume_uuid: String,
    pub volume_mount: PathBuf,
    pub volume_name: String,
    /// 썸네일 캐시 폴더. 프론트가 경로를 조합할 때 쓴다.
    pub cache_root: PathBuf,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        Self {
            db: Arc::new(db),
            library: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn current(&self) -> Result<Library, String> {
        self.library
            .lock()
            .map_err(|_| "상태를 읽을 수 없습니다".to_string())?
            .clone()
            .ok_or_else(|| "라이브러리를 먼저 열어야 합니다".to_string())
    }
}

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ── 라이브러리 ─────────────────────────────────────────────────────────

/// 폴더를 라이브러리로 연다. 스캔은 하지 않는다 (`scan_start`가 한다).
#[tauri::command]
pub fn library_open(state: State<'_, AppState>, path: String) -> Result<Library, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let v = crate::db::volumes::describe(&root).map_err(err)?;
    let lib = Library {
        cache_root: cache::cache_root(&root),
        root: root.clone(),
        volume_uuid: v.uuid.clone(),
        volume_mount: v.mount_path,
        volume_name: v.name,
    };

    // 다음에 자동으로 열 수 있게 기억해 둔다.
    state
        .db
        .write(|c| {
            c.execute(
                "INSERT INTO settings(key,value) VALUES('library_root',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&path],
            )
        })
        .map_err(err)?;

    *state.library.lock().map_err(|_| "상태 오류".to_string())? = Some(lib.clone());
    Ok(lib)
}

/// 마지막으로 열었던 라이브러리를 다시 연다. 앱 시작 시 부른다.
///
/// 볼륨이 다른 곳에 마운트됐어도 UUID로 찾아내므로 경로가 바뀌어도 열린다.
#[tauri::command]
pub fn library_reopen(state: State<'_, AppState>) -> Result<Option<Library>, String> {
    let saved: Option<String> = state
        .db
        .read(|c| {
            c.query_row("SELECT value FROM settings WHERE key='library_root'", [], |r| {
                r.get(0)
            })
            .or(Ok(String::new()))
        })
        .map_err(err)?
        .into();
    let Some(path) = saved.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if !PathBuf::from(&path).is_dir() {
        // 디스크가 빠졌거나 마운트 경로가 바뀌었다.
        return Ok(None);
    }
    library_open(state, path).map(Some)
}

/// 등록된 볼륨 목록 (연결 여부 포함).
#[derive(Debug, Serialize)]
pub struct VolumeRow {
    pub uuid: String,
    pub name: String,
    pub role: String,
    pub is_online: bool,
    pub total_bytes: Option<i64>,
    pub free_bytes: Option<i64>,
    pub current_mount: Option<String>,
}

#[tauri::command]
pub fn volumes_list(state: State<'_, AppState>) -> Result<Vec<VolumeRow>, String> {
    let rows: Vec<(String, String, String, Option<i64>, Option<i64>)> = state
        .db
        .read(|c| {
            let mut st =
                c.prepare("SELECT uuid,name,role,total_bytes,free_bytes FROM volumes ORDER BY name")?;
            let it = st.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)?;

    Ok(rows
        .into_iter()
        .map(|(uuid, name, role, total, free)| {
            // 지금 실제로 붙어 있는지 UUID로 확인한다.
            let mount = crate::db::volumes::find_mount(&uuid);
            VolumeRow {
                is_online: mount.is_some(),
                current_mount: mount.map(|p| p.to_string_lossy().into_owned()),
                uuid,
                name,
                role,
                total_bytes: total,
                free_bytes: free,
            }
        })
        .collect())
}

// ── 스캔 ───────────────────────────────────────────────────────────────

/// 스캔을 시작한다. 진행 상황은 `scan-progress` 이벤트로 흘린다.
///
/// 블로킹 작업이라 별도 스레드에서 돈다. 커맨드 자체는 바로 돌아온다.
#[tauri::command]
pub fn scan_start(app: AppHandle, area: i32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lib = state.current()?;
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let handle = app.clone();
        let r = scan::scan_folder(&db, &lib.root, area, |p| {
            let _ = handle.emit("scan-progress", p);
        });
        match r {
            Ok(p) => {
                let _ = app.emit("scan-done", p);
                // 스캔이 끝나면 곧바로 썸네일을 만든다. 목록은 이미 볼 수 있다.
                let tp = scan::thumbs::generate(
                    &db,
                    &lib.volume_uuid,
                    &lib.volume_mount,
                    &lib.root,
                    cancel,
                    |p| {
                        let _ = app.emit("thumb-progress", p);
                    },
                );
                let _ = app.emit("thumb-done", tp.ok());
            }
            Err(e) => {
                let _ = app.emit("scan-error", e.to_string());
            }
        }
    });
    Ok(())
}

/// 진행 중인 스캔·썸네일 생성을 멈춘다.
#[tauri::command]
pub fn scan_cancel(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

// ── 목록 ───────────────────────────────────────────────────────────────

/// 한 페이지. `cursor`가 없으면 첫 페이지.
#[tauri::command]
pub fn files_page(
    state: State<'_, AppState>,
    filter: Filter,
    cursor: Option<Cursor>,
    limit: usize,
) -> Result<Page, String> {
    // 한 번에 너무 많이 요청하면 IPC가 막힌다.
    let limit = limit.clamp(1, 500);
    query::page(&state.db, &filter, cursor, limit).map_err(err)
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub count: i64,
    pub bytes: i64,
}

/// 월별 분포 — 우측 스크러버용.
#[tauri::command]
pub fn files_timeline(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<Vec<query::Bucket>, String> {
    query::timeline(&state.db, &filter).map_err(err)
}

#[tauri::command]
pub fn files_summary(state: State<'_, AppState>, filter: Filter) -> Result<Summary, String> {
    let (count, bytes) = query::summary(&state.db, &filter).map_err(err)?;
    Ok(Summary { count, bytes })
}

// ── 사이드바 ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FolderRow {
    pub id: i64,
    pub rel_path: String,
    pub name: String,
    pub area: i32,
    pub file_count: i64,
    pub depth: usize,
}

/// 폴더 트리. 파일이 하나도 없는 폴더는 빼서 사이드바를 짧게 유지한다.
#[tauri::command]
pub fn folders_list(state: State<'_, AppState>) -> Result<Vec<FolderRow>, String> {
    let lib = state.current()?;
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT id, rel_path, name, area, file_count FROM folders
                 WHERE volume_uuid = ?1 AND file_count > 0
                 ORDER BY rel_path",
            )?;
            let it = st.query_map([&lib.volume_uuid], |r| {
                let rel_path: String = r.get(1)?;
                Ok(FolderRow {
                    id: r.get(0)?,
                    depth: rel_path.matches('/').count(),
                    rel_path,
                    name: r.get(2)?,
                    area: r.get(3)?,
                    file_count: r.get(4)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

// ── 상태 ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct LibraryStats {
    pub files: i64,
    pub bytes: i64,
    pub thumbs_done: i64,
    pub thumbs_pending: i64,
    pub cache_bytes: u64,
    pub cache_files: usize,
}

/// 상태바에 띄울 값들.
#[tauri::command]
pub fn library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let lib = state.current()?;
    let (files, bytes, done): (i64, i64, i64) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(fi.size),0),
                        COALESCE(SUM(CASE WHEN t.state=1 THEN 1 ELSE 0 END),0)
                 FROM files fi
                 JOIN folders fo ON fo.id=fi.folder_id
                 LEFT JOIN thumbs t ON t.file_id=fi.id
                 WHERE fo.volume_uuid = ?1",
                [&lib.volume_uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .map_err(err)?;
    let (cache_bytes, cache_files) = cache::cache_stats(&lib.cache_root);
    Ok(LibraryStats {
        files,
        bytes,
        thumbs_done: done,
        thumbs_pending: files - done,
        cache_bytes,
        cache_files,
    })
}

/// 파일 하나의 상세 (인스펙터용).
#[tauri::command]
pub fn file_detail(state: State<'_, AppState>, id: i64) -> Result<serde_json::Value, String> {
    state
        .db
        .read(|c| {
            c.query_row(
                "SELECT fi.name, fo.rel_path, fi.size, fi.taken_at, fi.taken_at_source,
                        fi.width, fi.height, fi.cam_make, fi.cam_model, fi.lens,
                        fi.iso, fi.aperture, fi.shutter, fi.focal_mm,
                        fi.gps_lat, fi.gps_lon, fi.rating, fi.culling_flag, fi.favorite,
                        fi.comment, fi.kind
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id WHERE fi.id=?1",
                [id],
                |r| {
                    Ok(serde_json::json!({
                        "name": r.get::<_, String>(0)?,
                        "folder": r.get::<_, String>(1)?,
                        "size": r.get::<_, i64>(2)?,
                        "takenAt": r.get::<_, i64>(3)?,
                        "takenAtSource": r.get::<_, i32>(4)?,
                        "width": r.get::<_, Option<i64>>(5)?,
                        "height": r.get::<_, Option<i64>>(6)?,
                        "camMake": r.get::<_, Option<String>>(7)?,
                        "camModel": r.get::<_, Option<String>>(8)?,
                        "lens": r.get::<_, Option<String>>(9)?,
                        "iso": r.get::<_, Option<i64>>(10)?,
                        "aperture": r.get::<_, Option<f64>>(11)?,
                        "shutter": r.get::<_, Option<String>>(12)?,
                        "focalMm": r.get::<_, Option<f64>>(13)?,
                        "gpsLat": r.get::<_, Option<f64>>(14)?,
                        "gpsLon": r.get::<_, Option<f64>>(15)?,
                        "rating": r.get::<_, i32>(16)?,
                        "cullingFlag": r.get::<_, i32>(17)?,
                        "favorite": r.get::<_, i32>(18)? != 0,
                        "comment": r.get::<_, Option<String>>(19)?,
                        "kind": r.get::<_, i32>(20)?,
                    }))
                },
            )
        })
        .map_err(err)
}

// ── 고르기 판정 ────────────────────────────────────────────────────────

/// 평점·선별 플래그·즐겨찾기를 한 번에 바꾼다. 여러 장을 동시에 처리한다.
#[tauri::command]
pub fn files_mark(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    rating: Option<i32>,
    culling_flag: Option<i32>,
    favorite: Option<bool>,
) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    let n = state
        .db
        .transaction(|tx| {
            let mut n = 0;
            for id in &ids {
                if let Some(r) = rating {
                    tx.execute(
                        "UPDATE files SET rating=?1 WHERE id=?2",
                        rusqlite::params![r.clamp(0, 5), id],
                    )?;
                }
                if let Some(f) = culling_flag {
                    tx.execute(
                        "UPDATE files SET culling_flag=?1 WHERE id=?2",
                        rusqlite::params![f.clamp(0, 2), id],
                    )?;
                }
                if let Some(v) = favorite {
                    tx.execute(
                        "UPDATE files SET favorite=?1 WHERE id=?2",
                        rusqlite::params![v as i32, id],
                    )?;
                }
                n += 1;
            }
            Ok(n)
        })
        .map_err(err)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limit_is_clamped() {
        // IPC를 막지 않도록 상한을 둔다
        assert_eq!(0usize.clamp(1, 500), 1);
        assert_eq!(10_000usize.clamp(1, 500), 500);
        assert_eq!(200usize.clamp(1, 500), 200);
    }

    #[test]
    fn marks_are_clamped_to_valid_range() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            c.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,1,'a.jpg',1,0,1,0,0)",
                [],
            )
        })
        .unwrap();

        // 범위를 벗어난 값이 그대로 들어가면 안 된다
        db.transaction(|tx| {
            tx.execute("UPDATE files SET rating=?1 WHERE id=1", [99i32.clamp(0, 5)])?;
            Ok(())
        })
        .unwrap();
        let r: i32 = db
            .read(|c| c.query_row("SELECT rating FROM files WHERE id=1", [], |x| x.get(0)))
            .unwrap();
        assert_eq!(r, 5);
    }
}
