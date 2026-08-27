use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

use super::migrations;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(app_data_dir: &PathBuf) -> Result<Self, rusqlite::Error> {
        std::fs::create_dir_all(app_data_dir).ok();
        let db_path = app_data_dir.join("smart_category.db");
        let conn = Connection::open(db_path)?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        migrations::run(&conn)
    }

    pub fn with_conn<F, T>(&self, f: F) -> Result<T, rusqlite::Error>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }
}
