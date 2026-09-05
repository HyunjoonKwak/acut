use super::*;

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
    let (faces_done, faces_total, faces, persons) =
        crate::ai::people::counts(&state.db).map_err(err)?;
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
                let _ = handle.emit(
                    "ai-download",
                    DownloadProgress {
                        id,
                        got: before + p.got,
                        total,
                    },
                );
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
    let Some(guard) = job::try_start_wait(
        &state.running,
        "AI 벡터",
        std::time::Duration::from_secs(20),
    ) else {
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
        *slot = Some(Arc::new(
            crate::ai::similar::Index::load(&state.db).map_err(err)?,
        ));
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
        .map(|file| SimilarRow {
            score: score.get(&file.id).copied().unwrap_or(0.0),
            file,
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(out)
}

/// 이 사진과 비슷한 것들 — 가까운 순.
#[tauri::command]
pub async fn ai_similar(
    state: State<'_, AppState>,
    id: i64,
    limit: usize,
) -> Result<Vec<SimilarRow>, String> {
    let index = ai_index(&state)?;
    similar_rows(&state, index.similar(id, limit.clamp(1, 200)))
}

/// 폴더 한 갈래의 크기 — 옮기기 전에 보여 준다
#[tauri::command]
pub async fn folder_size(
    state: State<'_, AppState>,
    folder_id: i64,
) -> Result<crate::ops::offload::FolderSize, String> {
    crate::ops::offload::folder_size(&state.db, folder_id).map_err(err)
}

/// 폴더 한 갈래를 다른 라이브러리(디스크)로. 진행은 `offload-progress`, 끝나면 `offload-done`.
#[tauri::command]
pub async fn folder_offload(
    app: AppHandle,
    folder_id: i64,
    dest_library_id: i64,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) =
        job::try_start_wait(&state.running, "옮기기", std::time::Duration::from_secs(20))
    else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let db = Arc::clone(&state.db);
    let base = state.cache_base.clone();
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let r = crate::ops::offload::move_folder(
            &db,
            &base,
            folder_id,
            dest_library_id,
            &cancel,
            |p| {
                let _ = handle.emit("offload-progress", p);
            },
        );
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
pub async fn folder_merge(
    app: AppHandle,
    library_id: i64,
    src_rel: String,
    dst_rel: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(
        &state.running,
        "폴더 합치기",
        std::time::Duration::from_secs(20),
    ) else {
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
pub async fn husk_list(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<crate::ops::husk::Husk>, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || crate::ops::husk::list(&db, library_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// 고른 껍데기 폴더들을 라이브러리 휴지통(_폴더)으로. (옮긴 수, 첫 실패)
#[tauri::command]
pub async fn husk_trash(
    app: AppHandle,
    library_id: i64,
    rels: Vec<String>,
) -> Result<(usize, Option<String>), String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(
        &state.running,
        "폴더 정리",
        std::time::Duration::from_secs(20),
    ) else {
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
pub async fn folder_leftovers(
    state: State<'_, AppState>,
    library_id: i64,
    rel: String,
) -> Result<crate::ops::merge::Leftovers, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        crate::ops::merge::leftovers(&db, library_id, &rel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

/// 남은 파일도 같은 자리로 옮기고 빈 폴더를 지운다
#[tauri::command]
pub async fn folder_merge_rest(
    app: AppHandle,
    library_id: i64,
    src_rel: String,
    dst_rel: String,
) -> Result<crate::ops::trash::Outcome, String> {
    let state = app.state::<AppState>();
    let Some(guard) = job::try_start_wait(
        &state.running,
        "폴더 합치기",
        std::time::Duration::from_secs(20),
    ) else {
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
    state
        .last_beat
        .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
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
    if std::fs::metadata(&path)
        .map(|m| m.len() > 4 * 1024 * 1024)
        .unwrap_or(false)
    {
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
pub async fn startup_report(
    state: State<'_, AppState>,
    marks: serde_json::Value,
) -> Result<StartupInfo, String> {
    let now_ms = crate::started().elapsed().as_millis() as u64;
    let page_started = crate::PAGE_MS[0].load(Ordering::Relaxed);
    let reload = STARTUP_REPORTED.swap(true, Ordering::AcqRel);
    let info = StartupInfo {
        db_ms: state.db_ready_ms,
        // 재로드면 프로세스 기준이 아니라 이 웹뷰 세션 기준으로 잰다
        first_grid_ms: if reload {
            ms_since_page(now_ms, page_started)
        } else {
            now_ms
        },
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
        log::info!(
            "웹뷰 다시 불러옴 — 페이지 기준 그리드 {}ms · {}",
            info.first_grid_ms,
            info.marks
        );
        crate::db::settings::set(
            &state.db,
            "startup.reload_last",
            &serde_json::to_string(&info).unwrap(),
        )
        .map_err(err)?;
    } else {
        log::info!(
            "시작 — DB {}ms · 첫 화면 {}ms · 네이티브 {} · 웹뷰 {}",
            info.db_ms,
            info.first_grid_ms,
            info.native,
            info.marks
        );
        crate::db::settings::set(
            &state.db,
            "startup.last",
            &serde_json::to_string(&info).unwrap(),
        )
        .map_err(err)?;
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
    let Some(guard) = job::try_start_wait(
        &state.running,
        "영상 촬영일",
        std::time::Duration::from_secs(20),
    ) else {
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
            let Some(Some(mount)) = mounts.get(&vol) else {
                continue;
            };
            let rel = rel.trim_start_matches('/').to_string();
            let path = mount.join(&rel);
            let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
            // 단서(컨테이너·파일명·폴더명·파일 시각) 가운데 가장 이른 그럴듯한 것
            let embedded = crate::media::mp4date::creation_time(&path);
            let md = std::fs::metadata(&path).ok();
            let unix = |t: Option<std::time::SystemTime>| {
                t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
            };
            let now = chrono::Utc::now().timestamp();
            let folder = rel
                .trim_end_matches(&name)
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
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
                    c.execute(
                        "UPDATE files SET taken_at = ?2, taken_at_source = ?3 WHERE id = ?1",
                        rusqlite::params![id, t, src],
                    )
                });
                fixed += 1;
            }
            if last.elapsed().as_millis() >= 200 {
                last = std::time::Instant::now();
                let _ = handle.emit(
                    "video-dates-progress",
                    serde_json::json!({ "done": checked, "total": total }),
                );
            }
        }
        let _ = app.emit("video-dates-done", VideoDatesDone { checked, fixed });
    });
    Ok(())
}

/// 지도 전체 조건의 장수와 경계 — 마커 제한과 무관한 자동 맞춤 기준.
#[tauri::command]
pub async fn map_overview(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<query::MapOverview, String> {
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
    let Some(guard) = job::try_start_wait(
        &state.running,
        "얼굴 찾기",
        std::time::Duration::from_secs(20),
    ) else {
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
                let _ = app.emit(
                    "faces-done",
                    FacesDone {
                        done: p.done,
                        faces: p.faces,
                        persons: c.persons,
                    },
                );
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
                let (thumb, bbox) = match cover.and_then(|c| {
                    c.split_once('|')
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                }) {
                    Some((t, b)) => (Some(t), serde_json::from_str(&b).ok()),
                    None => (None, None),
                };
                Ok(PersonRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
                    cover_thumb: thumb,
                    cover_bbox: bbox,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

#[tauri::command]
pub async fn person_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), String> {
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
            tx.execute(
                "UPDATE faces SET person_id = ?1 WHERE person_id = ?2",
                [into, from],
            )?;
            tx.execute("DELETE FROM persons WHERE id = ?1", [from])?;
            Ok(())
        })
        .map_err(err)
}

/// 글로 찾기 — «바닷가에서 뛰는 강아지» 같은 글에 가까운 사진들.
#[tauri::command]
pub async fn ai_text_search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<SimilarRow>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let text = {
        let mut slot = state.ai_text.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(Arc::new(
                crate::ai::text::Text::load(&state.cache_base).map_err(err)?,
            ));
        }
        Arc::clone(slot.as_ref().unwrap())
    };
    let v = text.embed(q).map_err(err)?;
    let index = ai_index(&state)?;
    similar_rows(&state, index.similar_to(&v, limit.clamp(1, 200), None))
}

#[cfg(test)]
mod tests {
    use super::ms_since_page;

    /// 재로드 판별과 페이지 기준 계산 — 페이지가 늦게 떠도 음수가 되지 않는다
    #[test]
    fn reload_grid_time_is_measured_from_the_page_not_the_process() {
        assert_eq!(ms_since_page(1_051_545, 1_050_500), 1_045);
        assert_eq!(ms_since_page(100, 200), 0, "시계가 어긋나도 음수 대신 0");
    }
}
