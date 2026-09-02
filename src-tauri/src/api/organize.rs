//! 정리 커맨드 — 이벤트 폴더로 옮기고, 틀리면 되돌린다.

use super::{err, AppState};
use crate::ops::{naming, organize, trash::Outcome, transfer, undo};
use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Debug, Default, Serialize)]
pub struct OrganizeOutcome {
    pub batch_id: i64,
    pub moved: usize,
    pub copied: usize,
    pub failed: usize,
    pub already_published: usize,
    pub bytes: i64,
    pub first_error: Option<String>,
    pub mode: String,
}

/// 고른 사진들에 붙일 이벤트 이름 후보.
#[tauri::command]
pub async fn organize_suggest(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<naming::Suggestion>, String> {
    naming::suggest(&state.db, &ids, 12).map_err(err)
}

/// 고른 사진들의 촬영일 중 가장 이른 날 — 폴더 이름의 앞자리가 된다.
#[tauri::command]
pub async fn organize_date(state: State<'_, AppState>, ids: Vec<i64>) -> Result<String, String> {
    if ids.is_empty() {
        return Ok(String::new());
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    state
        .db
        .read(|c| {
            c.query_row(
                &format!(
                    "SELECT COALESCE(date(MIN(taken_at),'unixepoch','localtime'),'')
                     FROM files WHERE id IN ({list})"
                ),
                [],
                |r| r.get(0),
            )
        })
        .map_err(err)
}

/// 미리보기 — 실제로 어디로 가는지 보여 준 뒤에 옮긴다.
#[tauri::command]
pub async fn organize_preview(
    state: State<'_, AppState>,
    library_id: i64,
    date: String,
    title: String,
) -> Result<String, String> {
    let area = crate::db::libraries::get(&state.db, library_id).map_err(err)?.map(|l| l.area).unwrap_or(2);
    Ok(organize::event_rel_dir_for(area, &date, &title))
}

#[tauri::command]
pub async fn organize_move(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    library_id: i64,
    date: String,
    title: String,
) -> Result<OrganizeOutcome, String> {
    // 다른 긴 일(합치기·옮기기·스캔)과 겹쳐 돌지 않게 — 겹치면 서로의 폴더 행을 지우거나 이름이 부딪힌다
    let Some(guard) = super::job::try_start_wait(&state.running, "정리", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    let area = crate::db::libraries::get(&state.db, library_id).map_err(err)?.map(|l| l.area).unwrap_or(2);
    let rel_dir = organize::event_rel_dir_for(area, &date, &title);
    let source_areas = organize::areas_of(&state.db, &ids).map_err(err)?;
    if organize::should_publish(&source_areas, area) {
        let request = transfer::Request {
            ids,
            destination_library_id: library_id,
            destination_dir: rel_dir.clone(),
            mode: transfer::Mode::Copy,
            conflict_policy: transfer::ConflictPolicy::Rename,
            publish: true,
        };
        let out = transfer::execute(&state.db, &request, &format!("공용 발행 → {rel_dir}"))
            .map_err(err)?;
        drop(guard);
        if out.completed > 0 {
            if let Err(error) = super::start_pending_thumbs(&app, library_id) {
                log::warn!("공용 발행 뒤 썸네일 생성 보류: {error}");
            }
        }
        return Ok(OrganizeOutcome {
            batch_id: out.batch_id,
            copied: out.completed,
            failed: out.failed,
            already_published: out.already_published,
            bytes: out.bytes,
            first_error: out.first_error,
            mode: "publish_copy".into(),
            ..Default::default()
        });
    }
    let dest = organize::Dest { library_id, rel_dir: rel_dir.clone() };
    // 비는 폴더는 **떠난 쪽** 라이브러리에 생긴다 — 옮기기 전에 어느 라이브러리들인지 적어 둔다
    let mut sources = organize::libraries_of(&state.db, &ids).map_err(err)?;
    let out =
        organize::move_to(&state.db, &ids, &dest, &format!("정리 → {rel_dir}")).map_err(err)?;
    // 비어 버린 폴더 행은 사이드바에서 치운다
    if !sources.contains(&library_id) {
        sources.push(library_id);
    }
    for lib in sources {
        organize::forget_empty_folders(&state.db, lib).map_err(err)?;
    }
    Ok(OrganizeOutcome {
        batch_id: out.batch_id,
        moved: out.moved,
        failed: out.failed,
        bytes: out.bytes,
        first_error: out.first_error,
        mode: "move".into(),
        ..Default::default()
    })
}

/// 최근 작업 묶음. 되돌리기 목록에 쓴다.
#[tauri::command]
pub async fn batches_recent(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<undo::Batch>, String> {
    undo::recent(&state.db, limit).map_err(err)
}

#[tauri::command]
pub async fn batch_undo(state: State<'_, AppState>, batch_id: i64) -> Result<Outcome, String> {
    // 다른 긴 일(합치기·옮기기·스캔)과 겹쳐 돌지 않게 — 겹치면 서로의 폴더 행을 지우거나 이름이 부딪힌다
    let Some(_guard) = super::job::try_start_wait(&state.running, "되돌리기", std::time::Duration::from_secs(20)) else {
        return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
    };
    undo::undo(&state.db, batch_id).map_err(err)
}
