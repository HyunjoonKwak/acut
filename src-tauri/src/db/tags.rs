//! 태그 — 폴더로는 표현 못 하는 갈래.
//!
//! 사진 하나는 폴더 한 곳에만 있지만 태그는 여럿 붙는다. 「주원」이면서
//! 「생일」인 사진이 그렇다.
//!
//! 이름은 앞뒤 공백을 떼고 NFC로 맞춘다 — 맥에서 친 한글과 붙여넣은 한글이
//! 다른 태그가 되면 안 된다.

use crate::db::conn::{Db, DbError, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    /// 이 태그가 붙은 사진 수 (버린 것은 뺀다)
    pub count: i64,
}

fn read_tags(c: &rusqlite::Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Tag>> {
    let mut st = c.prepare(sql)?;
    let it = st.query_map(p, |r| {
        Ok(Tag {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            count: r.get(3)?,
        })
    })?;
    it.collect()
}

/// 태그 목록. 많이 쓴 것부터.
pub fn list(db: &Db) -> Result<Vec<Tag>> {
    db.read(|c| {
        read_tags(
            c,
            "SELECT t.id, t.name, t.color,
                    (SELECT COUNT(*) FROM file_tags ft
                       JOIN files fi ON fi.id = ft.file_id
                      WHERE ft.tag_id = t.id AND fi.trashed_at IS NULL)
             FROM tags t ORDER BY 4 DESC, t.name",
            &[],
        )
    })
}

/// 한 장에 붙은 태그.
pub fn of_file(db: &Db, file_id: i64) -> Result<Vec<Tag>> {
    db.read(|c| {
        read_tags(
            c,
            "SELECT t.id, t.name, t.color, 0
             FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
             WHERE ft.file_id = ?1 ORDER BY t.name",
            &[&file_id],
        )
    })
}

/// 고른 사진들에 태그를 붙인다. 없는 태그면 만든다.
///
/// 이미 붙어 있는 것은 조용히 넘어간다 — 두 번 눌러도 같은 결과여야 한다.
pub fn add(db: &Db, ids: &[i64], name: &str) -> Result<i64> {
    let name = crate::scan::nfc(name.trim());
    if name.is_empty() {
        return Err(DbError::Invalid("태그 이름이 비어 있습니다".into()));
    }
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO tags(name) VALUES(?1) ON CONFLICT(name) DO NOTHING",
            [&name],
        )?;
        let id: i64 = tx.query_row("SELECT id FROM tags WHERE name = ?1", [&name], |r| r.get(0))?;
        let mut ins = tx.prepare(
            "INSERT INTO file_tags(file_id, tag_id) VALUES(?1, ?2) ON CONFLICT DO NOTHING",
        )?;
        for f in ids {
            ins.execute(rusqlite::params![f, id])?;
        }
        Ok(id)
    })
}

/// 고른 사진들에서 태그를 뗀다. 태그 자체는 남는다.
pub fn remove(db: &Db, ids: &[i64], tag_id: i64) -> Result<()> {
    db.transaction(|tx| {
        let mut del = tx.prepare("DELETE FROM file_tags WHERE file_id = ?1 AND tag_id = ?2")?;
        for f in ids {
            del.execute(rusqlite::params![f, tag_id])?;
        }
        Ok(())
    })?;
    Ok(())
}

/// 태그 자체를 지운다. 붙어 있던 것도 함께 떨어진다 (CASCADE).
pub fn delete(db: &Db, tag_id: i64) -> Result<()> {
    db.write(|c| c.execute("DELETE FROM tags WHERE id = ?1", [tag_id]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
                [],
            )?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            for i in 1..=5i64 {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                     VALUES(?,1,?,1,0,0,0,0)",
                    rusqlite::params![i, format!("IMG_{i}.jpg")],
                )?;
            }
            Ok(())
        })
        .unwrap();
        (dir, db)
    }

    #[test]
    fn adding_the_same_tag_twice_changes_nothing() {
        let (_d, db) = seeded();
        let a = add(&db, &[1, 2], "여행").unwrap();
        let b = add(&db, &[1, 2], "여행").unwrap();
        assert_eq!(a, b, "같은 이름이면 같은 태그여야 한다");
        assert_eq!(list(&db).unwrap().len(), 1);
        assert_eq!(list(&db).unwrap()[0].count, 2);
        assert_eq!(of_file(&db, 1).unwrap().len(), 1);
    }

    /// 맥에서 친 한글(NFC)과 파인더가 준 한글(NFD)은 바이트가 다르다.
    /// 맞춰 두지 않으면 눈에 똑같은 태그가 두 줄로 갈라진다.
    #[test]
    fn tag_names_are_normalized() {
        let (_d, db) = seeded();
        let nfc = add(&db, &[1], "가족").unwrap();
        // 같은 글자를 NFD로 — 자모를 따로 쓴다. 눈에는 똑같다.
        let decomposed = "\u{1100}\u{1161}\u{110C}\u{1169}\u{11A8}";
        assert_ne!(decomposed, "가족", "테스트 입력이 이미 NFC면 시험이 안 된다");
        let nfd = add(&db, &[2], decomposed).unwrap();
        assert_eq!(nfc, nfd);
        assert_eq!(list(&db).unwrap().len(), 1);

        // 앞뒤 공백도 같은 태그
        assert_eq!(add(&db, &[3], "  가족 ").unwrap(), nfc);
        assert_eq!(list(&db).unwrap()[0].count, 3);
    }

    #[test]
    fn empty_names_are_refused() {
        let (_d, db) = seeded();
        assert!(add(&db, &[1], "   ").is_err());
        assert!(list(&db).unwrap().is_empty());
    }

    /// 태그를 떼도 태그 자체는 남는다 — 남은 사진이 없다고 이름까지 없애면
    /// 다시 붙일 때 오타가 난다.
    #[test]
    fn removing_from_files_keeps_the_tag() {
        let (_d, db) = seeded();
        let id = add(&db, &[1, 2, 3], "여행").unwrap();
        remove(&db, &[1, 2, 3], id).unwrap();
        let l = list(&db).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].count, 0);
        assert!(of_file(&db, 1).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_tag_detaches_it_everywhere() {
        let (_d, db) = seeded();
        let id = add(&db, &[1, 2], "여행").unwrap();
        add(&db, &[1], "가족").unwrap();
        delete(&db, id).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
        // 1번에는 「가족」만 남는다
        let mine = of_file(&db, 1).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].name, "가족");
    }

    /// 버린 사진은 세지 않는다 — 휴지통에 있는 것까지 세면 목록의 장수와
    /// 실제로 보이는 장수가 어긋난다.
    #[test]
    fn trashed_files_are_not_counted() {
        let (_d, db) = seeded();
        add(&db, &[1, 2, 3], "여행").unwrap();
        db.write(|c| c.execute("UPDATE files SET trashed_at = 1 WHERE id = 1", []))
            .unwrap();
        assert_eq!(list(&db).unwrap()[0].count, 2);
    }
}
