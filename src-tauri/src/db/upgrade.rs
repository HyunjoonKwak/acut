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
    add_phash(c)?;
    add_live_count_index(c)?;
    add_done_at(c)?;
    add_geo_levels(c)?;
    add_nas_pulls(c)?;
    add_gallery_transition_p0(c)?;
    add_gallery_transition_p1(c)?;
    add_release_091_integrity(c)?;
    add_journal_file_stat(c)?;
    rename_old_labels(c)?;
    migrate_taken_at_to_utc(c)?;
    Ok(())
}

/// 0.9.1 무결성 보강. 0.9.0 저널은 해시가 없으므로 NULL로 남겨 두고 undo에서
/// 보수적으로 거절한다. 새 작업만 완전한 before/after 및 copy manifest를 가진다.
fn add_release_091_integrity(c: &Connection) -> rusqlite::Result<()> {
    for (column, ddl) in [
        (
            "before_sha256",
            "ALTER TABLE capture_date_journal ADD COLUMN before_sha256 TEXT",
        ),
        (
            "after_sha256",
            "ALTER TABLE capture_date_journal ADD COLUMN after_sha256 TEXT",
        ),
        (
            "undone_at",
            "ALTER TABLE capture_date_journal ADD COLUMN undone_at INTEGER",
        ),
    ] {
        if !has_column(c, "capture_date_journal", column)? {
            c.execute_batch(ddl)?;
        }
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS copy_manifest (
            batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
            file_id INTEGER NOT NULL,
            seq INTEGER NOT NULL,
            to_vol TEXT NOT NULL,
            to_path TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            is_main INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (batch_id,file_id,seq),
            UNIQUE (batch_id,to_vol,to_path)
         );
         CREATE INDEX IF NOT EXISTS idx_publication_batch ON publication_ledger(batch_id);",
    )
}

/// 일반 되돌리기(move·rename·trash·restore)의 동일성 대조용 — 옮긴 직후 목적지의
/// 크기·mtime. 이전 저널은 NULL 로 남아 대조 없이 되돌린다 (2차 리뷰 M-3).
fn add_journal_file_stat(c: &Connection) -> rusqlite::Result<()> {
    for (column, ddl) in [
        ("to_size", "ALTER TABLE journal ADD COLUMN to_size INTEGER"),
        (
            "to_mtime",
            "ALTER TABLE journal ADD COLUMN to_mtime INTEGER",
        ),
    ] {
        if !has_column(c, "journal", column)? {
            c.execute_batch(ddl)?;
        }
    }
    Ok(())
}

/// Gallery→Desk P1 폴더명 감사의 부모→자식 배치 연결. 신규·기존 DB 모두 멱등이다.
fn add_gallery_transition_p1(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS folder_audit_children (
            parent_batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
            child_batch_id INTEGER NOT NULL UNIQUE REFERENCES batches(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            PRIMARY KEY (parent_batch_id, child_batch_id)
         );",
    )
}

/// Gallery→Desk P0 작업용 테이블. CREATE IF NOT EXISTS라 구버전·신규 DB 모두 멱등이다.
fn add_gallery_transition_p0(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS capture_date_journal (
            batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            backup_vol TEXT, backup_path TEXT,
            old_atime_sec INTEGER NOT NULL, old_atime_nsec INTEGER NOT NULL,
            old_mtime_sec INTEGER NOT NULL, old_mtime_nsec INTEGER NOT NULL,
            old_taken_at INTEGER NOT NULL, old_source INTEGER NOT NULL,
            old_override INTEGER, new_taken_at INTEGER NOT NULL,
            write_scope TEXT NOT NULL,
            PRIMARY KEY (batch_id, file_id)
         );
         CREATE TABLE IF NOT EXISTS capture_date_overrides (
            file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
            taken_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
         );
         CREATE TABLE IF NOT EXISTS publication_ledger (
            id INTEGER PRIMARY KEY,
            source_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
            source_sha256 TEXT NOT NULL,
            destination_library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
            destination_path TEXT NOT NULL,
            destination_sha256 TEXT NOT NULL,
            batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            UNIQUE(source_sha256,destination_library_id,destination_path)
         );
         CREATE INDEX IF NOT EXISTS idx_publication_hash ON publication_ledger(source_sha256,destination_library_id);
         CREATE INDEX IF NOT EXISTS idx_publication_batch ON publication_ledger(batch_id);
         CREATE TABLE IF NOT EXISTS folder_journal (
            batch_id INTEGER PRIMARY KEY REFERENCES batches(id) ON DELETE CASCADE,
            op TEXT NOT NULL,
            source_library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
            source_path TEXT NOT NULL,
            destination_library_id INTEGER REFERENCES libraries(id) ON DELETE CASCADE,
            destination_path TEXT,
            file_count INTEGER NOT NULL DEFAULT 0,
            dir_count INTEGER NOT NULL DEFAULT 0,
            bytes INTEGER NOT NULL DEFAULT 0,
            manifest_sha256 TEXT NOT NULL,
            cross_volume INTEGER NOT NULL DEFAULT 0
         );",
    )
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
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
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
                    .or_else(|| {
                        folder
                            .rsplit('/')
                            .next()
                            .and_then(crate::media::taken_at::from_filename)
                    })
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

