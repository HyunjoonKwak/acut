use super::*;

pub(super) fn temp_sibling(target: &Path, batch: i64) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.photo-desk-{batch}.tmp"))
}

pub(super) fn copy_tree_verified(
    source: &Path,
    target: &Path,
    batch: i64,
    fail_after: Option<usize>,
    before: &Manifest,
) -> Result<()> {
    let temp = temp_sibling(target, batch);
    if temp.exists() {
        remove_tree(&temp).io_context("이전 폴더 복사 임시 경로를 지우다가 실패했습니다")?;
    }
    std::fs::create_dir_all(&temp).io_context("폴더 복사 임시 경로를 만들다가 실패했습니다")?;
    let result = (|| -> Result<()> {
        let mut copied = 0usize;
        for entry in WalkDir::new(source).min_depth(1).follow_links(false) {
            let entry = entry.map_err(|e| bad(e.to_string()))?;
            if entry.file_type().is_symlink() {
                return Err(bad("심볼릭 링크가 든 폴더는 복사할 수 없습니다"));
            }
            if entry.file_type().is_file() && is_appledouble(entry.path()) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(source)
                .map_err(|_| bad("복사 경로 오류"))?;
            let dest = temp.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&dest)
                    .io_context("복사할 하위 폴더를 만들다가 실패했습니다")?;
            } else if entry.file_type().is_file() {
                if fail_after.is_some_and(|limit| copied >= limit) {
                    return Err(bad("시험용 부분 실패"));
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .io_context("복사할 파일의 부모 폴더를 만들다가 실패했습니다")?;
                }
                std::fs::copy(entry.path(), &dest)
                    .io_context("폴더 안의 파일을 복사하다가 실패했습니다")?;
                copy_mtime(entry.path(), &dest);
                std::fs::File::open(&dest)
                    .io_context("복사한 파일을 열다가 실패했습니다")?
                    .sync_all()
                    .io_context("복사한 파일을 디스크에 기록하다가 실패했습니다")?;
                copied += 1;
            }
        }
        let after = manifest(&temp)?;
        if before.sha256 != after.sha256 {
            return Err(bad("폴더 사본의 SHA-256 manifest가 원본과 다릅니다"));
        }
        if target.exists() {
            return Err(bad("실행 직전 목적지에 같은 이름이 생겼습니다"));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .io_context("폴더 복사 목적지의 부모를 만들다가 실패했습니다")?;
        }
        std::fs::rename(&temp, target)
            .io_context("검증한 폴더 사본을 목적지로 옮기다가 실패했습니다")?;
        if let Some(parent) = target.parent() {
            std::fs::File::open(parent)
                .io_context("폴더 복사 목적지를 열다가 실패했습니다")?
                .sync_all()
                .io_context("폴더 복사 목적지를 디스크에 기록하다가 실패했습니다")?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = remove_tree(&temp);
        return Err(error);
    }
    Ok(())
}

/// 폴더 이동의 물리 단계. 다른 볼륨이면 검증된 사본을 먼저 완성하고, 원본은
/// 같은 볼륨 안의 임시 백업 이름으로 원자적으로 치워 둔다. DB 갱신 실패 시
/// 호출자가 그 백업을 제자리로 돌릴 수 있다.
pub(super) fn stage_move(
    source: &Path,
    destination: &Path,
    batch: i64,
    cross_volume: bool,
    info: &Manifest,
) -> Result<Option<PathBuf>> {
    if cross_volume {
        copy_tree_verified(source, destination, batch, None, info)?;
        let backup = free_path(
            source
                .parent()
                .unwrap_or(source)
                .join(format!(".photo-desk-move-{batch}.bak")),
        );
        if let Err(error) = std::fs::rename(source, &backup) {
            let _ = remove_tree(destination);
            return Err(DbError::Invalid(format!(
                "볼륨 간 이동 뒤 원본 폴더를 보관하다가 실패했습니다: {error}"
            )));
        }
        Ok(Some(backup))
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .io_context("이동 목적지의 부모 폴더를 만들다가 실패했습니다")?;
        }
        std::fs::rename(source, destination).io_context("폴더를 목적지로 옮기다가 실패했습니다")?;
        Ok(None)
    }
}

fn vol_rel(lib: &Library, rel: &str) -> String {
    crate::media::cache::rel_path(&lib.rel_path, rel)
}

