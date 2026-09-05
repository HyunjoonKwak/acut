use super::*;

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
            if p.exists() {
                Ok(p)
            } else {
                Err(format!("없는 경로입니다: {s}"))
            }
        })
        .collect()
}

/// 실제로 가져온다. 진행 상황은 `import-progress`로 흘린다.
///
/// 복사가 끝나면 **그 날짜 폴더만** 다시 스캔한다. 라이브러리 전체를 훑으면
/// 몇 장 들이는 데 몇 분이 걸린다. 스캐너는 이미 아는 파일을 건너뛰므로
/// 새로 들어온 것만 읽는다.
#[tauri::command]
pub async fn import_run(
    app: AppHandle,
    sources: Vec<String>,
    library_id: i64,
) -> Result<(), String> {
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
    let Some(guard) = job::try_start_wait(
        &state.running,
        "가져오기",
        std::time::Duration::from_secs(20),
    ) else {
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
                let _ = app.emit(
                    "import-done",
                    crate::ops::import::Report {
                        failed: 1,
                        first_error: Some(e.to_string()),
                        ..Default::default()
                    },
                );
                return;
            }
        };

        // 들어온 자리만 훑는다
        for d in &dirs {
            let _ = crate::scan::scan_folder(&db, library_id, d, lib.area, |_| {});
        }
        if let Err(e) = crate::ops::import::record_imported(&db, rep.batch_id, library_id, since) {
            rep.first_error.get_or_insert(e.to_string());
        }
        let _ = app.emit("import-done", rep);

        // 새로 들어온 것의 썸네일. 이미 있는 것은 건드리지 않는다.
        let _ = crate::scan::thumbs::generate(&db, library_id, &mount, &cache_root, cancel, |p| {
            let _ = app.emit("thumb-progress", p);
        });
        let _ = app.emit("import-thumbs-done", ());
    });
    Ok(())
}