/// 크기만 줄인 사본을 찾는 지각 해시(2026-09-01). 64비트를 i64 로 담는다 —
/// SQLite 정수가 부호 있는 64비트라 u64 를 그대로는 못 넣는다. 읽을 때 되돌린다.
/// 색인은 두지 않는다 — 같은 값 찾기가 아니라 전량을 메모리로 올려 견주기 때문이다.
fn add_phash(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "files", "phash")? {
        c.execute_batch("ALTER TABLE files ADD COLUMN phash INTEGER")?;
    }
    // 버전+16×16 밝기+8×8 색차 — 해시가 이은 짝이 정말 같은 그림인지 견준다
    if !has_column(c, "files", "psig")? {
        c.execute_batch("ALTER TABLE files ADD COLUMN psig BLOB")?;
    }
    Ok(())
}

/// 라이브러리별 «살아 있는 사진 수»를 세는 부분 인덱스(2026-09-01).
///
/// `idx_files_folder` 는 `trashed_at` 을 담지 않아, 세려면 14.6만 행을 하나씩 다시
/// 읽어야 했다. 그 한 질의가 **3.16초** — 첫 화면 2.5초의 거의 전부였다.
/// 이 인덱스로 0.005초가 된다. 구버전 DB 에도 만들어 준다.
fn add_live_count_index(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_files_folder_live ON files(folder_id) WHERE trashed_at IS NULL;",
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
    // 출처·정밀도 (2026-09-01) — 오프라인 지명이 들어오면 «어디서 온 값인지»로
    // 덮어쓰기를 판단해야 한다. status 하나에 출처를 섞지 않는다
    for (col, decl) in [
        ("source", "TEXT NOT NULL DEFAULT 'legacy'"),
        ("precision", "TEXT"),
        ("distance_km", "REAL"),
        ("dataset_version", "TEXT"),
        ("provider", "TEXT"),
        ("resolved_at", "INTEGER"),
        // 온라인 조회 이력 (2026-09-01) — 값의 출처와 다른 축이다. 서버가 못
        // 찾았거나 얕게 답했을 때 값은 그대로 두고 «물어봤다»만 남겨야, 같은
        // 좌표를 같은 서버에 되풀이해 묻지 않는다
        ("online_outcome", "TEXT"),
        ("online_provider", "TEXT"),
        ("online_checked_at", "INTEGER"),
    ] {
        if !has_column(c, "places", col)? {
            c.execute_batch(&format!("ALTER TABLE places ADD COLUMN {col} {decl}"))?;
        }
    }
    // 옛 판의 «이름 없음»은 온라인이 그렇게 답한 것이다. 조회 이력 칸이 생기기
    // 전에 만들어졌으므로 여기서 채워 준다 — 비워 두면 «아직 아무한테도 안
    // 물어봤다»로 읽혀 대상에 다시 들어간다. 어느 서버였는지는 알 수 없으니
    // online_provider 는 비워 둔다: 서버를 설정하면 딱 한 번 다시 물어보고,
    // 그때 서버 이름이 기록돼 그다음부터는 조용해진다.
    c.execute_batch(
        "UPDATE places
            SET online_outcome = 'none',
                online_provider = provider,
                online_checked_at = COALESCE(resolved_at, at)
          WHERE status = 'none' AND online_outcome IS NULL;",
    )?;
    // 기존 캐시는 모두 온라인에서 온 것이다 — 오프라인 경로가 없던 시절의 값이다
    c.execute_batch(
        "UPDATE places SET source='nominatim', precision='remote',
                resolved_at=COALESCE(resolved_at, at)
          WHERE source='legacy' AND status='ok'
            AND country IS NOT NULL AND trim(country) <> '';
         UPDATE places SET source='nominatim', resolved_at=COALESCE(resolved_at, at)
          WHERE source='legacy' AND status='none';
         CREATE INDEX IF NOT EXISTS idx_places_status ON places(status, source);
         CREATE INDEX IF NOT EXISTS idx_places_online ON places(online_outcome, online_provider);",
    )?;
    // 첫 지명 구현은 «결과 없음»도 세 이름이 모두 NULL인 캐시 행으로 남겼다.
    // status를 단순 DEFAULT 'ok'로 더하면 그 행은 성공 캐시가 되어 다시 묻지도,
    // 파일을 완료시키지도 못한다. 이미 그 중간 빌드를 열어 status가 생긴 DB도
    // 복구해야 하므로 컬럼 추가 여부와 무관하게 매번 멱등으로 보정한다.
    //
    // **`status='ok'` 인 행만 고친다.** 이 보정은 앱을 열 때마다 도는데, 조건을
    // «이름이 비었으면»으로 잡으면 오프라인 판정이 남긴 `unresolved`(이름이 비어
    // 있는 것이 정상이다)까지 `none` 으로 바꿔 버린다. 그러면 그 자리는 다시
    // 물어볼 수 없는 곳으로 굳어 영영 이름을 얻지 못한다 (2026-09-01 외부 검토).
    c.execute_batch(
        "UPDATE places
            SET status = 'none'
          WHERE status = 'ok' AND (country IS NULL OR trim(country) = '');",
    )?;
    Ok(())
}

