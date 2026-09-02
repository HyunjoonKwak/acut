//! 데이터베이스 연결 — 읽기와 쓰기를 분리한다.
//!
//! 왜 분리하는가: SQLite의 WAL 모드는 쓰기 한 건과 읽기 여러 건이 동시에
//! 진행되는 것을 허용한다. 그런데 연결이 하나뿐이면 그 이점을 쓸 수 없다.
//! 스캔이 도는 동안 화면이 멈추던 원인이 여기 있었다.
//!
//! 구조
//!   - 쓰기 연결 1개 (Mutex). SQLite는 어차피 동시 쓰기를 허용하지 않는다.
//!   - 읽기 연결 N개. 라운드로빈으로 나눠 쓴다.
//!
//! PRAGMA 주의: `foreign_keys`는 **연결마다** 켜야 한다. 스키마 파일에 써도
//! 새 연결에는 적용되지 않아, ON DELETE CASCADE가 조용히 동작하지 않는다.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 읽기 연결 개수. 화면 하나가 그리드·인스펙터·사이드바를 동시에 조회하는 것을
/// 감안한 값이다. 늘려도 SQLite 자체는 견디지만 파일 핸들이 늘어난다.
const READ_POOL_SIZE: usize = 4;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite 오류: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("데이터베이스 폴더를 만들 수 없습니다: {0}")]
    CreateDir(#[from] std::io::Error),
    /// 값이 규칙에 안 맞아 DB까지 갈 것도 없는 경우 (빈 이름 따위).
    /// 그대로 사용자에게 보여 줄 문장이어야 한다.
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Db {
    path: PathBuf,
    write: Mutex<Connection>,
    read: Vec<Mutex<Connection>>,
    next_read: AtomicUsize,
}

impl Db {
    /// 데이터베이스를 열고 스키마를 적용한다. 이미 있으면 그대로 쓴다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        // 쓰기 연결이 먼저 열려야 스키마가 만들어진다.
        let write = Connection::open(&path)?;
        configure(&write, false)?;
        write.execute_batch(SCHEMA)?;
        // 스키마로는 못 하는 변경 (컬럼 추가 등)
        super::upgrade::run(&write)?;

        let mut read = Vec::with_capacity(READ_POOL_SIZE);
        for _ in 0..READ_POOL_SIZE {
            let c = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure(&c, true)?;
            read.push(Mutex::new(c));
        }

        Ok(Self {
            path,
            write: Mutex::new(write),
            read,
            next_read: AtomicUsize::new(0),
        })
    }

    /// 읽기 전용 작업. 여러 개가 동시에 진행될 수 있다.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let i = self.next_read.fetch_add(1, Ordering::Relaxed) % self.read.len();
        // 다른 스레드가 같은 연결을 쓰고 있으면 다음 것을 시도한다.
        for k in 0..self.read.len() {
            let idx = (i + k) % self.read.len();
            if let Ok(c) = self.read[idx].try_lock() {
                return Ok(f(&c)?);
            }
        }
        // 전부 사용 중이면 순서를 기다린다.
        let c = self.read[i].lock().unwrap_or_else(|e| e.into_inner());
        Ok(f(&c)?)
    }

    /// 쓰기 작업. 한 번에 하나만 진행된다.
    pub fn write<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let c = self.write.lock().unwrap_or_else(|e| e.into_inner());
        Ok(f(&c)?)
    }

    /// 트랜잭션으로 묶어야 하는 쓰기. 대량 삽입은 반드시 이쪽을 쓴다.
    /// 낱개 INSERT는 매번 fsync가 걸려 수십 배 느리다.
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction) -> rusqlite::Result<T>,
    ) -> Result<T> {
        let mut c = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let tx = c.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 사본의 내용으로 **이 DB를 통째로 바꾼다.** 되돌릴 수 없다 — 부르는 쪽이
    /// 먼저 지금 상태를 한 벌 떠 둬야 한다.
    ///
    /// 파일을 바꿔치기하지 않는다. 열린 연결이 다섯이라 파일을 갈아 끼우면
    /// 옛 파일을 계속 보는 연결이 생긴다. SQLite의 online backup API로 쓰기
    /// 연결에 페이지를 부어 넣는다 — 다른 연결은 다음 읽기부터 새 내용을 본다.
    pub fn restore_from(&self, src: &Path) -> Result<()> {
        let from = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut c = self.write.lock().unwrap_or_else(|e| e.into_inner());
        {
            let b = rusqlite::backup::Backup::new(&from, &mut c)?;
            b.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
        }
        // 옛 사본에는 그 뒤 생긴 열·표가 없다 — 열 때와 똑같이 맞춘다. 안 하면 «no such
        // column»으로 앱을 다시 켤 때까지 죽는다 (리뷰 H15)
        c.execute_batch(SCHEMA)?;
        super::upgrade::run(&c)?;
        Ok(())
    }
}

