use super::*;

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
        let Some(dir) = l.dir.as_deref() else {
            continue;
        };
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
    let Some(guard) =
        job::try_start_wait(&state.running, "스캔", std::time::Duration::from_secs(20))
    else {
        return Err(
            "다른 작업이 아직 도는 중입니다 — 툴바의 작업 표시가 사라진 뒤 다시 눌러 주세요".into(),
        );
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        // 어떻게 끝나든 표시를 내린다
        let _guard = guard;

        let handle = app.clone();
        let r = crate::scan::scan_folder(&db, lib.id, &dir, lib.area, |p| {
            let _ = handle.emit("scan-progress", p);
        });
        match r {
            Ok(p) => {
                // 지명 캐시 적용은 scan_folder 안에서 이미 끝났다 — 이 알림을
                // 받은 화면이 새로 고치면 새 사진에도 이름이 붙어 있다
                let _ = app.emit("scan-done", p);
                // 스캔이 끝나면 곧바로 썸네일을 만든다. 목록은 이미 볼 수 있다.
                // 1차 — 박힌 미리보기를 그대로 받는다. 몇 분이면 그리드가 찬다.
                let tp = crate::scan::thumbs::generate(
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
