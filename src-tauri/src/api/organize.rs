//! 정리 커맨드 — 이벤트 폴더로 옮기고, 틀리면 되돌린다.

use super::{err, AppState};
use crate::ops::{naming, organize, trash::Outcome, undo};
use tauri::State;

/// 고른 사진들에 붙일 이벤트 이름 후보.
#[tauri::command]
pub fn organize_suggest(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<naming::Suggestion>, String> {
    naming::suggest(&state.db, &ids, 12).map_err(err)
}

/// 고른 사진들의 촬영일 중 가장 이른 날 — 폴더 이름의 앞자리가 된다.
#[tauri::command]
pub fn organize_date(state: State<'_, AppState>, ids: Vec<i64>) -> Result<String, String> {
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
pub fn organize_preview(
    state: State<'_, AppState>,
    library_id: i64,
    date: String,
    title: String,
) -> Result<String, String> {
    let area = crate::db::libraries::get(&state.db, library_id).map_err(err)?.map(|l| l.area).unwrap_or(2);
    Ok(organize::event_rel_dir_for(area, &date, &title))
}

#[tauri::command]
pub fn organize_move(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    library_id: i64,
    date: String,
    title: String,
) -> Result<Outcome, String> {
    let area = crate::db::libraries::get(&state.db, library_id).map_err(err)?.map(|l| l.area).unwrap_or(2);
    let rel_dir = organize::event_rel_dir_for(area, &date, &title);
    let dest = organize::Dest { library_id, rel_dir: rel_dir.clone() };
    let out =
        organize::move_to(&state.db, &ids, &dest, &format!("정리 → {rel_dir}")).map_err(err)?;
    // 비어 버린 폴더 행은 사이드바에서 치운다
    organize::forget_empty_folders(&state.db, library_id).map_err(err)?;
    Ok(out)
}

/// 최근 작업 묶음. 되돌리기 목록에 쓴다.
#[tauri::command]
pub fn batches_recent(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<undo::Batch>, String> {
    undo::recent(&state.db, limit).map_err(err)
}

#[tauri::command]
pub fn batch_undo(state: State<'_, AppState>, batch_id: i64) -> Result<Outcome, String> {
    undo::undo(&state.db, batch_id).map_err(err)
}
