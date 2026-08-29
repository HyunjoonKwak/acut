//! 휴지통 커맨드.
//!
//! 되돌릴 수 없는 일(`trash_empty`)은 프론트에서 개수와 용량을 보여 주고
//! 확인을 받은 뒤에만 부른다. 여기서는 막지 않는다 — 확인은 화면의 몫이다.

use super::{err, AppState};
use crate::ops::trash;
use tauri::State;

/// 제외로 판정했지만 아직 치우지 않은 것들의 개수·용량.
#[derive(Debug, serde::Serialize)]
pub struct Pending {
    pub files: i64,
    pub bytes: i64,
}

#[tauri::command]
pub async fn trash_pending(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<Pending, String> {
    state
        .db
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(fi.size),0)
                 FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                 WHERE fi.culling_flag = 2 AND fi.trashed_at IS NULL
                   AND (?1 IS NULL OR fo.library_id = ?1)",
                [library_id],
                |r| Ok(Pending { files: r.get(0)?, bytes: r.get(1)? }),
            )
        })
        .map_err(err)
}

/// 휴지통에 든 것들.
#[tauri::command]
pub async fn trash_summary(
    state: State<'_, AppState>,
    library_id: Option<i64>,
) -> Result<trash::Summary, String> {
    trash::summary(&state.db, library_id).map_err(err)
}

/// 제외로 판정한 것을 전부 휴지통으로 옮긴다.
#[tauri::command]
pub async fn trash_apply(
    state: State<'_, AppState>,
    library_id: Option<i64>,
    folder_ids: Option<Vec<i64>>,
) -> Result<trash::Outcome, String> {
    // 폴더 목록이 오면 그 안의 제외분만 — 비교 화면의 «표시한 것만 치우기»
    let ids = match folder_ids {
        Some(f) => trash::pending_in_folders(&state.db, &f).map_err(err)?,
        None => trash::pending(&state.db, library_id).map_err(err)?,
    };
    trash::to_trash(&state.db, &ids, "제외한 사진 치우기").map_err(err)
}

/// 고른 것만 휴지통으로.
#[tauri::command]
pub async fn trash_files(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<trash::Outcome, String> {
    trash::to_trash(&state.db, &ids, "고른 사진 치우기").map_err(err)
}

/// 휴지통에서 제자리로. `ids`가 비어 있으면 전부.
#[tauri::command]
pub async fn trash_restore(
    state: State<'_, AppState>,
    library_id: Option<i64>,
    ids: Vec<i64>,
) -> Result<trash::Outcome, String> {
    let ids = if ids.is_empty() {
        trashed_ids(&state, library_id)?
    } else {
        ids
    };
    trash::restore(&state.db, &ids).map_err(err)
}

/// 휴지통을 비운다. **되돌릴 수 없다.** `ids`가 비어 있으면 전부.
#[tauri::command]
pub async fn trash_empty(
    state: State<'_, AppState>,
    library_id: Option<i64>,
    ids: Vec<i64>,
) -> Result<trash::Outcome, String> {
    let ids = if ids.is_empty() {
        trashed_ids(&state, library_id)?
    } else {
        ids
    };
    trash::empty(&state.db, &ids).map_err(err)
}

fn trashed_ids(state: &State<'_, AppState>, library_id: Option<i64>) -> Result<Vec<i64>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT fi.id FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                 WHERE fi.trashed_at IS NOT NULL AND (?1 IS NULL OR fo.library_id = ?1)",
            )?;
            let it = st.query_map([library_id], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}
