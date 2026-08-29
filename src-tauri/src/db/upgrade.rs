//! 기존 DB를 새 스키마에 맞춘다.
//!
//! `schema.sql`은 `CREATE TABLE IF NOT EXISTS`라서 **이미 있는 테이블에 컬럼을
//! 더하지는 못한다.** 그런 변경만 여기서 처리한다. 새로 만든 DB에서는 전부
//! 아무 일도 하지 않는 no-op이 되어야 한다.

use rusqlite::Connection;

pub fn run(c: &Connection) -> rusqlite::Result<()> {
    add_library_id(c)?;
    backfill_libraries(c)?;
    add_trash_columns(c)?;
    add_faces_at(c)?;
    add_nas_pulls(c)?;
    rename_old_labels(c)?;
    Ok(())
}

/// 되돌리기 목록의 옛 낱말 — «치우기»를 없애고 «휴지통으로»로 부르기로 했다(2026-08-29).
/// 이미 저장된 묶음 이름도 같은 낱말이어야 단추가 «되돌리기: 제외한 사진 치우기»로 안 뜬다
fn rename_old_labels(c: &Connection) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE batches SET label = replace(label, '치우기', '휴지통으로') WHERE label LIKE '%치우기%'",
        [],
    )?;
    Ok(())
}

/// 휴지통 표시용 컬럼. 파일 행을 지우지 않고 표시만 하는 이유는
/// 되돌릴 때 평점·판정이 살아남아야 하기 때문이다.
fn add_trash_columns(c: &Connection) -> rusqlite::Result<()> {
    for (col, ddl) in [
        ("trashed_at", "ALTER TABLE files ADD COLUMN trashed_at INTEGER"),
        ("trash_path", "ALTER TABLE files ADD COLUMN trash_path TEXT"),
        (
            "trash_batch",
            "ALTER TABLE files ADD COLUMN trash_batch INTEGER REFERENCES batches(id) ON DELETE SET NULL",
        ),
    ] {
        if !has_column(c, "files", col)? {
            c.execute_batch(ddl)?;
        }
    }
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_files_trashed ON files(trashed_at) WHERE trashed_at IS NOT NULL;",
    )
}

/// 얼굴을 찾아 본 시각 — 얼굴이 없어도 남아 다음에 다시 보지 않는다 (4단계)
fn add_faces_at(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "files", "faces_at")? {
        c.execute_batch("ALTER TABLE files ADD COLUMN faces_at INTEGER")?;
    }
    c.execute_batch("CREATE INDEX IF NOT EXISTS idx_files_faces_at ON files(faces_at) WHERE faces_at IS NULL;")
}

/// NAS 1차 구역에서 내려받은 것의 원장 — 비울 때 «우리가 받은 것»만 고른다 (5단계)
fn add_nas_pulls(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS nas_pulls (
            rel_path  TEXT PRIMARY KEY,
            size      INTEGER NOT NULL,
            pulled_at INTEGER NOT NULL
        );",
    )
}

fn has_column(c: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut st = c.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        if r.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `folders.library_id`와 그 인덱스를 보장한다.
///
/// 인덱스를 `schema.sql`에 두면 안 된다. 스키마 배치는 이 함수보다 **먼저**
/// 도는데, 구버전 DB에는 그 시점에 컬럼이 없어 배치 전체가 실패한다.
fn add_library_id(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "folders", "library_id")? {
        c.execute_batch(
            "ALTER TABLE folders ADD COLUMN library_id INTEGER
                 REFERENCES libraries(id) ON DELETE CASCADE;",
        )?;
    }
    c.execute_batch("CREATE INDEX IF NOT EXISTS idx_folders_lib ON folders(library_id);")
}

