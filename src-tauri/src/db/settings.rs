//! 설정 — 열쇠·값 한 줄씩.
//!
//! 프론트가 보기 방식·썸네일 크기 같은 것을 여기 남긴다. 값은 JSON 문자열
//! 그대로다. 무엇이 들어 있는지는 프론트가 안다 — 여기서 풀어 보지 않는다.

use crate::db::conn::{Db, Result};

pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    db.read(|c| {
        c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })
    })
}

pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.write(|c| {
        c.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
    })?;
    Ok(())
}

pub fn remove(db: &Db, key: &str) -> Result<()> {
    db.write(|c| c.execute("DELETE FROM settings WHERE key = ?1", [key]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn missing_key_is_none_not_an_error() {
        let (_d, db) = fresh();
        assert_eq!(get(&db, "없음").unwrap(), None);
    }

    #[test]
    fn set_then_get_then_overwrite() {
        let (_d, db) = fresh();
        set(&db, "prefs", r#"{"thumbSize":180}"#).unwrap();
        assert_eq!(get(&db, "prefs").unwrap().as_deref(), Some(r#"{"thumbSize":180}"#));
        set(&db, "prefs", r#"{"thumbSize":240}"#).unwrap();
        assert_eq!(get(&db, "prefs").unwrap().as_deref(), Some(r#"{"thumbSize":240}"#));
    }

    #[test]
    fn remove_clears_it() {
        let (_d, db) = fresh();
        set(&db, "a", "1").unwrap();
        remove(&db, "a").unwrap();
        assert_eq!(get(&db, "a").unwrap(), None);
        // 없는 것을 지워도 오류가 아니다
        remove(&db, "a").unwrap();
    }
}
