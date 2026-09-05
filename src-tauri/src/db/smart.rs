//! 스마트 앨범 — 조건에 이름을 붙여 둔 것.
//!
//! 폴더가 "어디에 있나"라면 이것은 "어떤 것인가"다. 「별 4개 이상 영상」처럼
//! 되풀이해 쓰는 조건을 매번 다시 고르지 않게 한다.
//!
//! 조건은 `Filter`를 그대로 JSON으로 담는다. 필터가 늘어나도 스키마를 건드릴
//! 일이 없고, 프론트가 보낸 것을 그대로 되돌려 주면 된다.

use crate::db::conn::{Db, DbError, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct SmartAlbum {
    pub id: i64,
    pub name: String,
    /// 프론트가 그대로 filter로 쓸 값
    pub filter: serde_json::Value,
    pub sort: Option<serde_json::Value>,
}

pub fn list(db: &Db) -> Result<Vec<SmartAlbum>> {
    db.read(|c| {
        let mut st = c.prepare("SELECT id, name, filter, sort FROM smart_albums ORDER BY name")?;
        let it = st.query_map([], |r| {
            let f: String = r.get(2)?;
            let s: Option<String> = r.get(3)?;
            Ok(SmartAlbum {
                id: r.get(0)?,
                name: r.get(1)?,
                // 손으로 고친 DB가 아니면 깨질 일이 없다. 깨졌더라도 목록
                // 전체를 못 읽게 만들 이유는 없어 null로 흘려보낸다.
                filter: serde_json::from_str(&f).unwrap_or(serde_json::Value::Null),
                sort: s.and_then(|x| serde_json::from_str(&x).ok()),
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 지금 걸린 조건을 이름 붙여 저장한다. 같은 이름이면 덮어쓴다.
pub fn save(
    db: &Db,
    name: &str,
    filter: &serde_json::Value,
    sort: Option<&serde_json::Value>,
) -> Result<i64> {
    let name = crate::scan::nfc(name.trim());
    if name.is_empty() {
        return Err(DbError::Invalid("이름이 비어 있습니다".into()));
    }
    let f = filter.to_string();
    let s = sort.map(|x| x.to_string());
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO smart_albums(name, filter, sort) VALUES(?1,?2,?3)
             ON CONFLICT(name) DO UPDATE SET filter=excluded.filter, sort=excluded.sort",
            rusqlite::params![name, f, s],
        )?;
        tx.query_row(
            "SELECT id FROM smart_albums WHERE name = ?1",
            [&name],
            |r| r.get(0),
        )
    })
}

pub fn delete(db: &Db, id: i64) -> Result<()> {
    db.write(|c| c.execute("DELETE FROM smart_albums WHERE id = ?1", [id]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fresh() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    /// 넣은 조건이 그대로 나와야 한다. 여기서 한 자리라도 어긋나면 스마트
    /// 앨범을 눌렀을 때 저장할 때와 다른 사진이 뜬다.
    #[test]
    fn a_saved_filter_round_trips_unchanged() {
        let (_d, db) = fresh();
        let f = json!({
            "kind": 1,
            "min_rating": 4,
            "favorite_only": false,
            "name_like": null,
            "camera": "ILCE-7M4",
            "place": "37.5,126.9",
            "tag_id": 3,
        });
        let srt = json!({ "by": "size", "desc": true });
        save(&db, "별 넷 영상", &f, Some(&srt)).unwrap();

        let got = list(&db).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "별 넷 영상");
        assert_eq!(got[0].filter, f);
        assert_eq!(got[0].sort.as_ref(), Some(&srt));
    }

    /// 같은 이름으로 저장하면 하나가 더 생기는 게 아니라 덮어쓴다 —
    /// 「여행」이 세 줄 늘어서면 어느 것이 최신인지 알 수 없다.
    #[test]
    fn saving_the_same_name_replaces_it() {
        let (_d, db) = fresh();
        let a = save(&db, "여행", &json!({"kind": 0}), None).unwrap();
        let b = save(&db, "여행", &json!({"kind": 1}), None).unwrap();
        assert_eq!(a, b);
        let got = list(&db).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].filter["kind"], json!(1));
    }

    #[test]
    fn names_are_trimmed_and_normalized() {
        let (_d, db) = fresh();
        let a = save(&db, "가족", &json!({}), None).unwrap();
        // 앞뒤 공백을 떼고, 자모를 따로 친 NFD도 같은 이름으로 본다
        let b = save(
            &db,
            "  \u{1100}\u{1161}\u{110C}\u{1169}\u{11A8} ",
            &json!({}),
            None,
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn empty_names_are_refused() {
        let (_d, db) = fresh();
        assert!(save(&db, "  ", &json!({}), None).is_err());
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn sort_may_be_absent() {
        let (_d, db) = fresh();
        save(&db, "정렬 없음", &json!({"kind": 2}), None).unwrap();
        assert!(list(&db).unwrap()[0].sort.is_none());
    }

    #[test]
    fn deleting_removes_only_that_one() {
        let (_d, db) = fresh();
        let a = save(&db, "가", &json!({}), None).unwrap();
        save(&db, "나", &json!({}), None).unwrap();
        delete(&db, a).unwrap();
        let got = list(&db).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "나");
    }
}
