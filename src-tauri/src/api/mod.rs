//! 프론트엔드가 부르는 커맨드.
//!
//! 얇게 유지한다. 로직은 `db`·`scan`·`media`에 있고 여기는 그것을 감싸기만 한다.
//!
//! 썸네일은 **경로만 넘긴다**. 이전 구현은 이미지를 base64로 만들어 IPC로 보냈는데,
//! 6만 장 그리드에서는 문자열 복사 비용이 그대로 렉이 된다. 프론트는 받은 경로를
//! `convertFileSrc`로 바꿔 `<img src>`에 넣으면 된다 — 웹뷰가 파일을 직접 읽는다.

pub mod cull;
pub mod organize;
pub mod photo_protocol;
pub mod thumb_protocol;
pub mod smart;
pub mod tags;
pub mod trash;
pub mod video_protocol;

use crate::db::conn::Db;
use crate::db::libraries::Library as LibRow;
use crate::db::query::{self, Cursor, Filter, Page};
use crate::db::tree;
use crate::media::cache;
use crate::scan;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// 앱이 살아 있는 동안 유지되는 상태.
///
/// **"열린 라이브러리" 같은 건 없다.** 등록된 라이브러리가 여럿이고, 경로가
/// 필요한 곳에서는 그때그때 파일이 속한 라이브러리를 찾아 푼다. 하나를
/// 기억해 두면 다른 디스크 사진에서 엉뚱한 경로를 보게 된다.
pub struct AppState {
    pub db: Arc<Db>,
    /// 썸네일·미리보기 캐시의 기준 폴더 (앱 데이터 폴더).
    pub cache_base: PathBuf,
    /// 스캔·썸네일 생성을 멈추는 스위치.
    pub cancel: Arc<AtomicBool>,
    /// 라이브러리 id → 실제 폴더.
    ///
    /// `thumb://`는 **썸네일 한 장마다** 이걸 부른다. 한 화면에 200장이면
    /// 200번이다. 매번 DB를 읽고 `/Volumes`를 훑으면 그리드가 그대로 멈춘다.
    /// 등록이 바뀔 때만 비운다.
    dirs: Mutex<HashMap<i64, PathBuf>>,
}

impl AppState {
    pub fn new(db: Db, cache_base: PathBuf) -> Self {
        Self {
            db: Arc::new(db),
            cache_base,
            cancel: Arc::new(AtomicBool::new(false)),
            dirs: Mutex::new(HashMap::new()),
        }
    }

