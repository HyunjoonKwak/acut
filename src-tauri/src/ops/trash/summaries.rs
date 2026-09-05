use super::*;

/// 휴지통에 든 것들의 개수와 용량.
pub fn summary(db: &Db, library_id: Option<i64>) -> Result<Summary> {
    db.read(|c| {
        c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(fi.size),0)
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.trashed_at IS NOT NULL AND (?1 IS NULL OR fo.library_id = ?1)",
            [library_id],
            |r| {
                Ok(Summary {
                    files: r.get(0)?,
                    bytes: r.get(1)?,
                })
            },
        )
    })
}

/// 라이브러리 하나의 휴지통 집계 — 휴지통은 라이브러리마다 따로 있다(같은 디스크 안
/// `.acut/휴지통`). 한 라이브러리 것만 보여 주면 다른 쪽을 빠뜨린다 (2026-08-30 지적).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LibrarySummary {
    pub library_id: i64,
    pub name: String,
    pub files: i64,
    pub bytes: i64,
}

/// 모든 라이브러리의 휴지통을 한눈에 — 빈 것도 0으로 나온다
pub fn summary_by_library(db: &Db) -> Result<Vec<LibrarySummary>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT l.id, l.name,
                    (SELECT COUNT(*) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = l.id AND fi.trashed_at IS NOT NULL),
                    (SELECT COALESCE(SUM(fi.size),0) FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = l.id AND fi.trashed_at IS NOT NULL)
             FROM libraries l ORDER BY l.name",
        )?;
        let it = st.query_map([], |r| {
            Ok(LibrarySummary { library_id: r.get(0)?, name: r.get(1)?, files: r.get(2)?, bytes: r.get(3)? })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 제외로 판정했지만 아직 치우지 않은 것들의 id.
pub fn pending(db: &Db, library_id: Option<i64>) -> Result<Vec<i64>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.culling_flag = 2 AND fi.trashed_at IS NULL
               AND (?1 IS NULL OR fo.library_id = ?1)",
        )?;
        let it = st.query_map([library_id], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 이 폴더들 안에서 제외 표시된 것 — 비교 화면이 «방금 표시한 것만» 치울 때
pub fn pending_in_folders(db: &Db, folder_ids: &[i64]) -> Result<Vec<i64>> {
    if folder_ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = folder_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    db.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT id FROM files WHERE culling_flag = 2 AND trashed_at IS NULL AND folder_id IN ({list})"
        ))?;
        let it = st.query_map([], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}
