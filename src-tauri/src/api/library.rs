use super::*;

/// 등록된 것 전부. 디스크가 빠진 것도 포함한다.
#[tauri::command]
pub async fn libraries_list(state: State<'_, AppState>) -> Result<Vec<LibRow>, String> {
    crate::db::libraries::list(&state.db).map_err(err)
}

/// 폴더를 라이브러리로 등록한다. 스캔은 하지 않는다 (`scan_start`가 한다).
#[tauri::command]
pub async fn library_add(
    state: State<'_, AppState>,
    path: String,
    area: i32,
) -> Result<LibRow, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let r = crate::db::libraries::add(&state.db, &dir, area);
    state.forget_dirs();
    r
}

/// 등록을 지운다. **원본 사진과 디스크의 캐시 파일은 건드리지 않는다.**
/// 라이브러리의 영역(역할)을 바꾼다 — 0 작업대 · 1 내사진 · 2 공용 · 3 기타
#[tauri::command]
pub async fn library_set_area(
    state: State<'_, AppState>,
    id: i64,
    area: i32,
) -> Result<(), String> {
    if !(0..=3).contains(&area) {
        return Err(format!("모르는 영역: {area}"));
    }
    crate::db::libraries::set_area(&state.db, id, area).map_err(err)
}

#[tauri::command]
pub async fn library_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let r = crate::db::libraries::remove(&state.db, id).map_err(err);
    state.forget_dirs();
    r
}