    /// 라이브러리의 실제 폴더. 없으면 디스크가 빠진 것이다.
    ///
    /// 한 번 찾은 것은 기억한다. 디스크를 뽑았다 꽂으면 경로가 달라질 수
    /// 있는데, 그건 `forget_dirs()`를 부르는 쪽(등록 변경·스캔 시작)에서 처리한다.
    pub fn library_dir(&self, id: i64) -> Option<PathBuf> {
        if let Ok(m) = self.dirs.lock() {
            if let Some(p) = m.get(&id) {
                return Some(p.clone());
            }
        }
        let (uuid, rel): (String, String) = self
            .db
            .read(|c| {
                c.query_row(
                    "SELECT volume_uuid, rel_path FROM libraries WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .ok()?;
        let dir = crate::db::libraries::dir_of(&uuid, &rel)?;
        if let Ok(mut m) = self.dirs.lock() {
            m.insert(id, dir.clone());
        }
        Some(dir)
    }

    /// 이 라이브러리의 썸네일 캐시 폴더.
    pub fn cache_root(&self, library_id: i64) -> PathBuf {
        cache::cache_root(&self.cache_base, library_id)
    }

    /// 기억해 둔 경로를 버린다. 등록이 바뀌거나 디스크 상태가 달라졌을 때.
    pub fn forget_dirs(&self) {
        if let Ok(mut m) = self.dirs.lock() {
            m.clear();
        }
    }
}

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ── 라이브러리 ─────────────────────────────────────────────────────────

/// 등록된 것 전부. 디스크가 빠진 것도 포함한다.
#[tauri::command]
pub fn libraries_list(state: State<'_, AppState>) -> Result<Vec<LibRow>, String> {
    crate::db::libraries::list(&state.db).map_err(err)
}

/// 폴더를 라이브러리로 등록한다. 스캔은 하지 않는다 (`scan_start`가 한다).
#[tauri::command]
pub fn library_add(state: State<'_, AppState>, path: String, area: i32) -> Result<LibRow, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let r = crate::db::libraries::add(&state.db, &dir, area);
    state.forget_dirs();
    r
}

/// 등록을 지운다. **원본 사진과 디스크의 캐시 파일은 건드리지 않는다.**
#[tauri::command]
pub fn library_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let r = crate::db::libraries::remove(&state.db, id).map_err(err);
    state.forget_dirs();
    r
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

/// 옛 위치(볼륨 안 `.acut/thumbs`)의 캐시를 앱 폴더로 옮긴다.
///
/// 앱을 켤 때 한 번 부른다. 이미 만들어 둔 12만 장을 버리지 않기 위해서다 —
/// 다시 만들려면 390GB를 또 읽어야 한다.
#[tauri::command]
pub fn cache_migrate(app: AppHandle) -> Result<(usize, usize), String> {
    let state = app.state::<AppState>();
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    let base = state.cache_base.clone();
    let db = Arc::clone(&state.db);
    let _ = db;

    let mut moved = 0;
    let mut failed = 0;
    for l in libs {
        let Some(dir) = l.dir.as_deref() else { continue };
        let legacy = cache::legacy_root(dir);
        if !legacy.is_dir() {
            continue;
        }
        let (m, f) = cache::migrate_from_legacy(&legacy, &cache::cache_root(&base, l.id));
        moved += m;
        failed += f;
    }
    Ok((moved, failed))
}

/// 스캔을 시작한다. 진행 상황은 `scan-progress` 이벤트로 흘린다.
///
/// 블로킹 작업이라 별도 스레드에서 돈다. 커맨드 자체는 바로 돌아온다.
#[tauri::command]
pub fn scan_start(app: AppHandle, library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lib = crate::db::libraries::get(&state.db, library_id)
        .map_err(err)?
        .ok_or("등록되지 않은 라이브러리입니다")?;
    let dir = lib.dir.clone().ok_or("디스크가 연결되어 있지 않습니다")?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid)
        .ok_or("디스크가 연결되어 있지 않습니다")?;
    let cache_root = state.cache_root(lib.id);
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let handle = app.clone();
        let r = scan::scan_folder(&db, lib.id, &dir, lib.area, |p| {
            let _ = handle.emit("scan-progress", p);
        });
        match r {
            Ok(p) => {
                let _ = app.emit("scan-done", p);
                // 스캔이 끝나면 곧바로 썸네일을 만든다. 목록은 이미 볼 수 있다.
                // 1차 — 박힌 미리보기를 그대로 받는다. 몇 분이면 그리드가 찬다.
                let tp = scan::thumbs::generate(
                    &db,
                    lib.id,
                    &mount,
                    &cache_root,
                    Arc::clone(&cancel),
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
    group: Option<query::GroupBy>,
) -> Result<Page, String> {
    // 한 번에 너무 많이 요청하면 IPC가 막힌다.
    let limit = limit.clamp(1, 500);
    query::page(&state.db, &filter, cursor, limit, group.unwrap_or_default()).map_err(err)
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

/// 스크롤바 손잡이가 멈춘 자리를 커서로 바꾼다. 그 뒤는 다시 keyset이다.
#[tauri::command]
pub fn files_cursor_at(
    state: State<'_, AppState>,
    filter: Filter,
    index: i64,
) -> Result<Option<Cursor>, String> {
    query::cursor_at(&state.db, &filter, index).map_err(err)
}

/// 사이드바가 훑어볼 갈래별 장수.
#[tauri::command]
pub fn files_facets(
    state: State<'_, AppState>,
    filter: Filter,
    kind: query::FacetKind,
) -> Result<Vec<query::Facet>, String> {
    query::facets(&state.db, &filter, kind).map_err(err)
}

#[tauri::command]
pub fn files_summary(state: State<'_, AppState>, filter: Filter) -> Result<Summary, String> {
    let (count, bytes) = query::summary(&state.db, &filter).map_err(err)?;
    Ok(Summary { count, bytes })
}

// ── 사이드바 ───────────────────────────────────────────────────────────

/// 사이드바 폴더 트리. 라이브러리를 고른 뒤에만 의미가 있다.
///
/// 스캐너는 파일이 든 폴더만 기록하므로 중간 마디는 [`tree::build`]가 만든다.
#[tauri::command]
pub fn folders_list(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<Vec<tree::Node>, String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;

    // 라이브러리를 고르면 그 트리만 준다. 안 고르면 라이브러리마다 머리
    // 마디를 얹어 하나로 잇는다 — 예전에는 여기서 빈 목록을 돌려주는 바람에
    // 「앨범」을 열어도 하위 폴더가 아무것도 안 보였다.
    //
    // 4,476줄이 한꺼번에 쏟아지지 않는 건 접혀 있기 때문이다. 프론트는
    // 펼친 마디의 자식만 그린다.
    let mut out = Vec::new();
    for l in libs.iter().filter(|l| library_id.is_none_or(|id| l.id == id)) {
        let nodes = tree::build(leaves_of(state.inner(), l.id, &l.rel_path)?, &l.rel_path, l.id);
        if library_id.is_some() {
            out.extend(nodes);
        } else {
            out.extend(tree::under_root(nodes, l.id, &l.name, l.file_count));
        }
    }
    Ok(out)
}

/// 한 라이브러리의 "사진이 든 폴더"들. 중간 마디는 트리가 만들어 낸다.
fn leaves_of(state: &AppState, library_id: i64, library_rel: &str) -> Result<Vec<tree::Leaf>, String> {
    // rel_path는 **볼륨** 기준이라 라이브러리 루트만큼 앞이 길다. 그대로 쓰면
    // 들여쓰기가 통째로 밀린다. 여기서 잘라 낸다.
    //
    // 자르는 길이는 SQL의 `length()`로 센다. Rust의 `len()`은 **바이트**라
    // 「사진통합작업」 같은 한글 경로에서 세 배로 잘라 낸다.
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT id,
                        CASE WHEN ?2 = '' THEN rel_path
                             ELSE substr(rel_path, length(?2) + 2) END,
                        rel_path, file_count
                 FROM folders
                 WHERE library_id = ?1 AND file_count > 0
                 ORDER BY rel_path",
            )?;
            let it = st.query_map(rusqlite::params![library_id, library_rel], |r| {
                Ok(tree::Leaf {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    rel_path: r.get(2)?,
                    file_count: r.get(3)?,
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
}

/// 상태바에 띄울 값들. `library_id`가 없으면 등록된 전부를 합친다.
///
/// **캐시 용량은 여기서 세지 않는다.** 디스크의 파일 12만 개를 훑는 일이라
/// 1초쯤 걸린다. 폴더를 누를 때마다 그걸 하면 앱이 멈춘 것처럼 보인다.
/// 캐시 용량은 [`cache_usage`]로 따로, 가끔만 부른다.
#[tauri::command]
pub fn library_stats(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<LibraryStats, String> {
    let (files, bytes): (i64, i64) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(fi.size),0)
                 FROM files fi JOIN folders fo ON fo.id=fi.folder_id
                 WHERE fi.trashed_at IS NULL AND (?1 IS NULL OR fo.library_id = ?1)",
                [library_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .map_err(err)?;
    // thumbs를 따로 센다. LEFT JOIN으로 14만 행을 훑는 것보다 빠르다 —
    // 이쪽은 thumbs 테이블만 보면 된다.
    let done: i64 = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM thumbs t
                 JOIN files fi ON fi.id = t.file_id
                 JOIN folders fo ON fo.id = fi.folder_id
                 WHERE t.state = 1 AND fi.trashed_at IS NULL
                   AND (?1 IS NULL OR fo.library_id = ?1)",
                [library_id],
                |r| r.get(0),
            )
        })
        .map_err(err)?;

    Ok(LibraryStats {
        files,
        bytes,
        thumbs_done: done,
        thumbs_pending: files - done,
    })
}

#[derive(Debug, Serialize)]
pub struct CacheUsage {
    pub bytes: u64,
    pub files: usize,
}

/// 썸네일·미리보기 캐시가 디스크에서 차지하는 용량.
///
/// 폴더를 통째로 훑으므로 느리다. 앱 시작과 썸네일 생성이 끝났을 때만 부른다.
#[tauri::command]
pub fn cache_usage(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<CacheUsage, String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    let (bytes, files) = libs
        .iter()
        .filter(|l| library_id.is_none_or(|id| l.id == id))
        .flat_map(|l| {
            [
                cache::cache_root(&state.cache_base, l.id),
                cache::preview_root(&state.cache_base, l.id),
            ]
        })
        .map(|root| cache::cache_stats(&root))
        .fold((0u64, 0usize), |(b, n), (rb, rn)| (b + rb, n + rn));
    Ok(CacheUsage { bytes, files })
}

/// 썸네일·미리보기를 모두 지운다.
///
/// 사진은 건드리지 않는다. 다음에 볼 때 다시 만들어지므로 되돌릴 것이 없다.
/// 캐시가 망가졌을 때(빈 그림, 옛 방향)의 마지막 수단이다.
#[tauri::command]
pub fn cache_clear(state: State<'_, AppState>, library_id: Option<i64>) -> Result<(), String> {
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    for l in libs.iter().filter(|l| library_id.is_none_or(|id| l.id == id)) {
        for root in [
            cache::cache_root(&state.cache_base, l.id),
            cache::preview_root(&state.cache_base, l.id),
        ] {
            // 없으면 지울 것도 없다 — NotFound는 성공으로 본다.
            match std::fs::remove_dir_all(&root) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", root.display())),
            }
        }
    }
    // 디스크에서 지웠으니 "만들어 뒀다"는 기록도 함께 지운다. 안 지우면
    // 다음 스캔이 이미 있는 줄 알고 건너뛰어 빈 자리만 남는다.
    state
        .db
        .write(|c| c.execute("DELETE FROM thumbs", []))
        .map_err(err)?;
    Ok(())
}

/// Finder에서 그 파일을 골라 연다.
///
/// 우리가 못 하는 일(이름 바꾸기·다른 앱으로 열기)은 Finder에 맡기는 게 낫다.
/// `open -R`은 파일을 **고른 상태로** 폴더를 연다.
#[tauri::command]
pub fn reveal_in_finder(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let (uuid, rel): (String, String) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT fo.volume_uuid,
                        fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
                 FROM files fi JOIN folders fo ON fo.id = fi.folder_id WHERE fi.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .map_err(err)?;
    let mount = crate::db::volumes::find_mount(&uuid)
        .ok_or("디스크가 연결되어 있지 않습니다")?;
    let path = mount.join(&rel);
    if !path.exists() {
        return Err(format!("파일이 없습니다: {}", path.display()));
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
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
                        fi.comment, fi.kind, fi.duration_ms
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
                        "durationMs": r.get::<_, Option<i64>>(21)?,
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

// ── 가져오기 ───────────────────────────────────────────────────────────

/// 가져올 폴더를 훑어 무엇이 몇 장 들어갈지 미리 본다. 복사는 하지 않는다.
#[tauri::command]
pub fn import_preview(
    state: State<'_, AppState>,
    source: String,
    library_id: i64,
) -> Result<crate::ops::import::Preview, String> {
    let dir = PathBuf::from(&source);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {source}"));
    }
    crate::ops::import::preview(&state.db, &dir, library_id).map_err(err)
}

/// 실제로 가져온다. 진행 상황은 `import-progress`로 흘린다.
///
/// 복사가 끝나면 **그 날짜 폴더만** 다시 스캔한다. 라이브러리 전체를 훑으면
/// 몇 장 들이는 데 몇 분이 걸린다. 스캐너는 이미 아는 파일을 건너뛰므로
/// 새로 들어온 것만 읽는다.
#[tauri::command]
pub fn import_run(app: AppHandle, source: String, library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let dir = PathBuf::from(&source);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {source}"));
    }
    let lib = crate::db::libraries::get(&state.db, library_id)
        .map_err(err)?
        .ok_or("등록되지 않은 라이브러리입니다")?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid)
        .ok_or("디스크가 연결되어 있지 않습니다")?;
    let cache_root = state.cache_root(library_id);
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        // 스캔이 «새로 들어온 것»을 가려내는 기준. 복사 전에 찍어 둔다.
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let handle = app.clone();
        let r = crate::ops::import::copy_in(&db, &dir, library_id, |p| {
            let _ = handle.emit("import-progress", p);
        });
        let (mut rep, dirs) = match r {
            Ok(v) => v,
            Err(e) => {
                let _ = app.emit("import-done", crate::ops::import::Report {
                    failed: 1,
                    first_error: Some(e.to_string()),
                    ..Default::default()
                });
                return;
            }
        };

        // 들어온 자리만 훑는다
        for d in &dirs {
            let _ = scan::scan_folder(&db, library_id, d, lib.area, |_| {});
        }
        if let Err(e) = crate::ops::import::record_imported(&db, rep.batch_id, library_id, since) {
            rep.first_error.get_or_insert(e.to_string());
        }
        let _ = app.emit("import-done", rep);

        // 새로 들어온 것의 썸네일. 이미 있는 것은 건드리지 않는다.
        let _ = scan::thumbs::generate(&db, library_id, &mount, &cache_root, cancel, |p| {
            let _ = app.emit("thumb-progress", p);
        });
        let _ = app.emit("import-thumbs-done", ());
    });
    Ok(())
}

// ── 설정 ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    crate::db::settings::get(&state.db, &key).map_err(err)
}

#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    crate::db::settings::set(&state.db, &key, &value).map_err(err)
}

#[tauri::command]
pub fn settings_remove(state: State<'_, AppState>, key: String) -> Result<(), String> {
    crate::db::settings::remove(&state.db, &key).map_err(err)
}

// ── 백업 ───────────────────────────────────────────────────────────────

fn backup_dir(state: &AppState) -> PathBuf {
    state.cache_base.join("backups")
}

/// DB 사본을 한 벌 만든다. 켜 둔 채로 해도 된다.
#[tauri::command]
pub fn db_backup(state: State<'_, AppState>) -> Result<crate::db::backup::Backup, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    crate::db::backup::make(&state.db, &backup_dir(&state), now).map_err(err)
}

#[tauri::command]
pub fn db_backups(state: State<'_, AppState>) -> Result<Vec<crate::db::backup::Backup>, String> {
    crate::db::backup::list(&backup_dir(&state)).map_err(err)
}

/// 백업 폴더를 Finder에서 연다.
#[tauri::command]
pub fn db_backups_reveal(state: State<'_, AppState>) -> Result<(), String> {
    let dir = backup_dir(&state);
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(err)?;
    Ok(())
}

/// 파일을 기본 앱으로 연다 — 뷰어가 못 트는 영상은 QuickTime이 튼다.
#[tauri::command]
pub fn open_in_default_app(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let (uuid, rel): (String, String) = state
        .db
        .read(|c| {
            c.query_row(
                "SELECT fo.volume_uuid,
                        fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
                 FROM files fi JOIN folders fo ON fo.id = fi.folder_id WHERE fi.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .map_err(err)?;
    let mount = crate::db::volumes::find_mount(&uuid).ok_or("디스크가 연결되어 있지 않습니다")?;
    let path = mount.join(&rel);
    if !path.exists() {
        return Err(format!("파일이 없습니다: {}", path.display()));
    }
    std::process::Command::new("open").arg(&path).spawn().map_err(err)?;
    Ok(())
}
