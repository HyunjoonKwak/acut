use super::{scan_plan, KIND_BURST, KIND_DUP, KIND_JUNK, KIND_RESIZED, KIND_SCENE};
use crate::db::conn::Db;

#[test]
fn selective_scan_refreshes_every_dependent_kind() {
    assert_eq!(scan_plan(KIND_JUNK), Some(&[KIND_JUNK][..]));
    assert_eq!(scan_plan(KIND_BURST), Some(&[KIND_BURST, KIND_SCENE][..]));
    assert_eq!(
        scan_plan(KIND_DUP),
        Some(&[KIND_DUP, KIND_RESIZED, KIND_SCENE][..])
    );
    assert_eq!(
        scan_plan(KIND_RESIZED),
        Some(&[KIND_RESIZED, KIND_SCENE][..])
    );
    assert_eq!(scan_plan(KIND_SCENE), Some(&[KIND_SCENE][..]));
    assert_eq!(scan_plan(99), None);
}

/// 그룹 하나를 만들고 apply가 플래그를 어떻게 바꾸는지 본다.
fn seed(kind: i32) -> (tempfile::TempDir, Db) {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(d.path().join("t.db")).unwrap();
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
            [],
        )?;
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

/// 범위 SQL 이 목록·집계에서 같은 무리를 고르는지 — 어긋나면 «머리 숫자와 넘겨 보는 무리가 다르다»
#[test]
fn scope_sql_matches_groups_whose_rejected_copy_is_in_that_library() {
    let (_d, db) = seed(0);
    // 파일 1(대표)은 라이브러리 1, 파일 2·3은 라이브러리 2 로 옮긴다
    db.transaction(|tx| {
            tx.execute("INSERT OR IGNORE INTO volumes(uuid,name,role) VALUES('V','v','library')", [])?;
            tx.execute(
                "INSERT OR IGNORE INTO libraries(id,volume_uuid,rel_path,name) VALUES(1,'V','a','A'),(2,'V','b','B')",
                [],
            )?;
            tx.execute("UPDATE folders SET library_id = 1", [])?;
            tx.execute(
                "INSERT OR REPLACE INTO folders(id,volume_uuid,rel_path,name,area,library_id)
                 VALUES(2,'V','b','b',0,2)",
                [],
            )?;
            tx.execute("UPDATE files SET folder_id = 2 WHERE id IN (2,3)", [])?;
            Ok(())
        })
        .unwrap();
    let count = |lib: Option<i64>| -> i64 {
        db.read(|c| {
            c.query_row(
                &format!(
                    "SELECT COUNT(*) FROM groups g WHERE {}",
                    super::SCOPE.replace("?5", "?1")
                ),
                [lib],
                |r| r.get(0),
            )
        })
        .unwrap()
    };
    assert_eq!(count(None), 1, "범위 없으면 전부");
    assert_eq!(count(Some(2)), 1, "제외될 사본이 B 에 있다");
    assert_eq!(count(Some(1)), 0, "A 엔 대표뿐이라 지울 것이 없다");
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
    let before: i64 = db
        .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
        .unwrap();
    db.write(|c| c.execute("UPDATE files SET culling_flag=2", []))
        .unwrap();
    let after: i64 = db
        .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(before, after, "판정은 삭제가 아니다");
}