/// 연결마다 적용해야 하는 설정.
fn configure(c: &Connection, read_only: bool) -> rusqlite::Result<()> {
    // 연결 스코프 — 반드시 매번 설정해야 한다.
    c.execute_batch("PRAGMA foreign_keys = ON;")?;
    // 잠금 대기. 스캔 중 UI 조회가 곧바로 실패하지 않도록.
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    // 페이지 캐시 (음수 = KiB). 6만 행 인덱스가 메모리에 들어가는 크기.
    c.execute_batch("PRAGMA cache_size = -64000;")?;
    c.execute_batch("PRAGMA temp_store = MEMORY;")?;
    // 읽기는 mmap이 유리하다. 쓰기 연결은 기본값을 쓴다.
    if read_only {
        c.execute_batch("PRAGMA mmap_size = 268435456;")?; // 256 MiB
    } else {
        // DB 파일 속성 — 한 번만 설정되면 유지되지만 매번 확인해도 무해하다.
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.execute_batch("PRAGMA synchronous = NORMAL;")?;
        // 체크포인트가 성공할 때 WAL 파일을 이 크기까지 되감는다 — 대량 해시 뒤
        // 645MB 로 남던 것(실측 2026-08-31). 긴 읽기가 물고 있으면 못 줄이므로
        // 유휴 때 checkpoint_truncate 가 마저 자른다.
        c.execute_batch("PRAGMA journal_size_limit = 67108864;")?; // 64 MiB
    }
    Ok(())
}

impl Db {
    /// WAL 을 본체에 옮겨 적고 파일을 0 으로 자른다. 성공하면 true, 읽기가 물고
    /// 있어 못 잘랐으면 false — 다음 유휴 때 다시 하면 된다. 쓰기 연결을 잠그므로
    /// 유휴(작업 스위치가 빈) 때만 부를 것.
    pub fn checkpoint_truncate(&self) -> Result<bool> {
        self.write(|c| {
            let (busy, _log, _ckpt): (i64, i64, i64) =
                c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
            Ok(busy == 0)
        })
    }

    /// WAL 파일의 현재 크기 — 유지보수 스레드가 «자를 만큼 컸나»를 본다.
    pub fn wal_size(&self) -> u64 {
        let mut p = self.path.clone().into_os_string();
        p.push("-wal");
        std::fs::metadata(std::path::PathBuf::from(p))
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Db::open(dir.path().join("test.db")).expect("open");
        (dir, db)
    }