/// 라이브러리 층이 생기기 전에 스캔한 폴더들을 라이브러리에 붙인다.
///
/// 그때는 볼륨 하나가 곧 라이브러리 하나였다. 그래서 **볼륨마다 폴더 경로의
/// 공통 앞부분**을 찾으면 그게 그 시절의 라이브러리 루트다.
/// 예: `MERGE/사진통합작업/연도별/…`가 전부라면 루트는 `MERGE/사진통합작업`.
fn backfill_libraries(c: &Connection) -> rusqlite::Result<()> {
    let orphan_volumes: Vec<String> = {
        let mut st = c.prepare(
            "SELECT DISTINCT volume_uuid FROM folders WHERE library_id IS NULL",
        )?;
        let it = st.query_map([], |r| r.get::<_, String>(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for vol in orphan_volumes {
        let paths: Vec<String> = {
            let mut st = c.prepare(
                "SELECT rel_path FROM folders WHERE volume_uuid = ?1 AND library_id IS NULL",
            )?;
            let it = st.query_map([&vol], |r| r.get::<_, String>(0))?;
            it.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let root = common_dir_prefix(&paths);
        let name = if root.is_empty() {
            c.query_row("SELECT name FROM volumes WHERE uuid = ?1", [&vol], |r| {
                r.get::<_, String>(0)
            })
            .unwrap_or_else(|_| vol.clone())
        } else {
            root.rsplit('/').next().unwrap_or(&root).to_string()
        };

        c.execute(
            "INSERT INTO libraries(volume_uuid, rel_path, name)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(volume_uuid, rel_path) DO UPDATE SET name = excluded.name",
            rusqlite::params![vol, root, name],
        )?;
        let id: i64 = c.query_row(
            "SELECT id FROM libraries WHERE volume_uuid = ?1 AND rel_path = ?2",
            rusqlite::params![vol, root],
            |r| r.get(0),
        )?;
        c.execute(
            "UPDATE folders SET library_id = ?1 WHERE volume_uuid = ?2 AND library_id IS NULL",
            rusqlite::params![id, vol],
        )?;
    }
    Ok(())
}

/// 경로들의 공통 **디렉터리** 앞부분. 글자 단위가 아니라 `/` 단위로 자른다.
///
/// 글자 단위로 하면 `2003`과 `2004`에서 `200`이 나와 실재하지 않는 폴더가 된다.
///
/// 경로 하나가 다른 것들의 부모이면 그게 그대로 답이다. 스캐너는 **파일이 든
/// 폴더만** 기록하므로, 루트에 사진이 흩어져 있으면 루트도 목록에 들어 있다.
pub fn common_dir_prefix(paths: &[String]) -> String {
    let mut it = paths.iter();
    let Some(first) = it.next() else {
        return String::new();
    };
    let mut prefix: Vec<&str> = first.split('/').collect();

    for p in it {
        let parts: Vec<&str> = p.split('/').collect();
        let keep = prefix
            .iter()
            .zip(parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(keep);
        if prefix.is_empty() {
            break;
        }
    }
    prefix.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::conn::Db;

    /// 구버전 DB를 만든다 — 라이브러리 층이 생기기 전 모양.
    fn legacy_db(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("old.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(include_str!("schema.sql")).unwrap();
        c.execute_batch(
            "DROP INDEX IF EXISTS idx_folders_lib;
             ALTER TABLE folders DROP COLUMN library_id;
             DROP TABLE libraries;",
        )
        .unwrap();
        c.execute_batch(
            "INSERT INTO volumes(uuid,name,role) VALUES('V','MAIN SSD','library');
             INSERT INTO folders(volume_uuid,rel_path,name,area) VALUES
               ('V','MERGE/사진/연도별/2003','2003',1),
               ('V','MERGE/사진/연도별/2004','2004',1),
               ('V','MERGE/사진/주제별/여행','여행',1);",
        )
        .unwrap();
        path
    }

    #[test]
    fn trash_columns_are_added_to_an_old_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch(
                "DROP INDEX IF EXISTS idx_files_trashed;
                 ALTER TABLE files DROP COLUMN trashed_at;
                 ALTER TABLE files DROP COLUMN trash_path;
                 ALTER TABLE files DROP COLUMN trash_batch;",
            )
            .unwrap();
            assert!(!has_column(&c, "files", "trashed_at").unwrap());
        }
        let db = Db::open(&path).expect("구버전 DB도 열려야 한다");
        db.read(|c| {
            assert!(has_column(c, "files", "trashed_at")?);
            assert!(has_column(c, "files", "trash_path")?);
            assert!(has_column(c, "files", "trash_batch")?);
            Ok(())
        })
        .unwrap();
    }

    /// 이 순서를 틀리면 앱이 아예 뜨지 않는다. `schema.sql`이 먼저 도는데
    /// 구버전 DB에는 그 시점에 `library_id`가 없어 배치 전체가 실패했다.
    #[test]
    fn opens_a_database_that_predates_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let path = legacy_db(dir.path());

        let db = Db::open(&path).expect("구버전 DB도 열려야 한다");

        let (id, rel, name): (i64, String, String) = db
            .read(|c| {
                c.query_row("SELECT id, rel_path, name FROM libraries", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
            })
            .expect("라이브러리 하나가 만들어져야 한다");
        assert_eq!(rel, "MERGE/사진", "공통 앞부분이 그 시절의 루트다");
        assert_eq!(name, "사진");

        let attached: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM folders WHERE library_id = ?1",
                    [id],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(attached, 3, "폴더가 전부 붙어야 한다");
    }

    #[test]
    fn upgrading_twice_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = legacy_db(dir.path());
        let count = |db: &Db| -> i64 {
            db.read(|c| c.query_row("SELECT COUNT(*) FROM libraries", [], |r| r.get(0)))
                .unwrap()
        };
        assert_eq!(count(&Db::open(&path).unwrap()), 1);
        assert_eq!(count(&Db::open(&path).unwrap()), 1, "두 번 열어도 하나");
    }

    #[test]
    fn prefix_is_cut_at_slashes() {
        // 글자 단위였다면 "…/연도별/200"이 나온다
        let p = vec![
            "MERGE/사진통합작업/연도별/2003".to_string(),
            "MERGE/사진통합작업/연도별/2004".to_string(),
        ];
        assert_eq!(common_dir_prefix(&p), "MERGE/사진통합작업/연도별");
    }

    #[test]
    fn diverging_subtrees_stop_at_their_parent() {
        // 실제 데이터 모양: 한 루트 아래 연도별/주제별로 갈린다
        let p = vec![
            "MERGE/사진통합작업/연도별/2001".to_string(),
            "MERGE/사진통합작업/주제별/참고이미지들".to_string(),
        ];
        assert_eq!(common_dir_prefix(&p), "MERGE/사진통합작업");
    }

    #[test]
    fn no_common_prefix_means_volume_root() {
        // PHOTO 1처럼 볼륨 최상단을 통째로 잡은 경우
        let p = vec!["가족사진/2003".to_string(), "황금부엉이/Book1".to_string()];
        assert_eq!(common_dir_prefix(&p), "");
        let p = vec!["2003".to_string(), "2004".to_string()];
        assert_eq!(common_dir_prefix(&p), "");
    }

    #[test]
    fn a_parent_in_the_list_is_the_answer() {
        // 루트에 사진이 흩어져 있으면 루트도 목록에 들어 있다
        let p = vec![
            "MERGE/사진통합작업".to_string(),
            "MERGE/사진통합작업/연도별".to_string(),
        ];
        assert_eq!(common_dir_prefix(&p), "MERGE/사진통합작업");
    }

    #[test]
    fn empty_input() {
        assert_eq!(common_dir_prefix(&[]), "");
    }
}
