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
    }
    Ok(())
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
        assert_eq!(n, 14, "테이블 14개가 만들어져야 한다");
    }

    #[test]
    fn foreign_keys_are_on_for_every_connection() {
        let (_d, db) = temp_db();
        // 쓰기 연결
        let w: i64 = db.write(|c| c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))).unwrap();
        assert_eq!(w, 1, "쓰기 연결에서 켜져 있어야 한다");
        // 읽기 연결 — 풀 전체를 확인한다
        for _ in 0..READ_POOL_SIZE * 2 {
            let r: i64 = db.read(|c| c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))).unwrap();
            assert_eq!(r, 1, "읽기 연결에서도 켜져 있어야 한다");
        }
    }

    #[test]
    fn cascade_delete_actually_works() {
        let (_d, db) = temp_db();
        db.write(|c| {
            c.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
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
            c.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])
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
