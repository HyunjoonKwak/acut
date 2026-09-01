//! 지명 채우기 — 좌표를 «국가 / 시도 / 시군구»로.

use crate::api::{err, job, AppState};
use crate::geo;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// 얼마나 남았나 — 설정 화면이 단추 옆에 보여 준다.
#[tauri::command]
pub async fn geo_stats(state: State<'_, AppState>) -> Result<geo::Stats, String> {
    geo::stats(&state.db).map_err(err)
}

/// 이름 채우기를 시작한다. 진행은 `geo-progress`, 끝나면 `geo-done`.
///
/// `mode` 가 «offline» 이면 내장 자료로 곧바로 채운다 — 망도 설정도 필요 없고
/// 몇 초면 끝난다. «online» 이면 서버에 초당 한 건씩 물어 정밀하게 만든다.
/// 멈추기는 다른 긴 일과 같은 스위치를 쓴다.
#[tauri::command]
pub async fn geo_fill_start(app: AppHandle, limit: Option<usize>, mode: Option<geo::Mode>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    let mode = mode.unwrap_or(geo::Mode::Offline);
    let label = match mode {
        geo::Mode::Offline => "에이컷 지명 채우기",
        geo::Mode::Online => "에이컷 지명 정밀 보강",
    };
    let Some(guard) = job::try_start_wait(&state.running, label, std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 아직 도는 중입니다 — 툴바의 작업 표시가 사라진 뒤 다시 눌러 주세요".into());
    };
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let _guard = guard;
        let handle = app.clone();
        let report = |p: &geo::Progress| {
            let _ = handle.emit("geo-progress", p);
        };
        let r = match mode {
            geo::Mode::Offline => geo::fill_offline(&db, &cancel, limit, report),
            geo::Mode::Online => geo::fill(&db, &cancel, limit, report),
        };
        match r {
            Ok(p) => {
                log::info!(
                    "{label} — 자리 {}곳 · 물어본 {}건 · 사진 {}장 · 이름 없음 {}곳{}",
                    p.done, p.asked, p.files, p.empty,
                    p.stopped.as_deref().map(|s| format!(" · 멈춤: {s}")).unwrap_or_default()
                );
                let _ = app.emit("geo-done", p);
            }
            Err(e) => {
                let _ = app.emit("geo-error", e.to_string());
            }
        }
    });
    Ok(())
}
