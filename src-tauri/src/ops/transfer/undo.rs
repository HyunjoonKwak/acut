use super::*;

pub fn undo_copy(db: &Db, batch_id: i64) -> Result<Outcome> {
    #[derive(Debug)]
    struct CopyRow {
        id: i64,
        volume: String,
        path: String,
        expected: Option<String>,
    }
    #[derive(Debug)]
    struct Artifact {
        volume: String,
        path: String,
        sha256: String,
    }
    let rows: Vec<CopyRow> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT j.file_id,COALESCE(j.to_vol,j.from_vol),j.to_path,fi.full_hash
             FROM journal j LEFT JOIN files fi ON fi.id=j.file_id
             WHERE j.batch_id=?1 AND j.ok=1 AND j.file_id IS NOT NULL
               AND j.to_path IS NOT NULL ORDER BY j.id DESC",
        )?;
        let rows = st.query_map([batch_id], |r| {
            Ok(CopyRow {
                id: r.get(0)?,
                volume: r.get(1)?,
                path: r.get(2)?,
                expected: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };
    for row in rows {
        let result = (|| -> Result<()> {
            let mount = crate::db::volumes::find_mount(&row.volume)
                .ok_or_else(|| DbError::Invalid("사본 디스크가 연결되어 있지 않습니다".into()))?;
            let main = mount.join(&row.path);
            let mut artifacts: Vec<Artifact> = db.read(|c| {
                let mut st = c.prepare(
                    "SELECT to_vol,to_path,sha256 FROM copy_manifest
                     WHERE batch_id=?1 AND file_id=?2 ORDER BY seq",
                )?;
                let rows = st.query_map(rusqlite::params![batch_id, row.id], |r| {
                    Ok(Artifact {
                        volume: r.get(0)?,
                        path: r.get(1)?,
                        sha256: r.get(2)?,
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })?;
            if artifacts.is_empty() {
                let expected = row.expected.clone().ok_or_else(|| {
                    DbError::Invalid("사본의 원래 SHA-256 기록이 없어 지우지 않았습니다".into())
                })?;
                if !crate::ops::trash::sidecars(&main, &main).is_empty() {
                    return Err(DbError::Invalid(
                        "0.9.0 사본의 사이드카 경로 기록이 없어 안전하게 되돌릴 수 없습니다".into(),
                    ));
                }
                artifacts.push(Artifact {
                    volume: row.volume.clone(),
                    path: row.path.clone(),
                    sha256: expected,
                });
            }
            let mut paths = Vec::with_capacity(artifacts.len());
            for artifact in &artifacts {
                let artifact_mount =
                    crate::db::volumes::find_mount(&artifact.volume).ok_or_else(|| {
                        DbError::Invalid("사본 디스크가 연결되어 있지 않습니다".into())
                    })?;
                let path = artifact_mount.join(&artifact.path);
                if !path.is_file() {
                    return Err(DbError::Invalid(format!(
                        "되돌릴 사본이 없습니다: {}",
                        artifact.path
                    )));
                }
                if crate::cull::hash::full(&path)
                    .io_context("되돌릴 사본의 해시를 읽다가 실패했습니다")?
                    != artifact.sha256
                {
                    return Err(DbError::Invalid(format!(
                        "사본 내용이 작업 뒤 바뀌어 지우지 않았습니다: {}",
                        artifact.path
                    )));
                }
                paths.push(path);
            }

            // 영구 삭제 전에 같은 폴더 안의 숨은 이름으로 원자적으로 치워 둔다.
            // DB 갱신이 실패하면 전부 제자리로 돌릴 수 있다.
            let mut staged = Vec::with_capacity(paths.len());
            for path in paths {
                let temp = free_path(path.with_file_name(format!(
                    ".{}.photo-desk-undo-{batch_id}.tmp",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )));
                if let Err(error) = std::fs::rename(&path, &temp) {
                    for (original, staged_path) in staged.iter().rev() {
                        let _ = std::fs::rename(staged_path, original);
                    }
                    return Err(DbError::Invalid(format!(
                        "되돌릴 사본을 임시 위치로 옮기다가 실패했습니다: {error}"
                    )));
                }
                staged.push((path, temp));
            }
            let changed = db.transaction(|tx| {
                tx.execute("DELETE FROM files WHERE id=?1", [row.id])?;
                tx.execute(
                    "UPDATE journal SET ok=0 WHERE batch_id=?1 AND file_id=?2 AND ok=1",
                    rusqlite::params![batch_id, row.id],
                )?;
                Ok(())
            });
            if let Err(error) = changed {
                for (original, staged_path) in staged.iter().rev() {
                    let _ = std::fs::rename(staged_path, original);
                }
                return Err(error);
            }
            for (_, staged_path) in staged {
                if let Err(error) = std::fs::remove_file(&staged_path) {
                    log::warn!(
                        "되돌린 사본 임시 파일을 지우지 못했습니다 {}: {error}",
                        staged_path.display()
                    );
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => out.moved += 1,
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(row.id);
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    if rows_is_empty(db, batch_id)? {
        db.write(|c| {
            c.execute(
                "DELETE FROM publication_ledger WHERE batch_id=?1",
                [batch_id],
            )
        })?;
        crate::ops::undo::mark_undone(db, batch_id)?;
    }
    Ok(out)
}

fn rows_is_empty(db: &Db, batch: i64) -> Result<bool> {
    db.read(|c| {
        c.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM journal WHERE batch_id=?1 AND ok=1)",
            [batch],
            |r| r.get(0),
        )
    })
}
