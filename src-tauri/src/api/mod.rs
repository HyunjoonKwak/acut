//! 프론트엔드가 부르는 커맨드.
//!
//! 얇게 유지한다. 로직은 `db`·`scan`·`media`에 있고 여기는 그것을 감싸기만 한다.
//!
//! 썸네일은 **경로만 넘긴다**. 이전 구현은 이미지를 base64로 만들어 IPC로 보냈는데,
//! 6만 장 그리드에서는 문자열 복사 비용이 그대로 렉이 된다. 프론트는 받은 경로를
//! `convertFileSrc`로 바꿔 `<img src>`에 넣으면 된다 — 웹뷰가 파일을 직접 읽는다.

pub mod geo;
pub mod cull;
pub mod backup;
pub mod job;
pub mod nas;
pub mod organize;
pub mod photo_protocol;
pub mod thumb_protocol;
pub mod update;
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
    /// 스캔·가져오기가 도는 중인가. 둘이 겹치면 같은 DB·캐시에 동시에 쓰고
    /// 진행 숫자가 두 줄기로 뒤섞인다.
    pub running: Arc<AtomicBool>,
    /// 폴더 감시 — 파인더로 넣은 사진이 저절로 나타난다
    pub watch: Arc<scan::watch::Watchers>,
    /// 비슷한 사진 색인 — 벡터를 메모리에 한 번 올려 둔 것. 임베딩이 바뀌면 비운다.
    pub ai_index: Mutex<Option<Arc<crate::ai::similar::Index>>>,
    /// 글로 찾기 모델 — 처음 물을 때 올리고 그대로 둔다 (0.5초)
    pub ai_text: Mutex<Option<Arc<crate::ai::text::Text>>>,
    /// 백업 계획 — 「살펴보기」가 두고 「백업 시작」이 가져간다
    pub backup_plans: Mutex<Option<Vec<backup::Planned>>>,
    /// DB가 준비된 순간까지 걸린 시간 — 시작 시간 재기(0단계 성능 목표 «1초»)
    pub db_ready_ms: u64,
    /// 화면이 마지막으로 «살아 있음»을 보낸 시각(유닉스 초). 0이면 아직 한 번도.
    /// 메모리가 모자라면 macOS가 웹뷰 프로세스만 내려 창이 검게 된다 — 이걸로 알아챈다.
    pub last_beat: std::sync::atomic::AtomicI64,
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
            running: Arc::new(AtomicBool::new(false)),
            watch: Arc::new(scan::watch::Watchers::default()),
            ai_index: Mutex::new(None),
            ai_text: Mutex::new(None),
            backup_plans: Mutex::new(None),
            db_ready_ms: crate::started().elapsed().as_millis() as u64,
            last_beat: std::sync::atomic::AtomicI64::new(0),
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
pub async fn libraries_list(state: State<'_, AppState>) -> Result<Vec<LibRow>, String> {
    crate::db::libraries::list(&state.db).map_err(err)
}

/// 폴더를 라이브러리로 등록한다. 스캔은 하지 않는다 (`scan_start`가 한다).
#[tauri::command]
pub async fn library_add(state: State<'_, AppState>, path: String, area: i32) -> Result<LibRow, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let r = crate::db::libraries::add(&state.db, &dir, area);
    state.forget_dirs();
    r
}

/// 등록을 지운다. **원본 사진과 디스크의 캐시 파일은 건드리지 않는다.**
/// 라이브러리의 영역(역할)을 바꾼다 — 0 작업대 · 1 내사진 · 2 공용 · 3 기타
#[tauri::command]
pub async fn library_set_area(state: State<'_, AppState>, id: i64, area: i32) -> Result<(), String> {
    if !(0..=3).contains(&area) {
        return Err(format!("모르는 영역: {area}"));
    }
    crate::db::libraries::set_area(&state.db, id, area).map_err(err)
}

#[tauri::command]
pub async fn library_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let r = crate::db::libraries::remove(&state.db, id).map_err(err);
    state.forget_dirs();
    r
}

// ── 스캔 ───────────────────────────────────────────────────────────────