    #[test]
    fn wal_is_truncated_when_idle_and_size_limit_is_set() {
        let (_d, db) = temp_db();
        let limit: i64 = db
            .write(|c| c.query_row("PRAGMA journal_size_limit", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(limit, 67_108_864, "쓰기 연결에 WAL 상한이 걸려 있어야 한다");

        // 대량 쓰기로 WAL 을 키운다
        db.transaction(|tx| {
            tx.execute("CREATE TABLE IF NOT EXISTS blob_t(x BLOB)", [])?;
            let big = vec![7u8; 1024 * 1024];
            for _ in 0..8 {
                tx.execute("INSERT INTO blob_t(x) VALUES(?1)", [&big])?;
            }
            Ok(())
        })
        .unwrap();
        assert!(
            db.wal_size() > 1024 * 1024,
            "WAL 이 자랐어야 한다: {}",
            db.wal_size()
        );

        assert!(
            db.checkpoint_truncate().unwrap(),
            "읽기가 없으니 잘려야 한다"
        );
        assert_eq!(db.wal_size(), 0, "TRUNCATE 뒤 WAL 은 0 바이트");

        // 자른 뒤에도 읽기·쓰기가 멀쩡하다
        let n: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM blob_t", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 8);
        db.write(|c| c.execute("INSERT INTO blob_t(x) VALUES(x'00')", []))
            .unwrap();
    }

    #[test]
    fn schema_applies_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.db");
        let _ = Db::open(&p).expect("first open");
        // 두 번째 열기에서도 CREATE TABLE IF NOT EXISTS가 안전해야 한다.
        let db = Db::open(&p).expect("second open");
        let n: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(
            n, 24,
            "테이블 24개가 만들어져야 한다 (schema.sql 23 + upgrade의 nas_pulls)"
        );
    }

    /// 실제 DB 사본을 여는 데 얼마나 걸리나 — 시작 시간의 첫 구간.
    /// `ACUT_DB_COPY=… cargo test --release --lib db::conn::tests::real_open -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 DB 사본 필요"]
    fn real_open_time() {
        let Ok(p) = std::env::var("ACUT_DB_COPY") else {
            return;
        };
        for i in 0..3 {
            let t = std::time::Instant::now();
            let db = Db::open(&p).unwrap();
            let open = t.elapsed();
            let t2 = std::time::Instant::now();
            let n: i64 = db
                .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
                .unwrap();
            eprintln!(
                "열기 {i}: {:.0}ms · COUNT(files)={n} {:.0}ms",
                open.as_secs_f64() * 1000.0,
                t2.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    #[test]
    fn foreign_keys_are_on_for_every_connection() {
        let (_d, db) = temp_db();
        // 쓰기 연결
        let w: i64 = db
            .write(|c| c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(w, 1, "쓰기 연결에서 켜져 있어야 한다");
        // 읽기 연결 — 풀 전체를 확인한다
        for _ in 0..READ_POOL_SIZE * 2 {
            let r: i64 = db
                .read(|c| c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)))
                .unwrap();
            assert_eq!(r, 1, "읽기 연결에서도 켜져 있어야 한다");
        }
    }

    #[test]
    fn cascade_delete_actually_works() {
        let (_d, db) = temp_db();
        db.write(|c| {
            c.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
                [],
            )?;
            c.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,1,'x.jpg',10,0,100,0,0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        db.write(|c| c.execute("DELETE FROM folders WHERE id=1", []))
            .unwrap();

        let left: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(left, 0, "폴더를 지우면 파일도 함께 지워져야 한다");
    }

    #[test]
    fn reads_run_while_a_write_is_held() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path().join("t.db")).unwrap());
        db.write(|c| {
            c.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
                [],
            )
        })
        .unwrap();

        // 쓰기 잠금을 쥔 채로 읽기가 진행되는지 본다.
        let held = db.write.lock().unwrap();
        let d2 = Arc::clone(&db);
        let h = std::thread::spawn(move || {
            d2.read(|c| c.query_row("SELECT COUNT(*) FROM volumes", [], |r| r.get::<_, i64>(0)))
        });
        let got = h.join().expect("읽기 스레드가 끝나야 한다").unwrap();
        assert_eq!(got, 1);
        drop(held);
    }

    #[test]
    fn taken_at_ordering_uses_the_index() {
        let (_d, db) = temp_db();
        let plan: String = db
            .read(|c| {
                c.query_row(
                    "EXPLAIN QUERY PLAN SELECT id FROM files ORDER BY taken_at DESC LIMIT 200",
                    [],
                    |r| r.get(3),
                )
            })
            .unwrap();
        assert!(
            plan.contains("idx_files_taken"),
            "정렬이 인덱스를 타야 한다. 실제 플랜: {plan}"
        );
    }
}