pub(super) fn rows_in_subtree(db: &Db, library_id: i64, root: &str) -> Result<Vec<(i64, String)>> {
    let root = vol_rel(&library(db, library_id)?, root);
    let escaped = crate::db::query::escape_like(&root);
    db.read(|c| {
        let mut statement = c.prepare(
            "SELECT id,rel_path FROM folders WHERE library_id=?1
             AND (rel_path=?2 OR rel_path LIKE ?3 || '/%' ESCAPE '\\') ORDER BY length(rel_path),rel_path",
        )?;
        let rows = statement.query_map(rusqlite::params![library_id, root, escaped], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
}

fn ensure_folder_row(db: &Db, lib: &Library, rel: &str, marker: i64) -> Result<i64> {
    let path = vol_rel(lib, rel);
    let name = rel.rsplit('/').next().unwrap_or(&lib.name);
    db.write(|c| {
        c.execute(
            "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET library_id=excluded.library_id",
            rusqlite::params![lib.volume_uuid, lib.id, path, name, lib.area, marker],
        )
    })?;
    db.read(|c| {
        c.query_row(
            "SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2",
            rusqlite::params![lib.volume_uuid, path],
            |r| r.get(0),
        )
    })
}

pub(super) fn refresh_counts(db: &Db, library_id: i64) -> Result<()> {
    db.write(|c| c.execute(
        "UPDATE folders SET file_count=(SELECT COUNT(*) FROM files WHERE files.folder_id=folders.id AND files.trashed_at IS NULL) WHERE library_id=?1",
        [library_id],
    ))?;
    Ok(())
}

pub(super) fn move_db_rows(
    db: &Db,
    source_lib: &Library,
    source_rel: &str,
    destination_lib: &Library,
    destination_rel: &str,
) -> Result<()> {
    let rows = rows_in_subtree(db, source_lib.id, source_rel)?;
    let source_vol = vol_rel(source_lib, source_rel);
    let destination_vol = vol_rel(destination_lib, destination_rel);
    db.transaction(|tx| {
        for (id, old) in &rows {
            if source_lib.id != destination_lib.id {
                tx.execute(
                    "DELETE FROM thumbs WHERE file_id IN (SELECT id FROM files WHERE folder_id=?1)",
                    [id],
                )?;
            }
            let suffix = old.strip_prefix(&source_vol).unwrap_or_default();
            let new_path = format!("{destination_vol}{suffix}");
            let name = new_path.rsplit('/').next().unwrap_or(&new_path);
            tx.execute(
                "UPDATE folders SET volume_uuid=?2,library_id=?3,rel_path=?4,name=?5,area=?6,
                 parent_id=CASE WHEN rel_path=?7 THEN NULL ELSE parent_id END WHERE id=?1",
                rusqlite::params![
                    id,
                    destination_lib.volume_uuid,
                    destination_lib.id,
                    new_path,
                    name,
                    destination_lib.area,
                    source_vol
                ],
            )?;
        }
        Ok(())
    })?;
    if rows.is_empty() {
        ensure_folder_row(db, destination_lib, destination_rel, -1)?;
    }
    if let Err(error) = refresh_counts(db, source_lib.id) {
        log::warn!("폴더 이동 뒤 원본 장수 갱신 보류: {error}");
    }
    if source_lib.id != destination_lib.id {
        if let Err(error) = refresh_counts(db, destination_lib.id) {
            log::warn!("폴더 이동 뒤 목적지 장수 갱신 보류: {error}");
        }
    }
    Ok(())
}

fn copy_db_rows(
    db: &Db,
    source_lib: &Library,
    source_rel: &str,
    destination_lib: &Library,
    destination_rel: &str,
    hashes: &HashMap<String, String>,
) -> Result<()> {
    let rows = rows_in_subtree(db, source_lib.id, source_rel)?;
    let source_vol = vol_rel(source_lib, source_rel);
    let mut mapped = HashMap::new();
    let root_id = ensure_folder_row(db, destination_lib, destination_rel, -1)?;
    for (old_id, old_path) in rows {
        let suffix = old_path
            .strip_prefix(&source_vol)
            .unwrap_or_default()
            .trim_start_matches('/');
        let new_rel = if suffix.is_empty() {
            destination_rel.to_string()
        } else {
            join_rel(destination_rel, suffix)
        };
        let new_id = ensure_folder_row(db, destination_lib, &new_rel, -1)?;
        mapped.insert(old_id, (new_id, suffix.to_string()));
    }
    if mapped.is_empty() {
        mapped.insert(-1, (root_id, String::new()));
    }
    for (old_folder, (new_folder, suffix)) in &mapped {
        if *old_folder == -1 {
            continue;
        }
        let files: Vec<(i64, String)> = db.read(|c| {
            let mut statement = c.prepare(
                "SELECT id,name FROM files WHERE folder_id=?1 AND trashed_at IS NULL ORDER BY id",
            )?;
            let rows = statement.query_map([old_folder], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for (file_id, name) in files {
            let rel = if suffix.is_empty() {
                name.clone()
            } else {
                join_rel(suffix, &name)
            };
            let hash = hashes
                .get(&rel)
                .ok_or_else(|| bad(format!("복사 manifest에 파일이 없습니다: {rel}")))?;
            crate::ops::transfer::clone_row(db, file_id, *new_folder, &name, hash)?;
        }
    }
    if let Err(error) = refresh_counts(db, destination_lib.id) {
        log::warn!("폴더 복사 뒤 장수 갱신 보류: {error}");
    }
    Ok(())
}

pub(super) fn delete_copied_db(db: &Db, lib: &Library, rel: &str) -> Result<()> {
    let rows = rows_in_subtree(db, lib.id, rel)?;
    db.transaction(|tx| {
        for (id, _) in rows.iter().rev() {
            tx.execute("DELETE FROM folders WHERE id=?1", [id])?;
        }
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
fn record_folder(
    db: &Db,
    batch: i64,
    op: &str,
    source_lib: i64,
    source: &str,
    destination_lib: Option<i64>,
    destination: Option<&str>,
    info: &Manifest,
    cross_volume: bool,
) -> Result<()> {
    db.write(|c| c.execute(
        "INSERT INTO folder_journal(batch_id,op,source_library_id,source_path,destination_library_id,destination_path,file_count,dir_count,bytes,manifest_sha256,cross_volume,stat_sha256)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        rusqlite::params![batch,op,source_lib,source,destination_lib,destination,info.files as i64,info.directories as i64,info.bytes as i64,info.sha256,cross_volume as i32,info.stat_sha256],
    ))?;
    Ok(())
}

fn discard_batch(db: &Db, batch: i64) {
    let _ = db.write(|c| c.execute("DELETE FROM batches WHERE id=?1", [batch]));
}

pub fn execute(db: &Db, request: &Request, label: &str) -> Result<FolderOutcome> {
    // 실행 시점의 계획과 manifest를 한 번에 만든다. 프론트 미리보기 값은 오래됐을
    // 수 있어 신뢰하지 않되, 같은 실행 안에서 원본 트리를 서너 번 다시 읽지는 않는다.
    let OperationPlan {
        preview,
        info,
        source_lib,
        destination_lib,
        source_rel,
        source,
        destination,
    } = operation_plan(db, request, PlanDetail::Verified)?;
    if preview.action == "skip" {
        return Ok(FolderOutcome {
            batch_id: 0,
            completed: 0,
            failed: 0,
            files: preview.files,
            directories: preview.directories,
            bytes: preview.bytes,
            first_error: Some("같은 이름이 있어 실행하지 않았습니다".into()),
            manifest_sha256: None,
        });
    }
    let same_existing_folder = request.action == Action::Rename
        && destination.exists()
        && matches!(
            (source.canonicalize(), destination.canonicalize()),
            (Ok(source), Ok(destination)) if source == destination
        );
    if destination.exists() && request.action != Action::Trash && !same_existing_folder {
        return Err(bad("실행 직전 목적지에 같은 이름이 생겼습니다"));
    }
    let kind = match request.action {
        Action::Create => "folder_create",
        Action::Rename => "folder_rename",
        Action::Move => "folder_move",
        Action::Copy => "folder_copy",
        Action::Trash => "folder_trash",
    };
    let batch = super::super::open_batch(db, kind, label)?;
    // 작업은 끝났지만 사용자가 알아야 할 뒷정리 실패 — 결과의 first_error 로 보여 준다
    let mut warning: Option<String> = None;
    let operation = (|| -> Result<Manifest> {
        match request.action {
            Action::Create => {
                record_folder(
                    db,
                    batch,
                    "create",
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    false,
                )?;
                std::fs::create_dir(&destination).io_context("새 폴더를 만들다가 실패했습니다")?;
                if let Err(error) =
                    ensure_folder_row(db, &destination_lib, &preview.destination, -1)
                {
                    let _ = std::fs::remove_dir(&destination);
                    return Err(error);
                }
                Ok(info.clone())
            }
            Action::Copy => {
                record_folder(
                    db,
                    batch,
                    "copy",
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    preview.cross_volume,
                )?;
                copy_tree_verified(&source, &destination, batch, None, &info)?;
                if let Err(error) = copy_db_rows(
                    db,
                    &source_lib,
                    &source_rel,
                    &destination_lib,
                    &preview.destination,
                    &info.file_hashes,
                ) {
                    let _ = delete_copied_db(db, &destination_lib, &preview.destination);
                    let _ = remove_tree(&destination);
                    return Err(error);
                }
                Ok(info.clone())
            }
            Action::Move | Action::Rename => {
                let cross = source_lib.volume_uuid != destination_lib.volume_uuid;
                record_folder(
                    db,
                    batch,
                    if request.action == Action::Rename {
                        "rename"
                    } else {
                        "move"
                    },
                    source_lib.id,
                    &source_rel,
                    Some(destination_lib.id),
                    Some(&preview.destination),
                    &info,
                    cross,
                )?;
                let backup = stage_move(&source, &destination, batch, cross, &info)?;
                if let Err(error) = move_db_rows(
                    db,
                    &source_lib,
                    &source_rel,
                    &destination_lib,
                    &preview.destination,
                ) {
                    if let Some(backup) = &backup {
                        let _ = std::fs::rename(backup, &source);
                        let _ = remove_tree(&destination);
                    } else {
                        let _ = std::fs::rename(&destination, &source);
                    }
                    return Err(error);
                }
                if let Some(backup) = backup {
                    if let Err(error) = remove_tree(&backup) {
                        // 원본 전체가 든 숨은 폴더가 출발 디스크에 남는다 — 조용히 넘기면
                        // 사용자는 공간이 왜 안 비는지 모른다
                        log::warn!(
                            "볼륨 간 이동 뒤 백업 정리 실패 {}: {error}",
                            backup.display()
                        );
                        warning = Some(format!(
                            "옮기기는 끝났지만 원본 쪽 임시 백업을 지우지 못했습니다: {} ({error}). Finder 에서 지워 주세요",
                            backup.display()
                        ));
                    }
                }
                Ok(info.clone())
            }
            Action::Trash => {
                record_folder(
                    db,
                    batch,
                    "trash",
                    source_lib.id,
                    &source_rel,
                    Some(source_lib.id),
                    Some(&preview.destination),
                    &info,
                    false,
                )?;
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)
                        .io_context("휴지통 폴더를 만들다가 실패했습니다")?;
                }
                std::fs::rename(&source, &destination)
                    .io_context("폴더를 휴지통으로 옮기다가 실패했습니다")?;
                let rows = rows_in_subtree(db, source_lib.id, &source_rel)?;
                let source_vol = vol_rel(&source_lib, &source_rel);
                let trash_prefix = preview.destination.clone();
                let changed=db.transaction(|tx|{
                    for (folder_id,folder_path) in &rows {
                        let suffix=folder_path.strip_prefix(&source_vol).unwrap_or_default().trim_start_matches('/');
                        let files:Vec<(i64,String)>= { let mut st=tx.prepare("SELECT id,name FROM files WHERE folder_id=?1 AND trashed_at IS NULL")?; let it=st.query_map([folder_id],|r|Ok((r.get(0)?,r.get(1)?)))?; it.collect::<rusqlite::Result<Vec<_>>>()? };
                        for (file_id,name) in files {
                            let sub=if suffix.is_empty(){name}else{join_rel(suffix,&name)};
                            let trash_path=join_rel(&trash_prefix,&sub);
                            tx.execute("UPDATE files SET trashed_at=strftime('%s','now'),trash_path=?2,trash_batch=?3 WHERE id=?1",rusqlite::params![file_id,trash_path,batch])?;
                        }
                        tx.execute("UPDATE folders SET scanned_at=CASE WHEN scanned_at=-1 THEN -2 ELSE scanned_at END,file_count=0 WHERE id=?1",[folder_id])?;
                    }
                    Ok(())
                });
                if let Err(error) = changed {
                    let _ = std::fs::rename(&destination, &source);
                    return Err(error);
                }
                Ok(info.clone())
            }
        }
    })();
    match operation {
        Ok(info) => {
            super::super::close_batch(db, batch, 1)?;
            Ok(FolderOutcome {
                batch_id: batch,
                completed: 1,
                failed: 0,
                files: info.files,
                directories: info.directories,
                bytes: info.bytes,
                first_error: warning,
                manifest_sha256: Some(info.sha256).filter(|digest| !digest.is_empty()),
            })
        }
        Err(error) => {
            discard_batch(db, batch);
            Ok(FolderOutcome {
                batch_id: 0,
                completed: 0,
                failed: 1,
                files: preview.files,
                directories: preview.directories,
                bytes: preview.bytes,
                first_error: Some(error.to_string()),
                manifest_sha256: None,
            })
        }
    }
}
