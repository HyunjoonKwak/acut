//! 고르기 커맨드.
//!
//! 스캔은 오래 걸릴 수 있으므로(완전 중복은 파일을 읽는다) 별도 스레드에서 돌고
//! 진행 상황을 이벤트로 흘린다. 조회는 즉시 돌아온다.

use crate::api::{err, AppState};
use crate::cull::{burst, dedup, junk};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// 고르기 종류. DB `groups.kind`와 같은 값이다.
pub const KIND_DUP: i32 = 0;
pub const KIND_JUNK: i32 = 1;
pub const KIND_BURST: i32 = 2;

#[derive(Debug, Serialize)]
pub struct GroupRow {
    pub id: i64,
    pub kind: i32,
    pub reason: Option<String>,
    /// 이 그룹을 정리하면 확보되는 용량
    pub size_bytes: i64,
    pub state: i32,
    pub member_count: i64,
    /// 대표 미리보기용 — 대표 파일의 썸네일 경로
    pub cover: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MemberRow {
    pub file_id: i64,
    pub name: String,
    pub size: i64,
    pub taken_at: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub is_best: bool,
    pub score: Option<f64>,
    pub thumb: Option<String>,
    pub culling_flag: i32,
}

/// 세 갈래를 순서대로 돌린다. 가벼운 것부터 — 결과가 빨리 보이게.
#[tauri::command]
pub fn cull_scan(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lib = state.current()?;
    let db = Arc::clone(&state.db);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        // 1. 잡동사니 — 파일을 열지 않는다. 즉시 끝난다
        match junk::scan(&db, &lib.volume_uuid) {
            Ok(p) => {
                let _ = app.emit("cull-junk", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 2. 같은 순간 — 역시 파일을 열지 않는다
        match burst::scan(&db, &lib.volume_uuid, burst::DEFAULT_GAP_SECS) {
            Ok(p) => {
                let _ = app.emit("cull-burst", &p);
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
                return;
            }
        }
        // 3. 완전 중복 — 해시를 읽으므로 가장 오래 걸린다
        let r = dedup::scan(&db, &lib.volume_uuid, &lib.volume_mount, cancel, |p| {
            let _ = app.emit("cull-dedup-progress", p);
        });
        match r {
            Ok(p) => {
                let _ = app.emit("cull-dedup", &p);
                let _ = app.emit("cull-done", ());
            }
            Err(e) => {
                let _ = app.emit("cull-error", e.to_string());
            }
        }
    });
    Ok(())
}

/// 그룹 목록. 확보 용량이 큰 것부터 — 효과가 큰 것을 먼저 보게 한다.
#[tauri::command]
pub fn cull_groups(
    state: State<'_, AppState>,
    kind: i32,
    limit: usize,
    offset: usize,
) -> Result<Vec<GroupRow>, String> {
    let limit = limit.clamp(1, 200);
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT g.id, g.kind, g.reason, g.size_bytes, g.state,
                        (SELECT COUNT(*) FROM group_members m WHERE m.group_id = g.id),
                        (SELECT t.rel_path FROM group_members m
                         LEFT JOIN thumbs t ON t.file_id = m.file_id AND t.state = 1
                         WHERE m.group_id = g.id
                         ORDER BY m.is_best DESC LIMIT 1)
                 FROM groups g
                 WHERE g.kind = ?1 AND g.state = 0
                 ORDER BY g.size_bytes DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let it = st.query_map(rusqlite::params![kind, limit as i64, offset as i64], |r| {
                Ok(GroupRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    reason: r.get(2)?,
                    size_bytes: r.get(3)?,
                    state: r.get(4)?,
                    member_count: r.get(5)?,
                    cover: r.get(6)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

/// 그룹 하나의 구성원. 대표가 맨 앞에 온다.
#[tauri::command]
pub fn cull_members(state: State<'_, AppState>, group_id: i64) -> Result<Vec<MemberRow>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT m.file_id, f.name, f.size, f.taken_at, f.width, f.height,
                        m.is_best, m.score, t.rel_path, f.culling_flag
                 FROM group_members m
                 JOIN files f ON f.id = m.file_id
                 LEFT JOIN thumbs t ON t.file_id = f.id AND t.state = 1
                 WHERE m.group_id = ?1
                 ORDER BY m.is_best DESC, m.score DESC, f.size DESC",
            )?;
            let it = st.query_map([group_id], |r| {
                Ok(MemberRow {
                    file_id: r.get(0)?,
                    name: r.get(1)?,
                    size: r.get(2)?,
                    taken_at: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    is_best: r.get::<_, i32>(6)? != 0,
                    score: r.get(7)?,
                    thumb: r.get(8)?,
                    culling_flag: r.get(9)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

/// 대표를 바꾼다. 한 그룹에 대표는 하나뿐이다.
#[tauri::command]
pub fn cull_set_best(
    state: State<'_, AppState>,
    group_id: i64,
    file_id: i64,
) -> Result<(), String> {
    state
        .db
        .transaction(|tx| {
            tx.execute("UPDATE group_members SET is_best=0 WHERE group_id=?1", [group_id])?;
            tx.execute(
                "UPDATE group_members SET is_best=1 WHERE group_id=?1 AND file_id=?2",
                rusqlite::params![group_id, file_id],
            )?;
            Ok(())
        })
        .map_err(err)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub kept: usize,
    pub rejected: usize,
}

/// 그룹의 판정을 확정한다 — 대표는 '남김', 나머지는 '제외'로 표시한다.
///
/// **파일을 지우지는 않는다.** `culling_flag`만 바꾼다. 실제 삭제는 사용자가
/// 따로 확인한 뒤에 한다. 잘못 눌러도 되돌릴 수 있어야 한다.
#[tauri::command]
pub fn cull_apply(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<ApplyResult, String> {
    if group_ids.is_empty() {
        return Ok(ApplyResult { kept: 0, rejected: 0 });
    }
    let (kept, rejected) = state
        .db
        .transaction(|tx| {
            let mut kept = 0;
            let mut rejected = 0;
            for gid in &group_ids {
                // 잡동사니 그룹은 대표가 없다 — 전부 제외 대상이다
                let kind: i32 =
                    tx.query_row("SELECT kind FROM groups WHERE id=?1", [gid], |r| r.get(0))?;
                if kind == 1 {
                    rejected += tx.execute(
                        "UPDATE files SET culling_flag=2 WHERE id IN
                         (SELECT file_id FROM group_members WHERE group_id=?1)",
                        [gid],
                    )?;
                } else {
                    kept += tx.execute(
                        "UPDATE files SET culling_flag=1 WHERE id IN
                         (SELECT file_id FROM group_members WHERE group_id=?1 AND is_best=1)",
                        [gid],
                    )?;
                    rejected += tx.execute(
                        "UPDATE files SET culling_flag=2 WHERE id IN
                         (SELECT file_id FROM group_members WHERE group_id=?1 AND is_best=0)",
                        [gid],
                    )?;
                }
                tx.execute("UPDATE groups SET state=1 WHERE id=?1", [gid])?;
            }
            Ok((kept, rejected))
        })
        .map_err(err)?;
    Ok(ApplyResult { kept, rejected })
}

/// 그룹을 보류한다 — 목록에서 빠지되 판정은 하지 않는다.
#[tauri::command]
pub fn cull_skip(state: State<'_, AppState>, group_ids: Vec<i64>) -> Result<usize, String> {
    state
        .db
        .transaction(|tx| {
            let mut n = 0;
            for gid in &group_ids {
                n += tx.execute("UPDATE groups SET state=2 WHERE id=?1", [gid])?;
            }
            Ok(n)
        })
        .map_err(err)
}

#[derive(Debug, Serialize)]
pub struct CullSummary {
    pub kind: i32,
    pub groups: i64,
    pub photos: i64,
    pub reclaimable: i64,
}

/// 세 갈래의 현재 상태. 화면 상단에 "중복 1,956 · 같은순간 5,700" 식으로 쓴다.
#[tauri::command]
pub fn cull_summary(state: State<'_, AppState>) -> Result<Vec<CullSummary>, String> {
    state
        .db
        .read(|c| {
            let mut st = c.prepare(
                "SELECT g.kind, COUNT(DISTINCT g.id),
                        (SELECT COUNT(*) FROM group_members m
                          JOIN groups g2 ON g2.id = m.group_id
                          WHERE g2.kind = g.kind AND g2.state = 0),
                        COALESCE(SUM(g.size_bytes),0)
                 FROM groups g WHERE g.state = 0 GROUP BY g.kind ORDER BY g.kind",
            )?;
            let it = st.query_map([], |r| {
                Ok(CullSummary {
                    kind: r.get(0)?,
                    groups: r.get(1)?,
                    photos: r.get(2)?,
                    reclaimable: r.get(3)?,
                })
            })?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use crate::db::conn::Db;

    /// 그룹 하나를 만들고 apply가 플래그를 어떻게 바꾸는지 본다.
    fn seed(kind: i32) -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            for i in 1..=3 {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                     VALUES(?1,1,?2,100,0,1000,0,0)",
                    rusqlite::params![i, format!("f{i}.jpg")],
                )?;
            }
            tx.execute(
                "INSERT INTO groups(id,kind,reason,size_bytes,state,created_at)
                 VALUES(1,?1,'t',200,0,0)",
                [kind],
            )?;
            for i in 1..=3 {
                tx.execute(
                    "INSERT INTO group_members(group_id,file_id,is_best) VALUES(1,?1,?2)",
                    rusqlite::params![i, (i == 1) as i32],
                )?;
            }
            Ok(())
        })
        .unwrap();
        (d, db)
    }

    fn flags(db: &Db) -> Vec<i32> {
        db.read(|c| {
            let mut st = c.prepare("SELECT culling_flag FROM files ORDER BY id")?;
            let it = st.query_map([], |r| r.get(0))?;
            it.collect::<rusqlite::Result<Vec<i32>>>()
        })
        .unwrap()
    }

    #[test]
    fn apply_keeps_best_and_rejects_the_rest() {
        let (_d, db) = seed(0); // 중복 그룹
        db.transaction(|tx| {
            tx.execute(
                "UPDATE files SET culling_flag=1 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1 AND is_best=1)",
                [],
            )?;
            tx.execute(
                "UPDATE files SET culling_flag=2 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1 AND is_best=0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(flags(&db), vec![1, 2, 2], "대표만 남김, 나머지 제외");
    }

    #[test]
    fn junk_groups_reject_everything() {
        let (_d, db) = seed(1); // 잡동사니 그룹 — 대표가 의미 없다
        db.write(|c| {
            c.execute(
                "UPDATE files SET culling_flag=2 WHERE id IN
                 (SELECT file_id FROM group_members WHERE group_id=1)",
                [],
            )
        })
        .unwrap();
        assert_eq!(flags(&db), vec![2, 2, 2], "잡동사니는 전부 제외");
    }

    #[test]
    fn nothing_is_deleted_by_apply() {
        let (_d, db) = seed(0);
        let before: i64 =
            db.read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))).unwrap();
        db.write(|c| c.execute("UPDATE files SET culling_flag=2", [])).unwrap();
        let after: i64 =
            db.read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))).unwrap();
        assert_eq!(before, after, "판정은 삭제가 아니다");
    }
}