/// 옛 위치(볼륨 안 `.acut/thumbs`)의 캐시를 앱 폴더로 옮긴다.
///
/// 앱을 켤 때 한 번 부른다. 이미 만들어 둔 12만 장을 버리지 않기 위해서다 —
/// 다시 만들려면 390GB를 또 읽어야 한다.
#[tauri::command]
pub async fn cache_migrate(app: AppHandle) -> Result<(usize, usize), String> {
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
pub async fn scan_start(app: AppHandle, library_id: i64) -> Result<(), String> {
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
    // 이미 도는 중이면 새로 시작하지 않는다 — 두 벌이 같은 캐시에 쓴다.
    // 폴더 감시가 잠깐 쥔 것이면 기다렸다 잡는다 — 조용한 감시 탓에 «이미 스캔 중»이
    // 아무것도 안 보이는데 뜨던 문제 (2026-08-31)
    let Some(guard) = job::try_start_wait(&state.running, "스캔", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 아직 도는 중입니다 — 툴바의 작업 표시가 사라진 뒤 다시 눌러 주세요".into());
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        // 어떻게 끝나든 표시를 내린다
        let _guard = guard;

        let handle = app.clone();
        let r = scan::scan_folder(&db, lib.id, &dir, lib.area, |p| {
            let _ = handle.emit("scan-progress", p);
        });
        match r {
            Ok(p) => {
                // 지명 캐시 적용은 scan_folder 안에서 이미 끝났다 — 이 알림을
                // 받은 화면이 새로 고치면 새 사진에도 이름이 붙어 있다
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
    // 백업(sync.rs)은 제 스위치를 본다
    crate::core::sync::SYNC_CANCELLED.store(true, Ordering::SeqCst);
}

// ── 목록 ───────────────────────────────────────────────────────────────

/// 한 페이지. `cursor`가 없으면 첫 페이지.
#[tauri::command]
pub async fn files_page(
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

/// 월별 분포 — 우측 스크러버용.
#[tauri::command]
pub async fn files_timeline(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<Vec<query::Bucket>, String> {
    query::timeline(&state.db, &filter).map_err(err)
}

#[derive(Debug, Serialize)]
pub struct FileSummary {
    pub files: i64,
    pub bytes: i64,
}

/// 현재 필터에 걸린 파일 수와 용량 — 상태바와 툴바가 같은 대상을 가리키게 한다.
#[tauri::command]
pub async fn files_summary(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<FileSummary, String> {
    let (files, bytes) = query::summary(&state.db, &filter).map_err(err)?;
    Ok(FileSummary { files, bytes })
}

/// 스크롤바 손잡이가 멈춘 자리를 커서로 바꾼다. 그 뒤는 다시 keyset이다.
#[tauri::command]
pub async fn files_cursor_at(
    state: State<'_, AppState>,
    filter: Filter,
    index: i64,
) -> Result<Option<Cursor>, String> {
    query::cursor_at(&state.db, &filter, index).map_err(err)
}

/// 사이드바가 훑어볼 갈래별 장수.
#[tauri::command]
pub async fn files_facets(
    state: State<'_, AppState>,
    filter: Filter,
    kind: query::FacetKind,
) -> Result<Vec<query::Facet>, String> {
    query::facets(&state.db, &filter, kind).map_err(err)
}

// ── 사이드바 ───────────────────────────────────────────────────────────

/// 사이드바 폴더 트리. 라이브러리를 고른 뒤에만 의미가 있다.
///
/// 스캐너는 파일이 든 폴더만 기록하므로 중간 마디는 [`tree::build`]가 만든다.
#[derive(Debug, Serialize)]
pub struct FolderHit {
    /// `folders` 행 — 사진이 바로 아래 없는 중간 폴더는 행이 없어 None
    pub id: Option<i64>,
    pub library_id: i64,
    pub library: String,
    /// 라이브러리 기준 경로
    pub path: String,
    pub volume_uuid: String,
    /// 볼륨 기준 경로 — 견주기·질의의 열쇠
    pub vol_rel: String,
    /// Finder 가 준 절대경로 — 다음 고르기 창의 시작 자리
    pub abs: String,
    /// 이 폴더와 그 아래의 사진 수
    pub file_count: i64,
}

/// Finder 로 고른 절대경로 → 등록된 라이브러리 안의 폴더 행. 밖이면 None.
/// exFAT 은 목록을 NFD 로 주니 NFC 로 맞춰 견준다.
#[tauri::command]
pub async fn folder_by_path(state: State<'_, AppState>, path: String) -> Result<Option<FolderHit>, String> {
    use unicode_normalization::UnicodeNormalization;
    let nfc = |s: &str| s.nfc().collect::<String>();
    let want = nfc(path.trim_end_matches('/'));
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    // 가장 깊이 맞는 라이브러리 — 라이브러리가 겹쳐 등록돼 있을 수 있다
    let mut best: Option<(&crate::db::libraries::Library, String)> = None;
    for l in &libs {
        let Some(dir) = &l.dir else { continue };
        let root = nfc(&dir.to_string_lossy()).trim_end_matches('/').to_string();
        let sub = if want == root {
            Some(String::new())
        } else {
            want.strip_prefix(&format!("{root}/")).map(str::to_string)
        };
        if let Some(sub) = sub {
            if best.as_ref().is_none_or(|(b, _)| b.dir.as_ref().map_or(0, |d| d.as_os_str().len()) < root.len()) {
                best = Some((l, sub));
            }
        }
    }
    let Some((lib, sub)) = best else { return Ok(None) };
    let vol_rel = crate::media::cache::rel_path(&lib.rel_path, &sub);
    let esc = crate::db::query::escape_like(&vol_rel);
    let (id, n): (Option<i64>, i64) = state
        .db
        .read(|c| {
            use rusqlite::OptionalExtension;
            let id = c
                .query_row(
                    "SELECT id FROM folders WHERE volume_uuid = ?1 AND rel_path = ?2",
                    rusqlite::params![lib.volume_uuid, vol_rel],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            let n = c.query_row(
                "SELECT COUNT(*) FROM files f JOIN folders fo ON fo.id = f.folder_id
                  WHERE fo.volume_uuid = ?1 AND (fo.rel_path = ?2 OR fo.rel_path LIKE ?3 || '/%' ESCAPE '\\')
                    AND f.trashed_at IS NULL",
                rusqlite::params![lib.volume_uuid, vol_rel, esc],
                |r| r.get::<_, i64>(0),
            )?;
            Ok((id, n))
        })
        .map_err(err)?;
    Ok(Some(FolderHit {
        id,
        library_id: lib.id,
        library: lib.name.clone(),
        path: sub,
        volume_uuid: lib.volume_uuid.clone(),
        vol_rel,
        abs: path,
        file_count: n,
    }))
}

#[tauri::command]
pub async fn folders_list(
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
pub async fn library_stats(
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
pub async fn cache_usage(
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
pub async fn cache_clear(state: State<'_, AppState>, library_id: Option<i64>) -> Result<(), String> {
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
    clear_thumb_rows(&state.db, library_id).map_err(err)?;
    Ok(())
}

fn clear_thumb_rows(db: &Db, library_id: Option<i64>) -> crate::db::conn::Result<()> {
    db.write(|c| match library_id {
        Some(id) => c.execute(
            "DELETE FROM thumbs WHERE file_id IN (
                SELECT fi.id FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                WHERE fo.library_id = ?1
            )",
            [id],
        ),
        None => c.execute("DELETE FROM thumbs", []),
    })?;
    Ok(())
}

/// Finder에서 그 파일을 골라 연다.
///
/// 우리가 못 하는 일(이름 바꾸기·다른 앱으로 열기)은 Finder에 맡기는 게 낫다.
/// `open -R`은 파일을 **고른 상태로** 폴더를 연다.
#[tauri::command]
pub async fn reveal_in_finder(state: State<'_, AppState>, id: i64) -> Result<(), String> {
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
pub async fn file_detail(state: State<'_, AppState>, id: i64) -> Result<serde_json::Value, String> {
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
pub async fn files_mark(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    rating: Option<i32>,
    culling_flag: Option<i32>,
    favorite: Option<bool>,
) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    // 한 문장 — id 마다 세 문장이면 5,000장에 15,000번 실행이다. NULL 은 «그대로»
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let n = state
        .db
        .transaction(|tx| {
            let n = tx.execute(
                &format!(
                    "UPDATE files SET rating = COALESCE(?1, rating),
                                      culling_flag = COALESCE(?2, culling_flag),
                                      favorite = COALESCE(?3, favorite)
                     WHERE id IN ({list})"
                ),
                rusqlite::params![
                    rating.map(|r| r.clamp(0, 5)),
                    culling_flag.map(|f| f.clamp(0, 2)),
                    favorite.map(|v| v as i32),
                ],
            )?;
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

    /// 재로드 판별과 페이지 기준 계산 — 페이지가 늦게 떠도 음수가 되지 않는다
    #[test]
    fn reload_grid_time_is_measured_from_the_page_not_the_process() {
        assert_eq!(ms_since_page(1_051_545, 1_050_500), 1_045);
        assert_eq!(ms_since_page(100, 200), 0, "시계가 어긋나도 음수 대신 0");
    }

    #[test]
    fn clearing_one_library_keeps_other_thumbnail_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| c.execute_batch(
            "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
             INSERT INTO libraries(id,volume_uuid,rel_path,name) VALUES (1,'V','a','a'), (2,'V','b','b');
             INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area) VALUES
                (1,'V',1,'a','a',1), (2,'V',2,'b','b',1);
             INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at) VALUES
                (1,1,'a.jpg',1,0,1,0,0), (2,2,'b.jpg',1,0,1,0,0);
             INSERT INTO thumbs(file_id,src_size,src_mtime) VALUES (1,1,1), (2,1,1);"
        )).unwrap();

        clear_thumb_rows(&db, Some(1)).unwrap();
        let ids: Vec<i64> = db.read(|c| {
            let mut st = c.prepare("SELECT file_id FROM thumbs ORDER BY file_id")?;
            let ids = st.query_map([], |r| r.get(0))?.collect();
            ids
        }).unwrap();
        assert_eq!(ids, vec![2]);
    }
}

// ── 가져오기 ───────────────────────────────────────────────────────────

/// 가져올 폴더를 훑어 무엇이 몇 장 들어갈지 미리 본다. 복사는 하지 않는다.
#[tauri::command]
pub async fn import_preview(
    state: State<'_, AppState>,
    sources: Vec<String>,
    library_id: i64,
) -> Result<crate::ops::import::Preview, String> {
    let paths = source_paths(&sources)?;
    crate::ops::import::preview(&state.db, &paths, library_id).map_err(err)
}

/// 끌어다 놓은 것들 — 파일·폴더 섞여 온다. 없는 경로는 거절한다.
fn source_paths(sources: &[String]) -> Result<Vec<PathBuf>, String> {
    if sources.is_empty() {
        return Err("가져올 것이 없습니다".into());
    }
    sources
        .iter()
        .map(|s| {
            let p = PathBuf::from(s);
            if p.exists() { Ok(p) } else { Err(format!("없는 경로입니다: {s}")) }
        })
        .collect()
}

/// 실제로 가져온다. 진행 상황은 `import-progress`로 흘린다.
///
/// 복사가 끝나면 **그 날짜 폴더만** 다시 스캔한다. 라이브러리 전체를 훑으면
/// 몇 장 들이는 데 몇 분이 걸린다. 스캐너는 이미 아는 파일을 건너뛰므로
/// 새로 들어온 것만 읽는다.
#[tauri::command]
pub async fn import_run(app: AppHandle, sources: Vec<String>, library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let paths = source_paths(&sources)?;
    let lib = crate::db::libraries::get(&state.db, library_id)
        .map_err(err)?
        .ok_or("등록되지 않은 라이브러리입니다")?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid)
        .ok_or("디스크가 연결되어 있지 않습니다")?;
    let cache_root = state.cache_root(library_id);
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    let Some(guard) = job::try_start_wait(&state.running, "가져오기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤 가져오세요".into());
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let _guard = guard;

        // 스캔이 «새로 들어온 것»을 가려내는 기준. 복사 전에 찍어 둔다.
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let handle = app.clone();
        let r = crate::ops::import::copy_in(&db, &paths, library_id, |p| {
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
pub async fn settings_get(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    crate::db::settings::get(&state.db, &key).map_err(err)
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    crate::db::settings::set(&state.db, &key, &value).map_err(err)
}

#[tauri::command]
pub async fn settings_remove(state: State<'_, AppState>, key: String) -> Result<(), String> {
    crate::db::settings::remove(&state.db, &key).map_err(err)
}

// ── 백업 ───────────────────────────────────────────────────────────────

fn backup_dir(state: &AppState) -> PathBuf {
    state.cache_base.join("backups")
}

/// 지금 쓰는 DB 파일 — 어디에 있고 얼마나 큰가.
#[tauri::command]
pub async fn db_info(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let p = state.db.path().to_path_buf();
    let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    // WAL이 아직 안 합쳐진 만큼도 센다 — 켜 둔 동안은 여기에 쌓인다
    let wal = std::fs::metadata(p.with_extension("db-wal")).map(|m| m.len()).unwrap_or(0);
    Ok(serde_json::json!({ "path": p.to_string_lossy(), "bytes": bytes + wal }))
}

/// DB 사본을 한 벌 만든다. 켜 둔 채로 해도 된다.
#[tauri::command]
pub async fn db_backup(state: State<'_, AppState>) -> Result<crate::db::backup::Backup, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    crate::db::backup::make(&state.db, &backup_dir(&state), now).map_err(err)
}

#[tauri::command]
pub async fn db_backups(state: State<'_, AppState>) -> Result<Vec<crate::db::backup::Backup>, String> {
    crate::db::backup::list(&backup_dir(&state)).map_err(err)
}

/// 사본으로 되돌린다. 먼저 지금 상태를 한 벌 떠 두고, 되돌린 뒤 프론트가
/// 화면을 다시 읽는다 (설정까지 바뀌므로 통째로 새로고침).
#[tauri::command]
pub async fn db_restore(state: State<'_, AppState>, path: String) -> Result<crate::db::backup::Backup, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dir = backup_dir(&state);
    // 백업 폴더 안의 파일만 받는다 — 아무 경로나 부어 넣게 두지 않는다
    let p = PathBuf::from(&path);
    if p.parent() != Some(dir.as_path()) {
        return Err("백업 폴더 안의 사본만 되돌릴 수 있습니다".into());
    }
    let r = crate::db::backup::restore(&state.db, &dir, &p, now).map_err(err)?;
    state.forget_dirs();
    Ok(r)
}

/// 백업 폴더를 Finder에서 연다.
#[tauri::command]
pub async fn db_backups_reveal(state: State<'_, AppState>) -> Result<(), String> {
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
pub async fn open_in_default_app(state: State<'_, AppState>, id: i64) -> Result<(), String> {
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

// ── 코멘트 · 이름 ──────────────────────────────────────────────────────

/// 한 장의 코멘트. 비우면 NULL로.
#[tauri::command]
pub async fn file_comment(state: State<'_, AppState>, id: i64, text: String) -> Result<(), String> {
    let t = crate::scan::nfc(text.trim());
    let v: Option<String> = if t.is_empty() { None } else { Some(t) };
    state
        .db
        .write(|c| c.execute("UPDATE files SET comment = ?2 WHERE id = ?1", rusqlite::params![id, v]))
        .map_err(err)?;
    Ok(())
}

/// 이름을 바꾼다. 같은 이름이 있으면 거절한다. 새 이름을 돌려준다.
#[tauri::command]
pub async fn file_rename(state: State<'_, AppState>, id: i64, name: String) -> Result<String, String> {
    crate::ops::rename::rename(&state.db, id, &name).map_err(err)
}

// ── 폴더 감시 ───────────────────────────────────────────────────────────

/// 켜면 연결된 라이브러리 전부를 감시하고, 끄면 다 멈춘다.
///
/// 라이브러리를 더하거나 뺀 뒤에도 이걸 다시 부른다 — 지금 목록에 맞춘다.
#[tauri::command]
pub async fn watch_set(app: AppHandle, enabled: bool) -> Result<Vec<i64>, String> {
    let state = app.state::<AppState>();
    let w = Arc::clone(&state.watch);
    if !enabled {
        w.stop_all();
        return Ok(Vec::new());
    }
    // 처리 스레드 — 한 번만 뜬다. 달라진 것을 프론트에 알린다.
    {
        let handle = app.clone();
        let busy_handle = app.clone();
        w.run(
            Arc::clone(&state.db),
            state.cache_base.clone(),
            Arc::clone(&state.running),
            Arc::clone(&state.cancel),
            move |c| {
                let _ = handle.emit("library-changed", c);
            },
            move |n: usize| {
                let _ = busy_handle.emit("watch-busy", n);
            },
        );
    }
    let libs = crate::db::libraries::list(&state.db).map_err(err)?;
    let want: Vec<i64> = libs.iter().filter(|l| l.dir.is_some()).map(|l| l.id).collect();
    for id in w.watching() {
        if !want.contains(&id) {
            w.stop(id);
        }
    }
    for l in libs.iter().filter(|l| l.dir.is_some()) {
        if let Some(dir) = l.dir.as_deref() {
            if let Err(e) = w.start(l.id, std::path::Path::new(dir)) {
                log::warn!("감시 시작 실패 {}: {e}", l.name);
            }
        }
    }
    Ok(w.watching())
}

/// 주어진 id들의 행 — 준 순서대로. 목록에 없는 사진 한 줄이 필요할 때.
#[tauri::command]
pub async fn files_by_ids(state: State<'_, AppState>, ids: Vec<i64>) -> Result<Vec<query::FileRow>, String> {
    query::by_ids(&state.db, &ids).map_err(err)
}

// ── AI ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AiStatus {
    /// 모델 파일이 있나
    pub model_present: bool,
    pub model_bytes: u64,
    /// 벡터가 있는 장수 / 전체 장수
    pub embedded: i64,
    pub total: i64,
    /// 긴 일이 도는 중인가 — 화면이 새로 떠도 이걸로 다시 잡는다
    pub running: bool,
    /// 글로 찾기 모델(셋) 있나 / 받을 크기
    pub text_present: bool,
    pub text_bytes: u64,
    /// 얼굴 모델(둘) 있나 / 받을 크기
    pub face_present: bool,
    pub face_bytes: u64,
    /// 얼굴을 찾아 본 사진 / 찾을 수 있는 사진, 얼굴 수, 사람 수
    pub faces_done: i64,
    pub faces_total: i64,
    pub faces: i64,
    pub persons: i64,
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> Result<AiStatus, String> {
    use crate::ai::models::{self, ModelId};
    let (embedded, total) = crate::ai::embed::counts(&state.db).map_err(err)?;
    let (faces_done, faces_total, faces, persons) = crate::ai::people::counts(&state.db).map_err(err)?;
    Ok(AiStatus {
        model_present: models::present(&state.cache_base, ModelId::ClipVision),
        model_bytes: models::spec(ModelId::ClipVision).bytes,
        embedded,
        total,
        running: state.running.load(Ordering::Acquire),
        text_present: models::text_present(&state.cache_base),
        text_bytes: models::text_bytes(),
        face_present: models::face_present(&state.cache_base),
        face_bytes: models::face_bytes(),
        faces_done,
        faces_total,
        faces,
        persons,
    })
}

/// 모델을 받는다 — `which`는 "vision"(사진 벡터) 또는 "text"(글로 찾기, 파일 셋).
/// 진행은 `ai-download`(셋이면 합산), 끝나면 `ai-download-done`(오류 글 또는 null).
#[tauri::command]
pub async fn ai_model_download(app: AppHandle, which: String) -> Result<(), String> {
    use crate::ai::models::{self, DownloadProgress, ModelId};
    let ids: Vec<ModelId> = match which.as_str() {
        "vision" => vec![ModelId::ClipVision],
        "text" => models::TEXT_BUNDLE.to_vec(),
        "face" => models::FACE_BUNDLE.to_vec(),
        _ => return Err(format!("모르는 모델: {which}")),
    };
    let state = app.state::<AppState>();
    let base = state.cache_base.clone();
    std::thread::spawn(move || {
        let handle = app.clone();
        let total: u64 = ids.iter().map(|&id| models::spec(id).bytes).sum();
        let mut before = 0u64;
        let mut r = Ok(());
        for &id in &ids {
            if models::present(&base, id) {
                before += models::spec(id).bytes;
                continue;
            }
            let got = models::download(&base, id, |p| {
                let _ = handle.emit("ai-download", DownloadProgress { id, got: before + p.got, total });
            });
            match got {
                Ok(_) => before += models::spec(id).bytes,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        // 텍스트 모델을 새로 받았으면 올려 둔 옛것은 버린다
        if let Ok(mut t) = app.state::<AppState>().ai_text.lock() {
            *t = None;
        }
        let _ = app.emit("ai-download-done", r.err().map(|e| e.to_string()));
    });
    Ok(())
}

/// 벡터를 채운다. 스캔과 같은 running 스위치를 쓴다 — 같은 DB에 둘이 쓰지 않는다.
/// 진행은 `ai-progress`, 끝나면 `ai-done`.
#[tauri::command]
pub async fn ai_embed_start(app: AppHandle) -> Result<(), String> {
    use crate::ai::models::{self, ModelId};
    let state = app.state::<AppState>();
    if !models::present(&state.cache_base, ModelId::ClipVision) {
        return Err("모델이 없습니다 — 설정 › AI에서 받으세요".into());
    }
    let Some(guard) = job::try_start_wait(&state.running, "AI 벡터", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let base = state.cache_base.clone();
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    let model = models::path(&base, ModelId::ClipVision);

    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ai::embed::run(&db, &model, &base, cancel, |p| {
            let _ = handle.emit("ai-progress", p);
        });
        // 색인은 낡았다 — 다음 물음 때 다시 올린다
        if let Ok(mut i) = app.state::<AppState>().ai_index.lock() {
            *i = None;
        }
        match r {
            Ok(p) => {
                let _ = app.emit("ai-done", p);
            }
            Err(e) => {
                let _ = app.emit("ai-error", e.to_string());
            }
        }
    });
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SimilarRow {
    pub file: query::FileRow,
    /// 닮은 정도 0–1 (코사인)
    pub score: f32,
}

/// 벡터 색인 — 처음 물을 때 올리고, 벡터를 새로 만들면 버린다
fn ai_index(state: &AppState) -> Result<Arc<crate::ai::similar::Index>, String> {
    let mut slot = state.ai_index.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(Arc::new(crate::ai::similar::Index::load(&state.db).map_err(err)?));
    }
    let index = Arc::clone(slot.as_ref().unwrap());
    if index.is_empty() {
        return Err("아직 벡터가 없습니다 — 설정 › AI에서 만드세요".into());
    }
    Ok(index)
}

/// 점수 붙은 줄들 — 가까운 순
fn similar_rows(state: &AppState, hits: Vec<(i64, f32)>) -> Result<Vec<SimilarRow>, String> {
    let ids: Vec<i64> = hits.iter().map(|h| h.0).collect();
    let rows = query::by_ids(&state.db, &ids).map_err(err)?;
    let score: HashMap<i64, f32> = hits.into_iter().collect();
    let mut out: Vec<SimilarRow> = rows
        .into_iter()
        .map(|file| SimilarRow { score: score.get(&file.id).copied().unwrap_or(0.0), file })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(out)
}

/// 이 사진과 비슷한 것들 — 가까운 순.
#[tauri::command]
pub async fn ai_similar(state: State<'_, AppState>, id: i64, limit: usize) -> Result<Vec<SimilarRow>, String> {
    let index = ai_index(&state)?;
    similar_rows(&state, index.similar(id, limit.clamp(1, 200)))
}

/// 폴더 한 갈래의 크기 — 옮기기 전에 보여 준다
#[tauri::command]
pub async fn folder_size(state: State<'_, AppState>, folder_id: i64) -> Result<crate::ops::offload::FolderSize, String> {
    crate::ops::offload::folder_size(&state.db, folder_id).map_err(err)
}

/// 폴더 한 갈래를 다른 라이브러리(디스크)로. 진행은 `offload-progress`, 끝나면 `offload-done`.
#[tauri::command]
pub async fn folder_offload(app: AppHandle, folder_id: i64, dest_library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "옮기기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let base = state.cache_base.clone();
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ops::offload::move_folder(&db, &base, folder_id, dest_library_id, &cancel, |p| {
            let _ = handle.emit("offload-progress", p);
        });
        app.state::<AppState>().forget_dirs();
        match r {
            Ok(o) => {
                let _ = app.emit("offload-done", &o);
            }
            Err(e) => {
                let _ = app.emit("offload-error", e.to_string());
            }
        }
    });
    Ok(())
}

/// 폴더 합치기 — `src_rel` 나무를 같은 라이브러리의 `dst_rel` 안으로. 진행 `merge-progress`, 끝 `merge-done`.
#[tauri::command]
pub async fn folder_merge(app: AppHandle, library_id: i64, src_rel: String, dst_rel: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "폴더 합치기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ops::merge::merge_tree(&db, library_id, &src_rel, &dst_rel, &cancel, |p| {
            let _ = handle.emit("merge-progress", p);
        });
        app.state::<AppState>().forget_dirs();
        match r {
            Ok(o) => {
                let _ = app.emit("merge-done", &o);
            }
            Err(e) => {
                let _ = app.emit("merge-error", e.to_string());
            }
        }
    });
    Ok(())
}

/// 사진 없는 폴더(껍데기) 목록
#[tauri::command]
pub async fn husk_list(state: State<'_, AppState>, library_id: i64) -> Result<Vec<crate::ops::husk::Husk>, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || crate::ops::husk::list(&db, library_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// 고른 껍데기 폴더들을 라이브러리 휴지통(_폴더)으로. (옮긴 수, 첫 실패)
#[tauri::command]
pub async fn husk_trash(app: AppHandle, library_id: i64, rels: Vec<String>) -> Result<(usize, Option<String>), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "폴더 정리", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let r = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        crate::ops::husk::to_trash(&db, library_id, &rels)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    app.state::<AppState>().forget_dirs();
    Ok(r)
}

/// 합치고 남은 것(사진 아닌 파일) 세기
#[tauri::command]
pub async fn folder_leftovers(state: State<'_, AppState>, library_id: i64, rel: String) -> Result<crate::ops::merge::Leftovers, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || crate::ops::merge::leftovers(&db, library_id, &rel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// 남은 파일도 같은 자리로 옮기고 빈 폴더를 지운다
#[tauri::command]
pub async fn folder_merge_rest(app: AppHandle, library_id: i64, src_rel: String, dst_rel: String) -> Result<crate::ops::trash::Outcome, String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "폴더 합치기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let r = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        crate::ops::merge::merge_rest(&db, library_id, &src_rel, &dst_rel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    app.state::<AppState>().forget_dirs();
    Ok(r)
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct StartupInfo {
    /// 프로세스 시작 → DB 준비
    pub db_ms: u64,
    /// 프로세스 시작 → 첫 그리드가 그려짐
    pub first_grid_ms: u64,
    pub at: i64,
    /// 웹뷰 기준 표식(ms) — script·hydrated·migrated·libs·grid
    #[serde(default)]
    pub marks: serde_json::Value,
    /// run() 기준 — 웹뷰가 페이지를 읽기 시작한·끝낸 시각
    #[serde(default)]
    pub page_started_ms: u64,
    #[serde(default)]
    pub page_finished_ms: u64,
    /// run() 기준 네이티브 구간 — setup 진입·이전·DB·상태·setup 끝
    #[serde(default)]
    pub native: serde_json::Value,
}

/// 화면이 5초마다 부른다 — 살아 있다는 신호. 뒷단의 감시 스레드가 20초 넘게
/// 못 받으면 웹뷰를 다시 불러온다 (lib.rs watchdog).
#[tauri::command]
pub async fn heartbeat(state: State<'_, AppState>) -> Result<(), String> {
    state.last_beat.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    Ok(())
}

/// 화면 쪽 오류를 파일에 남긴다 — 릴리스 앱은 웹뷰 콘솔을 볼 수 없어서, 그리다
/// 죽은 이유를 이 파일로만 알 수 있다. `~/Library/Logs/com.acut.media/webview.log`
#[tauri::command]
pub async fn frontend_log(app: AppHandle, level: String, msg: String) -> Result<(), String> {
    use std::io::Write;
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("webview.log");
    // 상한 — 오류가 반복되는 세션에서 로그가 끝없이 자라지 않게. 4MB 를 넘으면 한 벌 물린다
    if std::fs::metadata(&path).map(|m| m.len() > 4 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("webview.log.1"));
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let msg: String = msg.chars().take(8_000).collect();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(f, "{stamp} [{level}] {msg}").map_err(|e| e.to_string())
}

/// 이 프로세스에서 이미 한 번 보고했나 — 그 뒤의 보고는 감시견의 웹뷰 재로드다.
/// 재로드를 프로세스 시작 기준으로 재면 «첫 그리드 1,051,545ms» 같은 값이 남는다 (2026-08-31)
static STARTUP_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 페이지(웹뷰 세션) 기준 경과 — 프로세스 기준 now 에서 페이지 시작 시각을 뺀다
pub(crate) fn ms_since_page(now_ms: u64, page_started_ms: u64) -> u64 {
    now_ms.saturating_sub(page_started_ms)
}

/// 첫 그리드가 그려진 순간 화면이 부른다 — 시작 시간을 재서 설정에 남긴다.
/// 성능 목표 «앱 시작 1초»를 눈대중이 아니라 숫자로 본다.
/// 최초 시작만 `startup.last` 에 남고, 웹뷰 재로드는 `startup.reload_last` 로 따로 남는다.
#[tauri::command]
pub async fn startup_report(state: State<'_, AppState>, marks: serde_json::Value) -> Result<StartupInfo, String> {
    let now_ms = crate::started().elapsed().as_millis() as u64;
    let page_started = crate::PAGE_MS[0].load(Ordering::Relaxed);
    let reload = STARTUP_REPORTED.swap(true, Ordering::AcqRel);
    let info = StartupInfo {
        db_ms: state.db_ready_ms,
        // 재로드면 프로세스 기준이 아니라 이 웹뷰 세션 기준으로 잰다
        first_grid_ms: if reload { ms_since_page(now_ms, page_started) } else { now_ms },
        at: chrono::Utc::now().timestamp(),
        marks,
        page_started_ms: page_started,
        page_finished_ms: crate::PAGE_MS[1].load(Ordering::Relaxed),
        native: serde_json::Value::Object(
            crate::NATIVE_LABELS
                .iter()
                .zip(crate::NATIVE_MS.iter())
                .map(|(k, v)| ((*k).to_string(), v.load(Ordering::Relaxed).into()))
                .collect(),
        ),
    };
    if reload {
        log::info!("웹뷰 다시 불러옴 — 페이지 기준 그리드 {}ms · {}", info.first_grid_ms, info.marks);
        crate::db::settings::set(&state.db, "startup.reload_last", &serde_json::to_string(&info).unwrap()).map_err(err)?;
    } else {
        log::info!("시작 — DB {}ms · 첫 화면 {}ms · 네이티브 {} · 웹뷰 {}", info.db_ms, info.first_grid_ms, info.native, info.marks);
        crate::db::settings::set(&state.db, "startup.last", &serde_json::to_string(&info).unwrap()).map_err(err)?;
    }
    Ok(info)
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoDatesDone {
    pub checked: usize,
    pub fixed: usize,
}

/// 영상의 촬영 시각을 컨테이너에서 다시 읽어 고친다. 진행은 `video-dates-progress`,
/// 끝나면 `video-dates-done`. Spotlight가 복사한 날을 돌려주던 것을 바로잡는 용도.
#[tauri::command]
pub fn video_dates_refresh(app: AppHandle, library_id: Option<i64>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(&state.running, "영상 촬영일", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let rows: Vec<(i64, String, String, i64)> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT fi.id, fo.volume_uuid, fo.rel_path || '/' || fi.name, fi.taken_at
                       FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fi.kind = 1 AND fi.trashed_at IS NULL AND (?1 IS NULL OR fo.library_id = ?1)",
                )?;
                let it = st.query_map([library_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();
        let mounts: HashMap<String, Option<PathBuf>> = rows
            .iter()
            .map(|r| r.1.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .map(|u| (u.clone(), crate::db::volumes::find_mount(&u)))
            .collect();
        let total = rows.len();
        let (mut checked, mut fixed) = (0usize, 0usize);
        let mut last = std::time::Instant::now();
        for (id, vol, rel, taken_at) in rows {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            checked += 1;
            let Some(Some(mount)) = mounts.get(&vol) else { continue };
            let rel = rel.trim_start_matches('/').to_string();
            let path = mount.join(&rel);
            let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
            // 단서(컨테이너·파일명·폴더명·파일 시각) 가운데 가장 이른 그럴듯한 것
            let embedded = crate::media::mp4date::creation_time(&path);
            let md = std::fs::metadata(&path).ok();
            let unix = |t: Option<std::time::SystemTime>| t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            let now = chrono::Utc::now().timestamp();
            let folder = rel.trim_end_matches(&name).trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
            let (t, s) = crate::media::taken_at::resolve_video(
                embedded,
                &name,
                &folder,
                md.as_ref().and_then(|m| unix(m.modified().ok())),
                md.as_ref().and_then(|m| unix(m.created().ok())),
                now,
            );
            let src = s as i32;
            if t != taken_at {
                let _ = db.write(|c| {
                    c.execute("UPDATE files SET taken_at = ?2, taken_at_source = ?3 WHERE id = ?1", rusqlite::params![id, t, src])
                });
                fixed += 1;
            }
            if last.elapsed().as_millis() >= 200 {
                last = std::time::Instant::now();
                let _ = handle.emit("video-dates-progress", serde_json::json!({ "done": checked, "total": total }));
            }
        }
        let _ = app.emit("video-dates-done", VideoDatesDone { checked, fixed });
    });
    Ok(())
}

/// 지도 전체 조건의 장수와 경계 — 마커 제한과 무관한 자동 맞춤 기준.
#[tauri::command]
pub async fn map_overview(state: State<'_, AppState>, filter: Filter) -> Result<query::MapOverview, String> {
    query::map_overview(&state.db, &filter).map_err(err)
}

/// 지도의 칸들 — 현재 보이는 영역만 `precision`도 격자로 묶는다.
/// filter의 저장된 bbox 대신 viewport를 써야 영역을 고른 뒤에도 밖으로 이동할 수 있다.
#[tauri::command]
pub async fn map_cells(
    state: State<'_, AppState>,
    mut filter: Filter,
    precision: f64,
    viewport: Option<String>,
) -> Result<Vec<query::MapCell>, String> {
    filter.bbox = viewport;
    query::map_cells(&state.db, &filter, precision).map_err(err)
}

/// 얼굴을 찾고 사람으로 묶는다. 진행은 `faces-progress`, 끝나면 `faces-done`.
#[tauri::command]
pub async fn ai_faces_start(app: AppHandle) -> Result<(), String> {
    use crate::ai::models;
    let state = app.state::<AppState>();
    if !models::face_present(&state.cache_base) {
        return Err("얼굴 모델이 없습니다 — 설정 › AI에서 받으세요".into());
    }
    let Some(guard) = job::try_start_wait(&state.running, "얼굴 찾기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let base = state.cache_base.clone();
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ai::people::run(&db, &base, &base, cancel, |p| {
            let _ = handle.emit("faces-progress", p);
        })
        .and_then(|p| crate::ai::people::cluster(&db).map(|c| (p, c)));
        match r {
            Ok((p, c)) => {
                let _ = app.emit("faces-done", FacesDone { done: p.done, faces: p.faces, persons: c.persons });
            }
            Err(e) => {
                let _ = app.emit("ai-error", e.to_string());
            }
        }
    });
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct FacesDone {
    pub done: usize,
    pub faces: usize,
    pub persons: usize,
}

#[derive(Debug, Serialize)]
pub struct PersonRow {
    pub id: i64,
    pub name: Option<String>,
    pub count: i64,
    /// 대표 얼굴 — 썸네일 주소(라이브러리/상대경로)와 그 안의 상자(비율)
    pub cover_thumb: Option<String>,
    pub cover_bbox: Option<serde_json::Value>,
}

/// 사람 목록 — 얼굴 많은 순. 대표 얼굴은 가장 크게 찍힌 것.
#[tauri::command]
pub async fn people_list(state: State<'_, AppState>) -> Result<Vec<PersonRow>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT p.id, p.name, COUNT(f.id),
                        (SELECT fo.library_id || '/' || t.rel_path || '|' || f2.bbox
                           FROM faces f2
                           JOIN files fi ON fi.id = f2.file_id
                           JOIN folders fo ON fo.id = fi.folder_id
                           JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
                          WHERE f2.person_id = p.id AND fi.trashed_at IS NULL
                          ORDER BY json_extract(f2.bbox, '$.w') DESC LIMIT 1)
                   FROM persons p
                   LEFT JOIN faces f ON f.person_id = p.id
                  GROUP BY p.id
                  ORDER BY COUNT(f.id) DESC, p.id",
            )?;
            let it = st.query_map([], |r| {
                let cover: Option<String> = r.get(3)?;
                let (thumb, bbox) = match cover.and_then(|c| c.split_once('|').map(|(a, b)| (a.to_string(), b.to_string()))) {
                    Some((t, b)) => (Some(t), serde_json::from_str(&b).ok()),
                    None => (None, None),
                };
                Ok(PersonRow { id: r.get(0)?, name: r.get(1)?, count: r.get(2)?, cover_thumb: thumb, cover_bbox: bbox })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

#[tauri::command]
pub async fn person_rename(state: State<'_, AppState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    state
        .db
        .transaction(|tx| {
            tx.execute(
                "UPDATE persons SET name = ?2 WHERE id = ?1",
                rusqlite::params![id, if name.is_empty() { None } else { Some(name) }],
            )?;
            Ok(())
        })
        .map_err(err)
}

/// `from`의 얼굴을 전부 `into`로 옮기고 `from`은 지운다 — 같은 사람이 둘로 갈렸을 때
#[tauri::command]
pub async fn person_merge(state: State<'_, AppState>, into: i64, from: i64) -> Result<(), String> {
    if into == from {
        return Ok(());
    }
    state
        .db
        .transaction(|tx| {
            tx.execute("UPDATE faces SET person_id = ?1 WHERE person_id = ?2", [into, from])?;
            tx.execute("DELETE FROM persons WHERE id = ?1", [from])?;
            Ok(())
        })
        .map_err(err)
}

/// 글로 찾기 — «바닷가에서 뛰는 강아지» 같은 글에 가까운 사진들.
#[tauri::command]
pub async fn ai_text_search(state: State<'_, AppState>, query: String, limit: usize) -> Result<Vec<SimilarRow>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let text = {
        let mut slot = state.ai_text.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(Arc::new(crate::ai::text::Text::load(&state.cache_base).map_err(err)?));
        }
        Arc::clone(slot.as_ref().unwrap())
    };
    let v = text.embed(q).map_err(err)?;
    let index = ai_index(&state)?;
    similar_rows(&state, index.similar_to(&v, limit.clamp(1, 200), None))
}
