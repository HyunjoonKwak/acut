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
    add_image_hash(c)?;
    add_done_at(c)?;
    add_geo_levels(c)?;
    add_nas_pulls(c)?;
    rename_old_labels(c)?;
    migrate_taken_at_to_utc(c)?;
    Ok(())
}

/// 초기 버전이 UTC처럼 저장했던 시간대 없는 EXIF/파일명 시각을 실제 Unix
/// 시각으로 한 번만 바꾼다. 파일명은 재파싱해 13자리 epoch 값은 이동하지 않는다.
fn migrate_taken_at_to_utc(c: &Connection) -> rusqlite::Result<()> {
    const KEY: &str = "internal.taken_at_utc_v1";
    let done: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1)",
        [KEY],
        |r| r.get(0),
    )?;
    if done {
        return Ok(());
    }

    let rows: Vec<(i64, String, i32, i32, i64, String)> = {
        let mut st = c.prepare(
            "SELECT fi.id, fi.name, fi.kind, fi.taken_at_source, fi.taken_at, fo.rel_path
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let tx = c.unchecked_transaction()?;
    {
        let mut update = tx.prepare("UPDATE files SET taken_at = ?2 WHERE id = ?1")?;
        for (id, name, kind, source, old, folder) in rows {
            let migrated = match source {
                0 if kind != 1 => crate::media::taken_at::floating_civil_to_unix(old),
                1 => crate::media::taken_at::from_filename(&name)
                    .or_else(|| folder.rsplit('/').next().and_then(crate::media::taken_at::from_filename))
                    .unwrap_or(old),
                _ => old,
            };
            if migrated != old {
                update.execute(rusqlite::params![id, migrated])?;
            }
        }
    }
    tx.execute("INSERT INTO settings(key,value) VALUES(?1,'1')", [KEY])?;
    tx.commit()
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
/// 메타데이터만 다른 사본을 찾는 «그림 해시»(2026-08-30) — 촬영일시 EXIF 를 나중에 써 넣은
/// 사본은 바이트가 달라 완전 중복에서 빠졌다 (실측: 하와이 1,081장)
fn add_image_hash(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "files", "image_hash")? {
        c.execute_batch("ALTER TABLE files ADD COLUMN image_hash TEXT")?;
    }
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_files_image_hash ON files(image_hash) WHERE image_hash IS NOT NULL;",
    )
}

/// «처리됨 보기»(2026-08-31) — 확정한 무리를 최근 순으로 다시 보고 무리 단위로 취소한다
fn add_done_at(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "groups", "done_at")? {
        c.execute_batch("ALTER TABLE groups ADD COLUMN done_at INTEGER")?;
    }
    Ok(())
}

