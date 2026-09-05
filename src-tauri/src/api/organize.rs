//! 정리 커맨드 — 이벤트 폴더로 옮기고, 틀리면 되돌린다.

use super::{err, AppState};
use crate::ops::{naming, organize, transfer, trash::Outcome, undo};
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
    pub failed_ids: Vec<i64>,
    pub mode: String,
}

/// 고른 사진들에 붙일 이벤트 이름 후보.
#[tauri::command]
pub async fn organize_suggest(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<naming::Suggestion>, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || naming::suggest(&db, &ids, 12))
        .await
        .map_err(|error| error.to_string())?
        .map_err(err)
}

/// 고른 사진들의 촬영일 중 가장 이른 날 — 폴더 이름의 앞자리가 된다.
#[tauri::command]
pub async fn organize_date(state: State<'_, AppState>, ids: Vec<i64>) -> Result<String, String> {
    if ids.is_empty() {
        return Ok(String::new());
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        db.read(|c| {
            c.query_row(
                &format!(
                    "SELECT COALESCE(date(MIN(taken_at),'unixepoch','localtime'),'')
                     FROM files WHERE id IN ({list})"
                ),
                [],
                |r| r.get(0),
            )
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(err)
}

fn validate_date(date: &str) -> Result<(), String> {
    let bytes = date.as_bytes();
    let exact_iso_shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !exact_iso_shape {
        return Err("날짜는 YYYY-MM-DD 형식의 실제 날짜여야 합니다".into());
    }

    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "날짜는 YYYY-MM-DD 형식의 실제 날짜여야 합니다".into())
}

/// 미리보기 — 실제로 어디로 가는지 보여 준 뒤에 옮긴다.
#[tauri::command]
pub async fn organize_preview(
    state: State<'_, AppState>,
    library_id: i64,
    date: String,
    title: String,
) -> Result<String, String> {
    validate_date(&date)?;
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let area = crate::db::libraries::get(&db, library_id)
            .map_err(err)?
            .map(|l| l.area)
            .unwrap_or(2);
        Ok(organize::event_rel_dir_for(area, &date, &title))
    })
    .await
    .map_err(|error| error.to_string())?
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
    validate_date(&date)?;
    let db = std::sync::Arc::clone(&state.db);
    let running = std::sync::Arc::clone(&state.running);
    let outcome =
        tauri::async_runtime::spawn_blocking(move || -> Result<OrganizeOutcome, String> {
            // 파일·DB 작업과 20초 대기는 WebView의 async executor 밖에서 수행한다.
            let Some(_guard) =
                super::job::try_start_wait(&running, "정리", std::time::Duration::from_secs(20))
            else {
                return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
            };
            let area = crate::db::libraries::get(&db, library_id)
                .map_err(err)?
                .map(|l| l.area)
                .unwrap_or(2);
            let rel_dir = organize::event_rel_dir_for(area, &date, &title);
            let source_areas = organize::areas_of(&db, &ids).map_err(err)?;
            if organize::should_publish(&source_areas, area) {
                let request = transfer::Request {
                    ids,
                    destination_library_id: library_id,
                    destination_dir: rel_dir.clone(),
                    mode: transfer::Mode::Copy,
                    conflict_policy: transfer::ConflictPolicy::Rename,
                    publish: true,
                };
                let out = transfer::execute(&db, &request, &format!("공용 발행 → {rel_dir}"))
                    .map_err(err)?;
                return Ok(OrganizeOutcome {
                    batch_id: out.batch_id,
                    copied: out.completed,
                    failed: out.failed,
                    already_published: out.already_published,
                    bytes: out.bytes,
                    first_error: out.first_error,
                    failed_ids: out.failed_ids,
                    mode: "publish_copy".into(),
                    ..Default::default()
                });
            }
            let dest = organize::Dest {
                library_id,
                rel_dir: rel_dir.clone(),
            };
            let mut sources = organize::libraries_of(&db, &ids).map_err(err)?;
            let out =
                organize::move_to(&db, &ids, &dest, &format!("정리 → {rel_dir}")).map_err(err)?;
            if !sources.contains(&library_id) {
                sources.push(library_id);
            }
            for lib in sources {
                if let Err(error) = organize::forget_empty_folders(&db, lib) {
                    log::warn!("정리 뒤 빈 폴더 행 정리 보류: {error}");
                }
            }
            Ok(OrganizeOutcome {
                batch_id: out.batch_id,
                moved: out.moved,
                failed: out.failed,
                bytes: out.bytes,
                first_error: out.first_error,
                failed_ids: out.failed_ids,
                mode: "move".into(),
                ..Default::default()
            })
        })
        .await
        .map_err(|error| error.to_string())??;
    if outcome.moved + outcome.copied > 0 {
        if let Err(error) = super::start_pending_thumbs(&app, library_id) {
            log::warn!("정리 뒤 썸네일 생성 보류: {error}");
        }
    }
    Ok(outcome)
}

/// 최근 작업 묶음. 되돌리기 목록에 쓴다.
#[tauri::command]
pub async fn batches_recent(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<undo::Batch>, String> {
    // `recent`는 열린 묶음을 닫는 UPDATE 를 먼저 돌려 쓰기 뮤텍스를 잡는다 — 스캔이 쥔 동안
    // 상태바가 async executor 를 막고 기다리면 안 된다 (2차 리뷰 M-10)
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || undo::recent(&db, limit).map_err(err))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn batch_undo(state: State<'_, AppState>, batch_id: i64) -> Result<Outcome, String> {
    // 다른 긴 일(합치기·옮기기·스캔)과 겹쳐 돌지 않게 — 겹치면 서로의 폴더 행을 지우거나 이름이 부딪힌다
    let db = std::sync::Arc::clone(&state.db);
    let running = std::sync::Arc::clone(&state.running);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_guard) =
            super::job::try_start_wait(&running, "되돌리기", std::time::Duration::from_secs(20))
        else {
            return Err("다른 작업이 도는 중입니다. 끝난 뒤에 하세요".into());
        };
        undo::undo(&db, batch_id).map_err(err)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::validate_date;

    #[test]
    fn organize_date_path_segment_must_be_a_real_iso_date() {
        assert!(validate_date("2024-02-29").is_ok());
        assert!(validate_date("2023-02-29").is_err());
        assert!(validate_date("../../사진").is_err());
        assert!(validate_date("2024-1-2").is_err());
    }
}
