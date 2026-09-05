use super::*;

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
    let want: Vec<i64> = libs
        .iter()
        .filter(|l| l.dir.is_some())
        .map(|l| l.id)
        .collect();
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
pub async fn files_by_ids(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<query::FileRow>, String> {
    query::by_ids(&state.db, &ids).map_err(err)
}
