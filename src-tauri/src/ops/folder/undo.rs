use super::execute::{
    copy_tree_verified, delete_copied_db, move_db_rows, refresh_counts, rows_in_subtree,
    temp_sibling,
};
use super::*;

#[derive(Debug)]
struct JournalRow {
    op: String,
    source_library_id: i64,
    source_path: String,
    destination_library_id: Option<i64>,
    destination_path: Option<String>,
    /// 내용 다이제스트. 같은 볼륨 작업(0.9.2+)은 빈 문자열로 남기고 `stat_sha256` 로 대조한다.
    manifest_sha256: String,
    /// 이름·크기·mtime 다이제스트. 0.9.1 이전 저널은 NULL.
    stat_sha256: Option<String>,
    files: usize,
    dirs: usize,
    bytes: u64,
}

fn journal(db: &Db, batch: i64) -> Result<JournalRow> {
    db.read(|c|c.query_row("SELECT op,source_library_id,source_path,destination_library_id,destination_path,manifest_sha256,file_count,dir_count,bytes,stat_sha256 FROM folder_journal WHERE batch_id=?1",[batch],|r|Ok(JournalRow{op:r.get(0)?,source_library_id:r.get(1)?,source_path:r.get(2)?,destination_library_id:r.get(3)?,destination_path:r.get(4)?,manifest_sha256:r.get(5)?,files:r.get::<_,i64>(6)? as usize,dirs:r.get::<_,i64>(7)? as usize,bytes:r.get::<_,i64>(8)? as u64,stat_sha256:r.get(9)?})))
}

