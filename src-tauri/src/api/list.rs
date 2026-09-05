use super::*;

/// 한 페이지. `cursor`가 없으면 첫 페이지.
#[tauri::command]
pub async fn files_page(
    state: State<'_, AppState>,
    filter: Filter,
    cursor: Option<Cursor>,
    limit: usize,
    group: Option<query::GroupBy>,
) -> Result<Page, String> {
    // 한 번에 너무 많이 요청하면 IPC가 막힌다.
    let limit = limit.clamp(1, 500);
    query::page(&state.db, &filter, cursor, limit, group.unwrap_or_default()).map_err(err)
}

/// 월별 분포 — 우측 스크러버용.
#[tauri::command]
pub async fn files_timeline(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<Vec<query::Bucket>, String> {
    query::timeline(&state.db, &filter).map_err(err)
}

#[derive(Debug, Serialize)]
pub struct FileSummary {
    pub files: i64,
    pub bytes: i64,
}

/// 현재 필터에 걸린 파일 수와 용량 — 상태바와 툴바가 같은 대상을 가리키게 한다.
#[tauri::command]
pub async fn files_summary(
    state: State<'_, AppState>,
    filter: Filter,
) -> Result<FileSummary, String> {
    let (files, bytes) = query::summary(&state.db, &filter).map_err(err)?;
    Ok(FileSummary { files, bytes })
}

/// 스크롤바 손잡이가 멈춘 자리를 커서로 바꾼다. 그 뒤는 다시 keyset이다.
#[tauri::command]
pub async fn files_cursor_at(
    state: State<'_, AppState>,
    filter: Filter,
    index: i64,
) -> Result<Option<Cursor>, String> {
    query::cursor_at(&state.db, &filter, index).map_err(err)
}

/// 사이드바가 훑어볼 갈래별 장수.
#[tauri::command]
pub async fn files_facets(
    state: State<'_, AppState>,
    filter: Filter,
    kind: query::FacetKind,
) -> Result<Vec<query::Facet>, String> {
    query::facets(&state.db, &filter, kind).map_err(err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_limit_is_clamped() {
        // IPC를 막지 않도록 상한을 둔다
        assert_eq!(0usize.clamp(1, 500), 1);
        assert_eq!(10_000usize.clamp(1, 500), 500);
        assert_eq!(200usize.clamp(1, 500), 200);
    }
}
