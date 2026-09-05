//! 프론트엔드가 부르는 커맨드.
//!
//! 얇게 유지한다. 로직은 `db`·`scan`·`media`에 있고 여기는 그것을 감싸기만 한다.
//!
//! 썸네일은 **경로만 넘긴다**. 이전 구현은 이미지를 base64로 만들어 IPC로 보냈는데,
//! 6만 장 그리드에서는 문자열 복사 비용이 그대로 렉이 된다. 프론트는 받은 경로를
//! `convertFileSrc`로 바꿔 `<img src>`에 넣으면 된다 — 웹뷰가 파일을 직접 읽는다.

pub mod ai;
pub mod backup;
pub mod capture_date;
pub mod cull;
pub mod db_backup;
pub mod folder;
pub mod geo;
pub mod import;
pub mod job;
pub mod library;
pub mod list;
pub mod marking;
pub mod naming;
pub mod nas;
pub mod organize;
pub mod p1;
pub mod photo_protocol;
pub mod scan;
pub mod settings;
pub mod sidebar;
pub mod smart;
pub mod state;
pub mod tags;
pub mod thumb_protocol;
pub mod transfer;
pub mod trash;
pub mod update;
pub mod video_protocol;
pub mod watch;

pub(crate) use crate::db::conn::Db;
pub(crate) use crate::db::libraries::Library as LibRow;
pub(crate) use crate::db::query::{self, Cursor, Filter, Page};
pub(crate) use crate::db::tree;
use crate::media::cache;
pub(crate) use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
pub(crate) use tauri::{AppHandle, Emitter, Manager, State};

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
    pub watch: Arc<crate::scan::watch::Watchers>,
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
            watch: Arc::new(crate::scan::watch::Watchers::default()),
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

/// 파일 작업이 만든 썸네일 대기 행만 백그라운드에서 채운다.
/// 전체 폴더 스캔이나 원본 hash 재계산은 하지 않는다.
pub(crate) fn start_pending_thumbs(app: &AppHandle, library_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lib = crate::db::libraries::get(&state.db, library_id)
        .map_err(err)?
        .ok_or("등록되지 않은 라이브러리입니다")?;
    let mount = crate::db::volumes::find_mount(&lib.volume_uuid)
        .ok_or("디스크가 연결되어 있지 않습니다")?;
    let cache_root = state.cache_root(library_id);
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    let Some(guard) = job::try_start_with(&state.running, "썸네일", false) else {
        return Err("다른 작업이 시작되어 썸네일 생성을 다음 스캔으로 미룹니다".into());
    };
    cancel.store(false, Ordering::Relaxed);
    let handle = app.clone();
    std::thread::spawn(move || {
        let _guard = guard;
        let result = crate::scan::thumbs::generate(
            &db,
            library_id,
            &mount,
            &cache_root,
            cancel,
            |progress| {
                let _ = handle.emit("thumb-progress", progress);
            },
        );
        let _ = handle.emit("thumb-done", result.is_ok());
    });
    Ok(())
}

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
