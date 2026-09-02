//! 파일을 실제로 움직이는 일들.
//!
//! 조회·판정과 달리 여기서는 되돌릴 수 없는 일이 일어난다. 그래서 규칙이 있다:
//!   - 모든 이동은 `batches`/`journal`에 남긴다. 안 남기면 되돌릴 수 없다.
//!   - 지우기는 휴지통 경유. 진짜 삭제는 사용자가 따로 확인해야 한다.
//!   - 지울 경로는 정규화한 뒤 다시 한 번 범위를 확인한다.
//!
//! 저널의 경로는 **언제나 볼륨 기준**이다. 되돌릴 때 마운트만 앞에 붙이면 된다.

pub mod husk;
pub mod capture_date;
pub mod import;
pub mod merge;
pub mod naming;
pub mod offload;
pub mod organize;
pub mod xmp;
pub mod rename;
pub mod trash;
pub mod transfer;
pub mod undo;

use crate::db::conn::{Db, Result};

/// 작업 묶음을 연다. 되돌리기는 이 단위로 한다.
pub fn open_batch(db: &Db, kind: &str, label: &str) -> Result<i64> {
    db.write(|c| {
        c.execute(
            "INSERT INTO batches(kind,label,created_at) VALUES(?1,?2,strftime('%s','now'))",
            rusqlite::params![kind, label],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn close_batch(db: &Db, batch_id: i64, done: usize) -> Result<()> {
    db.write(|c| {
        c.execute(
            "UPDATE batches SET item_count = ?2 WHERE id = ?1",
            rusqlite::params![batch_id, done as i64],
        )
    })?;
    Ok(())
}

/// 한 파일에 일어난 일을 저널에 남긴다. 경로는 볼륨 기준.
#[allow(clippy::too_many_arguments)]
pub fn record(
    db: &Db,
    batch_id: i64,
    op: &str,
    file_id: i64,
    volume_uuid: &str,
    from_path: &str,
    to_path: Option<&str>,
    r: std::result::Result<(), &str>,
) -> Result<()> {
    record_to(db, batch_id, op, file_id, volume_uuid, from_path, volume_uuid, to_path, r)
}

/// 볼륨을 넘어간 이동 — 도착 볼륨을 따로 적는다. 예전엔 `to_vol`을 `from_vol`과 같은
/// 자리(?4)에 묶어 T7→NAS 이동을 되돌릴 때 원래 디스크에서 사본을 찾다 실패했다 (리뷰 C1)
#[allow(clippy::too_many_arguments)]
pub fn record_to(
    db: &Db,
    batch_id: i64,
    op: &str,
    file_id: i64,
    from_vol: &str,
    from_path: &str,
    to_vol: &str,
    to_path: Option<&str>,
    r: std::result::Result<(), &str>,
) -> Result<()> {
    db.write(|c| {
        c.execute(
            "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,error)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                batch_id,
                file_id,
                op,
                from_vol,
                from_path,
                to_vol,
                to_path,
                r.is_ok() as i32,
                r.err(),
            ],
        )
    })?;
    Ok(())
}