fn add_faces_at(c: &Connection) -> rusqlite::Result<()> {
    if !has_column(c, "files", "faces_at")? {
        c.execute_batch("ALTER TABLE files ADD COLUMN faces_at INTEGER")?;
    }
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_files_faces_at ON files(faces_at) WHERE faces_at IS NULL;",
    )
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
        let mut st =
            c.prepare("SELECT DISTINCT volume_uuid FROM folders WHERE library_id IS NULL")?;
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
                 DROP INDEX IF EXISTS idx_files_folder_live;
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
            // 라이브러리 장수를 세는 부분 인덱스도 되살아나야 한다 — 없으면 첫 화면이
            // 다시 3초로 돌아간다 (실측 2026-09-01)
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_files_folder_live'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 1, "구버전 DB 를 올린 뒤 idx_files_folder_live 가 없다");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn release_091_integrity_upgrade_is_idempotent_on_a_090_database() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("schema.sql")).unwrap();
        c.execute_batch(
            "ALTER TABLE capture_date_journal DROP COLUMN before_sha256;
             ALTER TABLE capture_date_journal DROP COLUMN after_sha256;
             ALTER TABLE capture_date_journal DROP COLUMN undone_at;
             DROP TABLE copy_manifest;",
        )
        .unwrap();

        add_release_091_integrity(&c).unwrap();
        add_release_091_integrity(&c).unwrap();
        for column in ["before_sha256", "after_sha256", "undone_at"] {
            assert!(has_column(&c, "capture_date_journal", column).unwrap());
        }
        let table: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='copy_manifest'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, 1);
        let index: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_publication_batch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
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
        db.write(|c| c.execute("UPDATE files SET image_hash='abc'", []))
            .unwrap();
    }

    #[test]
    fn done_at_column_is_added_to_an_old_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch("ALTER TABLE groups DROP COLUMN done_at;")
                .unwrap();
        }
        let db = Db::open(&path).expect("done_at 전 DB도 열려야 한다");
        db.write(|c| c.execute("UPDATE groups SET done_at = 1", []))
            .unwrap();
    }

    /// 업그레이드가 나중에 더하는 칸을 schema.sql 이 먼저 참조하면 구버전 DB 가 안 열린다.
    /// v0.5.4 DB 에서 실제로 «no such column: geo_country» 로 죽었다 (2026-09-01).
    /// 앱을 열 때마다 도는 보정이 «다시 물어볼 자리»를 «없는 자리»로 굳히면 안 된다.
    ///
    /// 오프라인 판정이 못 정한 자리는 이름이 비어 있는 것이 정상이다. 그것을
    /// 지우면 온라인 보강 대상에서 영영 빠진다 (2026-09-01 외부 검토).
    #[test]
    fn the_repair_never_settles_a_cell_that_is_still_waiting_for_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(&path).unwrap();
            db.write(|c| {
                c.execute_batch(
                    "INSERT INTO places(cell,status,source,precision,at)
                       VALUES('1,1','unresolved','offline_geonames','approximate',0),
                             ('2,2','ok','offline_geonames','approximate',0),
                             ('3,3','ok','nominatim','remote',0);
                     UPDATE places SET country='대한민국', name='대한민국'
                      WHERE cell IN ('2,2','3,3');
                     -- 값이 비었는데 성공이라고 적힌 모순 행 — 이것만 고쳐야 한다
                     INSERT INTO places(cell,status,source,precision,at)
                       VALUES('4,4','ok','nominatim','remote',0);",
                )
            })
            .unwrap();
        }
        // 다시 열어 보정을 한 번 더 돌린다 (실제로 앱을 껐다 켜는 것과 같다)
        let db = Db::open(&path).unwrap();
        let status = |cell: &str| -> String {
            db.read(|c| {
                c.query_row("SELECT status FROM places WHERE cell=?1", [cell], |r| {
                    r.get(0)
                })
            })
            .unwrap()
        };
        assert_eq!(
            status("1,1"),
            "unresolved",
            "아직 물어볼 자리를 굳히면 안 된다"
        );
        assert_eq!(status("2,2"), "ok");
        assert_eq!(status("3,3"), "ok");
        assert_eq!(
            status("4,4"),
            "none",
            "값이 비었는데 성공이라고 적힌 행은 고친다"
        );
    }

    /// 갓 만든 DB 와 옛 DB 를 올린 것이 **같은 모양**이어야 한다.
    ///
    /// 칸이나 인덱스를 한쪽에만 더하면 조용히 갈라진다 — 새로 설치한 사람만
    /// 인덱스가 없어 느리거나, 옛 사용자만 칸이 없어 질의가 깨진다.
    ///
    /// 만들어진 SQL 글월을 그대로 견주지는 않는다. `ALTER TABLE ADD COLUMN` 은
    /// 칸을 늘 끝에 붙이므로 순서와 주석이 달라진다 — 그것은 차이가 아니다.
    /// 이름의 집합만 본다.
    #[test]
    fn a_fresh_database_and_an_upgraded_one_end_up_identical() {
        let dir = tempfile::tempdir().unwrap();

        /// 표·인덱스 이름과 각 표의 칸 이름 — 순서에 흔들리지 않게 모두 정렬한다
        fn shape(db: &Db) -> Vec<String> {
            db.read(|c| {
                let mut names: Vec<(String, String)> = {
                    let mut st = c.prepare(
                        "SELECT type, name FROM sqlite_master
                          WHERE name NOT LIKE 'sqlite_%' AND type IN ('table','index','view','trigger')",
                    )?;
                    let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                    it.collect::<rusqlite::Result<Vec<_>>>()?
                };
                names.sort();
                let mut out = Vec::new();
                for (kind, name) in names {
                    if kind == "table" {
                        let mut st = c.prepare(&format!("PRAGMA table_info({name})"))?;
                        let it = st.query_map([], |r| r.get::<_, String>(1))?;
                        let mut cols = it.collect::<rusqlite::Result<Vec<_>>>()?;
                        cols.sort();
                        out.push(format!("table {name}({})", cols.join(",")));
                    } else {
                        out.push(format!("{kind} {name}"));
                    }
                }
                Ok(out)
            })
            .unwrap()
        }

        let fresh = Db::open(dir.path().join("fresh.db")).unwrap();
        let want = shape(&fresh);

        // 0.5.4 판의 모양으로 되돌린 DB 를 올린다. geo_name 은 v2 첫 스키마부터
        // 있었으므로 남긴다 — 실제로 존재했던 판을 흉내 내야 뜻이 있다.
        let old_path = dir.path().join("old.db");
        {
            let c = Connection::open(&old_path).unwrap();
            let create: Vec<String> = fresh
                .read(|f| {
                    let mut st = f.prepare(
                        "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'",
                    )?;
                    let it = st.query_map([], |r| r.get::<_, String>(0))?;
                    it.collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap();
            for sql in &create {
                c.execute_batch(&format!("{sql};")).unwrap();
            }
            c.execute_batch(
                "DROP INDEX IF EXISTS idx_files_geo;
                 DROP INDEX IF EXISTS idx_places_status;
                 DROP TABLE IF EXISTS places;
                 ALTER TABLE files DROP COLUMN geo_country;
                 ALTER TABLE files DROP COLUMN geo_admin1;
                 ALTER TABLE files DROP COLUMN geo_admin2;",
            )
            .unwrap();
        }
        let got = shape(&Db::open(&old_path).unwrap());

        let missing: Vec<_> = want.iter().filter(|x| !got.contains(x)).collect();
        let extra: Vec<_> = got.iter().filter(|x| !want.contains(x)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "갓 만든 DB 와 올린 DB 의 모양이 다릅니다\n올린 쪽에 없는 것: {missing:#?}\n올린 쪽에만 있는 것: {extra:#?}"
        );

        // 이 시험이 실제로 무언가를 지키는지 — 지명 인덱스가 양쪽에 다 있어야 한다
        assert!(
            want.contains(&"index idx_files_geo".to_string()),
            "새 DB 에 지명 인덱스가 없습니다"
        );
        assert!(
            want.iter().any(|x| x.starts_with("table places(")),
            "새 DB 에 places 표가 없습니다"
        );
    }

    /// 새 칸을 넣을 때마다 이 목록에 더한다 — 사람이 기억하지 않아도 시험이 잡게.
    #[test]
    fn schema_never_mentions_a_column_that_upgrade_adds_later() {
        let schema = include_str!("schema.sql");
        for col in [
            "trashed_at",
            "trash_path",
            "trash_batch",
            "faces_at",
            "image_hash",
            "done_at",
            "geo_country",
            "geo_admin1",
            "geo_admin2",
        ] {
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
                let rows = st
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            statuses,
            vec![
                ("10.00,20.00".into(), "none".into()),
                ("37.28,127.05".into(), "ok".into())
            ]
        );

        // 중간 수정 빌드가 이미 status='ok'를 붙인 DB도 다음 실행에서 복구한다.
        db.write(|c| c.execute("UPDATE places SET status='ok' WHERE cell='10.00,20.00'", []))
            .unwrap();
        drop(db);
        let reopened = Db::open(&path).unwrap();
        let repaired: String = reopened
            .read(|c| {
                c.query_row(
                    "SELECT status FROM places WHERE cell='10.00,20.00'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(repaired, "none");
    }

    /// 출처·정밀도 칸이 없던 DB 도 열리고, 기존 성공 캐시는 온라인 결과로 표시된다
    #[test]
    fn place_metadata_columns_are_added_and_existing_cache_is_labelled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("schema.sql")).unwrap();
            c.execute_batch(
                "DROP TABLE places;
                 CREATE TABLE places (
                   cell TEXT PRIMARY KEY, country TEXT, admin1 TEXT, admin2 TEXT, name TEXT,
                   status TEXT NOT NULL DEFAULT 'ok', at INTEGER NOT NULL
                 );
                 INSERT INTO places(cell,country,admin1,admin2,name,status,at)
                   VALUES('37.28,127.05','대한민국','경기도','수원시','수원시','ok',111),
                         ('10.00,20.00',NULL,NULL,NULL,NULL,'none',222);",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("출처 칸이 없던 DB 도 열려야 한다");
        let rows: Vec<(String, String, String, Option<String>, i64)> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT cell, status, source, precision, resolved_at FROM places ORDER BY cell",
                )?;
                let out = st
                    .query_map([], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>();
                out
            })
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "10.00,20.00".into(),
                    "none".into(),
                    "nominatim".into(),
                    None,
                    222
                ),
                (
                    "37.28,127.05".into(),
                    "ok".into(),
                    "nominatim".into(),
                    Some("remote".into()),
                    111
                ),
            ],
            "기존 캐시는 값이 그대로이고 출처만 붙는다"
        );
        // 두 번 열어도 그대로 (멱등)
        drop(db);
        let again = Db::open(&path).unwrap();
        let n: i64 = again
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM places WHERE source='nominatim'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(n, 2);
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
        let old = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(18, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        db.write(|c| {
            c.execute_batch("DELETE FROM settings WHERE key='internal.taken_at_utc_v1';")?;
            c.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','v','library')",
                [],
            )?;
            c.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','p','p',1)",
                [],
            )?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,1,'photo.jpg',1,0,?1,0,0)",
                [old],
            )?;
            migrate_taken_at_to_utc(c)
        })
        .unwrap();

        let migrated: i64 = db
            .read(|c| c.query_row("SELECT taken_at FROM files", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(
            migrated,
            crate::media::taken_at::civil_to_unix(2024, 1, 1, 18, 0, 0)
        );
        db.write(migrate_taken_at_to_utc).unwrap();
        let again: i64 = db
            .read(|c| c.query_row("SELECT taken_at FROM files", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(
            again, migrated,
            "두 번 열어도 다시 시간대를 적용하면 안 된다"
        );
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