pub fn undo(db: &Db, batch: i64) -> Result<Outcome> {
    let row = journal(db, batch)?;
    let source_lib = library(db, row.source_library_id)?;
    let source_root = online_root(&source_lib)?;
    let destination_lib = library(
        db,
        row.destination_library_id.unwrap_or(row.source_library_id),
    )?;
    let destination_rel = row
        .destination_path
        .clone()
        .ok_or_else(|| bad("되돌릴 목적지 기록이 없습니다"))?;
    let destination = online_root(&destination_lib)?.join(&destination_rel);
    let mut out = Outcome {
        batch_id: batch,
        ..Default::default()
    };
    let fail = |out: &mut Outcome, message: String| {
        out.failed = 1;
        out.first_error = Some(message);
    };
    if row.op == "create" {
        if std::fs::read_dir(&destination)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
        {
            fail(
                &mut out,
                "폴더 안에 새 항목이 있어 생성 작업을 되돌리지 않았습니다".into(),
            );
            return Ok(out);
        }
        let staged = free_path(temp_sibling(&destination, batch));
        std::fs::rename(&destination, &staged)
            .io_context("생성한 폴더를 임시 위치로 옮기다가 실패했습니다")?;
        if let Err(error) = delete_copied_db(db, &destination_lib, &destination_rel) {
            let _ = std::fs::rename(&staged, &destination);
            return Err(error);
        }
        if let Err(error) = remove_tree(&staged) {
            log::warn!(
                "폴더 생성 undo 임시 갈래 정리 보류 {}: {error}",
                staged.display()
            );
        }
    } else {
        if !destination.is_dir() {
            fail(&mut out, "되돌릴 폴더가 디스크에 없습니다".into());
            return Ok(out);
        }
        // 대조 기준: 내용 해시가 기록돼 있으면(복사·볼륨 간 이동·0.9.1 저널) 그것으로,
        // 없으면 같은 볼륨 작업이 남긴 이름·크기·mtime 다이제스트로 본다.
        let now = if row.manifest_sha256.is_empty() {
            let summary = tree_summary(&destination)?;
            if row.stat_sha256.as_deref() != Some(summary.stat_sha256.as_str()) {
                fail(
                    &mut out,
                    "작업 뒤 폴더 내용이 바뀌어 안전하게 되돌릴 수 없습니다".into(),
                );
                return Ok(out);
            }
            summary
        } else {
            let full = manifest(&destination)?;
            if full.sha256 != row.manifest_sha256 {
                fail(
                    &mut out,
                    "작업 뒤 폴더 내용이 바뀌어 안전하게 되돌릴 수 없습니다".into(),
                );
                return Ok(out);
            }
            full
        };
        match row.op.as_str() {
            "copy" => {
                let staged = free_path(temp_sibling(&destination, batch));
                std::fs::rename(&destination, &staged)
                    .io_context("복사한 폴더를 임시 위치로 옮기다가 실패했습니다")?;
                if let Err(error) = delete_copied_db(db, &destination_lib, &destination_rel) {
                    let _ = std::fs::rename(&staged, &destination);
                    return Err(error);
                }
                if let Err(error) = remove_tree(&staged) {
                    log::warn!(
                        "폴더 복사 undo 임시 갈래 정리 보류 {}: {error}",
                        staged.display()
                    );
                }
            }
            "move" | "rename" => {
                let wanted = source_root.join(&row.source_path);
                let target = if wanted.exists() {
                    free_path(wanted)
                } else {
                    wanted
                };
                let restored_rel = target
                    .strip_prefix(&source_root)
                    .map_err(|_| bad("복원 경로 오류"))?
                    .to_string_lossy()
                    .into_owned();
                if restored_rel != row.source_path {
                    // 원래 자리가 차 있어 옆 이름으로 돌아왔다 — «되돌렸다»만 보이면 사용자는 모른다
                    out.first_error = Some(format!(
                        "원래 자리에 같은 이름이 있어 «{restored_rel}» 로 되돌렸습니다"
                    ));
                }
                if source_lib.volume_uuid == destination_lib.volume_uuid {
                    if let Some(p) = target.parent() {
                        std::fs::create_dir_all(p)
                            .io_context("폴더 복원 목적지의 부모를 만들다가 실패했습니다")?;
                    }
                    std::fs::rename(&destination, &target)
                        .io_context("폴더를 원래 위치로 복원하다가 실패했습니다")?;
                    if let Err(error) = move_db_rows(
                        db,
                        &destination_lib,
                        &destination_rel,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = std::fs::rename(&target, &destination);
                        return Err(error);
                    }
                } else {
                    // 볼륨을 건너는 복원은 사본을 내용 해시로 검증한다
                    let verified = if now.sha256.is_empty() {
                        manifest(&destination)?
                    } else {
                        now.clone()
                    };
                    copy_tree_verified(&destination, &target, batch, None, &verified)?;
                    let staged = free_path(temp_sibling(&destination, batch));
                    if let Err(error) = std::fs::rename(&destination, &staged) {
                        let _ = remove_tree(&target);
                        return Err(DbError::Invalid(format!(
                            "복원할 폴더를 임시 위치로 옮기다가 실패했습니다: {error}"
                        )));
                    }
                    if let Err(error) = move_db_rows(
                        db,
                        &destination_lib,
                        &destination_rel,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = remove_tree(&target);
                        let _ = std::fs::rename(&staged, &destination);
                        return Err(error);
                    }
                    if let Err(error) = remove_tree(&staged) {
                        log::warn!(
                            "폴더 이동 undo 임시 갈래 정리 보류 {}: {error}",
                            staged.display()
                        );
                    }
                }
            }
            "trash" => {
                let wanted = source_root.join(&row.source_path);
                let target = if wanted.exists() {
                    free_path(wanted)
                } else {
                    wanted
                };
                if let Some(p) = target.parent() {
                    std::fs::create_dir_all(p)
                        .io_context("휴지통 폴더 복원 경로를 만들다가 실패했습니다")?;
                }
                std::fs::rename(&destination, &target)
                    .io_context("휴지통 폴더를 원래 위치로 복원하다가 실패했습니다")?;
                let restored_rel = target
                    .strip_prefix(&source_root)
                    .map_err(|_| bad("복원 경로 오류"))?
                    .to_string_lossy()
                    .into_owned();
                if restored_rel != row.source_path {
                    // 원래 자리가 차 있어 옆 이름으로 돌아왔다 — «되돌렸다»만 보이면 사용자는 모른다
                    out.first_error = Some(format!(
                        "원래 자리에 같은 이름이 있어 «{restored_rel}» 로 되돌렸습니다"
                    ));
                }
                let moved_in_db = restored_rel != row.source_path;
                if moved_in_db {
                    if let Err(error) = move_db_rows(
                        db,
                        &source_lib,
                        &row.source_path,
                        &source_lib,
                        &restored_rel,
                    ) {
                        let _ = std::fs::rename(&target, &destination);
                        return Err(error);
                    }
                }
                let rows = rows_in_subtree(db, source_lib.id, &restored_rel)?;
                let changed = db.transaction(|tx|{for (id,_) in &rows{tx.execute("UPDATE files SET trashed_at=NULL,trash_path=NULL,trash_batch=NULL WHERE folder_id=?1 AND trash_batch=?2",rusqlite::params![id,batch])?;tx.execute("UPDATE folders SET scanned_at=CASE WHEN scanned_at=-2 THEN -1 ELSE scanned_at END WHERE id=?1",[id])?;}Ok(())});
                if let Err(error) = changed {
                    if moved_in_db {
                        let _ = move_db_rows(
                            db,
                            &source_lib,
                            &restored_rel,
                            &source_lib,
                            &row.source_path,
                        );
                    }
                    let _ = std::fs::rename(&target, &destination);
                    return Err(error);
                }
                if let Err(error) = refresh_counts(db, source_lib.id) {
                    log::warn!("폴더 휴지통 undo 장수 갱신 보류: {error}");
                }
            }
            _ => return Err(bad("알 수 없는 폴더 저널입니다")),
        }
    }
    crate::ops::undo::mark_undone(db, batch)?;
    out.moved = 1;
    out.bytes = row.bytes as i64;
    out.folders_removed = row.dirs;
    let _ = row.files;
    Ok(out)
}