/// 지명 3단계(2026-09-01) — 국가·시도·시군구와 격자 캐시. 좌표만 보이던 위치 갈래를
/// 사람이 읽는 이름으로 묶기 위한 것. 값은 «지명 채우기»가 나중에 채운다.
fn add_geo_levels(c: &Connection) -> rusqlite::Result<()> {
    for col in ["geo_country", "geo_admin1", "geo_admin2"] {
        if !has_column(c, "files", col)? {
            c.execute_batch(&format!("ALTER TABLE files ADD COLUMN {col} TEXT"))?;
        }
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS places (
            cell TEXT PRIMARY KEY, country TEXT, admin1 TEXT, admin2 TEXT, name TEXT,
            status TEXT NOT NULL DEFAULT 'ok', at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_files_geo ON files(geo_country, geo_admin1, geo_admin2);",
    )?;
    if !has_column(c, "places", "status")? {
        c.execute_batch("ALTER TABLE places ADD COLUMN status TEXT NOT NULL DEFAULT 'ok'")?;
    }
    // 첫 지명 구현은 «결과 없음»도 세 이름이 모두 NULL인 캐시 행으로 남겼다.
    // status를 단순 DEFAULT 'ok'로 더하면 그 행은 성공 캐시가 되어 다시 묻지도,
    // 파일을 완료시키지도 못한다. 이미 그 중간 빌드를 열어 status가 생긴 DB도
    // 복구해야 하므로 컬럼 추가 여부와 무관하게 매번 멱등으로 보정한다.
    c.execute_batch(
        "UPDATE places
            SET status = 'none'
          WHERE country IS NULL OR trim(country) = '';",
    )?;
    Ok(())
}

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
    fn image_hash_column_is_added_to_an_old_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch(
                "DROP INDEX IF EXISTS idx_files_image_hash;
                 ALTER TABLE files DROP COLUMN image_hash;",
            )
            .unwrap();
            assert!(!has_column(&c, "files", "image_hash").unwrap());
        }
        let db = Db::open(&path).expect("그림 해시 전 DB도 열려야 한다");
        db.read(|c| {
            assert!(has_column(c, "files", "image_hash")?);
            Ok(())
        })
        .unwrap();
        db.write(|c| c.execute("UPDATE files SET image_hash='abc'", [])).unwrap();
    }

    #[test]
    fn done_at_column_is_added_to_an_old_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch("ALTER TABLE groups DROP COLUMN done_at;").unwrap();
        }
        let db = Db::open(&path).expect("done_at 전 DB도 열려야 한다");
        db.write(|c| c.execute("UPDATE groups SET done_at = 1", [])).unwrap();
    }

    /// 업그레이드가 나중에 더하는 칸을 schema.sql 이 먼저 참조하면 구버전 DB 가 안 열린다.
    /// v0.5.4 DB 에서 실제로 «no such column: geo_country» 로 죽었다 (2026-09-01).
    /// 새 칸을 넣을 때마다 이 목록에 더한다 — 사람이 기억하지 않아도 시험이 잡게.
    #[test]
    fn schema_never_mentions_a_column_that_upgrade_adds_later() {
        let schema = include_str!("schema.sql");
        for col in ["trashed_at", "trash_path", "trash_batch", "faces_at", "image_hash",
                    "done_at", "geo_country", "geo_admin1", "geo_admin2"] {
            for line in schema.lines() {
                let l = line.trim();
                if l.starts_with("CREATE INDEX") && l.contains(col) {
                    panic!("schema.sql 의 인덱스가 upgrade 전용 칸 «{col}»을 참조한다 — 구버전 DB 가 안 열린다:\n{l}");
                }
            }
        }
    }

    /// 지명 칸이 없던 DB(v0.5.4)도 그대로 열려야 한다
    #[test]
    fn a_database_from_before_place_names_still_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch(
                "DROP INDEX IF EXISTS idx_files_geo;
                 ALTER TABLE files DROP COLUMN geo_country;
                 ALTER TABLE files DROP COLUMN geo_admin1;
                 ALTER TABLE files DROP COLUMN geo_admin2;
                 DROP TABLE IF EXISTS places;",
            )
            .unwrap();
            assert!(!has_column(&c, "files", "geo_country").unwrap());
        }
        let db = Db::open(&path).expect("지명 칸이 없던 DB 도 열려야 한다");
        db.read(|c| {
            assert!(has_column(c, "files", "geo_country")?);
            assert!(has_column(c, "files", "geo_admin2")?);
            Ok(())
        })
        .unwrap();
        db.write(|c| c.execute("INSERT INTO places(cell,at) VALUES('0.00,0.00',0)", []))
            .unwrap();
    }

    /// 첫 지명 빌드가 남긴 빈 캐시는 status 칸이 없었다. 새 칸의 기본값 'ok'를
    /// 그대로 주면 성공으로 오인하므로, 이름 없는 행은 'none'으로 복구해야 한다.
    #[test]
    fn empty_place_rows_from_the_first_geo_build_become_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch(
                "DROP TABLE places;
                 CREATE TABLE places (
                   cell TEXT PRIMARY KEY, country TEXT, admin1 TEXT, admin2 TEXT,
                   name TEXT, at INTEGER NOT NULL
                 );
                 INSERT INTO places(cell,country,admin1,admin2,name,at)
                   VALUES('10.00,20.00',NULL,NULL,NULL,NULL,0),
                         ('37.28,127.05','대한민국','경기도','수원시','수원시',0);",
            )
            .unwrap();
        }

        let db = Db::open(&path).expect("첫 지명 빌드의 DB도 열려야 한다");
        let statuses: Vec<(String, String)> = db
            .read(|c| {
                let mut st = c.prepare("SELECT cell,status FROM places ORDER BY cell")?;
                let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            statuses,
            vec![("10.00,20.00".into(), "none".into()), ("37.28,127.05".into(), "ok".into())]
        );

        // 중간 수정 빌드가 이미 status='ok'를 붙인 DB도 다음 실행에서 복구한다.
        db.write(|c| c.execute("UPDATE places SET status='ok' WHERE cell='10.00,20.00'", []))
            .unwrap();
        drop(db);
        let reopened = Db::open(&path).unwrap();
        let repaired: String = reopened
            .read(|c| c.query_row("SELECT status FROM places WHERE cell='10.00,20.00'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(repaired, "none");
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
    fn old_floating_photo_dates_are_migrated_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        let old = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            .and_hms_opt(18, 0, 0).unwrap().and_utc().timestamp();
        db.write(|c| {
            c.execute_batch("DELETE FROM settings WHERE key='internal.taken_at_utc_v1';")?;
            c.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','v','library')", [])?;
            c.execute("INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','p','p',1)", [])?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,1,'photo.jpg',1,0,?1,0,0)",
                [old],
            )?;
            migrate_taken_at_to_utc(c)
        }).unwrap();

        let migrated: i64 = db.read(|c| c.query_row("SELECT taken_at FROM files", [], |r| r.get(0))).unwrap();
        assert_eq!(migrated, crate::media::taken_at::civil_to_unix(2024, 1, 1, 18, 0, 0));
        db.write(migrate_taken_at_to_utc).unwrap();
        let again: i64 = db.read(|c| c.query_row("SELECT taken_at FROM files", [], |r| r.get(0))).unwrap();
        assert_eq!(again, migrated, "두 번 열어도 다시 시간대를 적용하면 안 된다");
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
