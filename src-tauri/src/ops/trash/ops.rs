use super::*;

pub fn to_trash(db: &Db, ids: &[i64], label: &str) -> Result<Outcome> {
    let items = load(db, ids, false)?;
    if items.is_empty() {
        // 빈 배치를 남기지 않는다 — 되돌리기 목록에 «0장»이 쌓이고 사용자는 «안 된다»고 읽는다
        return Ok(Outcome {
            first_error: Some("휴지통으로 옮길 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::super::open_batch(db, "trash", label)?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };
    // 라이브러리·마운트는 한 번만 — 파일마다 찾으면 5천 장에 수천만 행 스캔·수만 syscall (리뷰 H16)
    let (libs, mounts) = lookups(db, &items)?;
    // 옮기고 나서 비는 폴더 — (폴더 행, 디스크 경로, 라이브러리 뿌리)
    let mut touched: std::collections::BTreeMap<i64, (PathBuf, PathBuf)> =
        std::collections::BTreeMap::new();

    for it in &items {
        let lib = libs.get(&it.library_id);
        let (Some(lib_dir), Some(lib_rel), Some(mount)) = (
            lib.and_then(|l| l.dir.clone()),
            lib.map(|l| l.rel_path.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
        ) else {
            let _ = super::super::record(
                db,
                batch_id,
                "trash",
                it.id,
                &it.volume_uuid,
                &it.vol_rel,
                None,
                Err("디스크가 연결되어 있지 않습니다"),
            );
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("디스크가 연결되어 있지 않습니다".into());
            continue;
        };

        let src = mount.join(&it.vol_rel);
        let dest = free_path(trash_root(&lib_dir).join(&it.lib_rel));
        let dest_rel = dest
            .strip_prefix(&lib_dir)
            .unwrap_or(&dest)
            .to_string_lossy()
            .into_owned();

        match move_with_sidecars(&src, &dest) {
            Ok(()) => {
                // 저널 경로는 언제나 볼륨 기준이다 — 되돌릴 때 마운트만 붙이면 된다
                let to_vol_rel = crate::media::cache::rel_path(&lib_rel, &dest_rel);
                // 저널과 행 갱신은 한 트랜잭션. 파일은 이미 휴지통에 있으므로 실패하면
                // 제자리로 돌려놓고 실패로 센다 — 저널만 남고 행이 안 바뀌면 격자엔
                // 보이는데 열리지 않는 사진이 된다 (2차 리뷰 M-4)
                let (to_size, to_mtime) = super::super::file_stat(&dest);
                let recorded = db.transaction(|tx| {
                    tx.execute(
                        "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                         VALUES(?1,?2,'trash',?3,?4,?3,?5,1,?6,?7)",
                        rusqlite::params![batch_id, it.id, it.volume_uuid, it.vol_rel, to_vol_rel, to_size, to_mtime],
                    )?;
                    tx.execute(
                        "UPDATE files SET trashed_at = strftime('%s','now'),
                                          trash_path = ?2, trash_batch = ?3
                         WHERE id = ?1",
                        rusqlite::params![it.id, dest_rel, batch_id],
                    )?;
                    Ok(())
                });
                match recorded {
                    Ok(()) => {
                        if let Some(dir) = src.parent() {
                            touched
                                .entry(it.folder_id)
                                .or_insert_with(|| (dir.to_path_buf(), lib_dir.clone()));
                        }
                        out.moved += 1;
                        out.bytes += it.size;
                    }
                    Err(error) => {
                        let message = match move_with_sidecars(&dest, &src) {
                            Ok(()) => error.to_string(),
                            Err(rollback) => format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ),
                        };
                        let _ = super::super::record(
                            db,
                            batch_id,
                            "trash",
                            it.id,
                            &it.volume_uuid,
                            &it.vol_rel,
                            None,
                            Err(&message),
                        );
                        out.failed += 1;
                        out.failed_ids.push(it.id);
                        out.first_error.get_or_insert(message);
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = super::super::record(
                    db,
                    batch_id,
                    "trash",
                    it.id,
                    &it.volume_uuid,
                    &it.vol_rel,
                    None,
                    Err(&msg),
                );
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(msg);
            }
        }
    }

    super::super::close_batch(db, batch_id, out.moved)?;
    // 사진이 다 나간 폴더는 디스크에서 지운다 — «폴더가 똑같아서» 치운 것인데 빈 껍데기가
    // 남으면 비교 화면에 «0장»으로 다시 나오고 Finder 에도 남는다 (사용자 지적).
    // 폴더 행은 남긴다: 휴지통의 파일 행이 그 폴더를 가리키고(FK CASCADE), 되돌리면 폴더가 되살아난다
    for (folder_id, (dir, lib_dir)) in touched {
        let live: i64 = db.read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM files WHERE folder_id = ?1 AND trashed_at IS NULL",
                [folder_id],
                |r| r.get(0),
            )
        })?;
        if live == 0 {
            out.folders_removed += prune_empty_dirs(&dir, &lib_dir);
        }
    }
    Ok(out)
}

/// Finder 가 남기는 것 — 이것만 있으면 «빈 폴더»로 본다
fn is_junk_entry(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name == "Thumbs.db" || name == "desktop.ini"
}

/// 빈 폴더를 지우고, 그래서 빈 위 폴더도 `stop`(라이브러리 뿌리) 바로 아래까지 올라가며 지운다.
/// 사진·다른 파일·하위 폴더가 하나라도 있으면 손대지 않는다. 지운 폴더 수를 돌려준다
pub fn prune_empty_dirs(dir: &Path, stop: &Path) -> usize {
    let mut n = 0;
    let mut cur = dir.to_path_buf();
    loop {
        if cur == stop
            || !cur.starts_with(stop)
            || cur.file_name().map(|f| f == ".acut").unwrap_or(false)
        {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&cur) else {
            break;
        };
        let mut junk = Vec::new();
        let mut other = false;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map(|t| t.is_file()).unwrap_or(false) && is_junk_entry(&name) {
                junk.push(e.path());
            } else {
                other = true;
                break;
            }
        }
        if other {
            break;
        }
        for j in junk {
            let _ = std::fs::remove_file(j);
        }
        if std::fs::remove_dir(&cur).is_err() {
            break;
        }
        n += 1;
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
    n
}

/// 휴지통에서 제자리로 되돌린다. 평점·판정은 그대로 살아 있다.
pub fn restore(db: &Db, ids: &[i64]) -> Result<Outcome> {
    let items = load(db, ids, true)?;
    if items.is_empty() {
        return Ok(Outcome {
            first_error: Some("휴지통에 되돌릴 사진이 없습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::super::open_batch(db, "restore", "휴지통에서 되돌리기")?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };

    let (libs, mounts) = lookups(db, &items)?;

    for it in &items {
        let lib = libs.get(&it.library_id);
        let (Some(lib_dir), Some(lib_rel), Some(mount), Some(tp)) = (
            lib.and_then(|l| l.dir.clone()),
            lib.map(|l| l.rel_path.clone()),
            mounts.get(&it.volume_uuid).cloned().flatten(),
            it.trash_path.as_ref(),
        ) else {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("되돌릴 위치를 알 수 없습니다".into());
            continue;
        };

        let src = lib_dir.join(tp);
        // `empty()` 와 같은 경계: trash_path 가 휴지통 밖(다른 사진, 링크 너머)을 가리키면
        // 그 파일을 «되돌린다»며 옮기면 안 된다
        if !is_inside(&src, &trash_root(&lib_dir)) {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error
                .get_or_insert("휴지통 밖의 경로는 되돌리지 않습니다".into());
            continue;
        }
        let dest = free_path(mount.join(&it.vol_rel));
        match move_with_sidecars(&src, &dest) {
            Ok(()) => {
                // 저널 — ⌘Z 로 «되돌리기»를 물릴 수 있게(다시 휴지통으로). 경로는 볼륨 기준
                let from_vol_rel = crate::media::cache::rel_path(&lib_rel, tp);
                let to_vol_rel = dest
                    .strip_prefix(&mount)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| it.vol_rel.clone());
                // 그새 같은 이름이 생겨 «IMG_1 (2).jpg»로 돌아왔을 수 있다 — 행도 그 이름으로.
                // 안 맞추면 다음 치우기·이름 바꾸기가 다른 사진에 걸린다 (리뷰 C5)
                let new_name = dest
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        it.vol_rel
                            .rsplit('/')
                            .next()
                            .unwrap_or(&it.vol_rel)
                            .to_string()
                    });
                // 저널과 행 갱신은 한 트랜잭션. 실패하면 파일을 휴지통 자리로 되돌린다 —
                // 디스크만 돌아오고 행이 «휴지통»이면 그 사진은 어느 화면에도 없다 (2차 리뷰 M-4)
                let (to_size, to_mtime) = super::super::file_stat(&dest);
                let recorded = db.transaction(|tx| {
                    tx.execute(
                        "INSERT INTO journal(batch_id,file_id,op,from_vol,from_path,to_vol,to_path,ok,to_size,to_mtime)
                         VALUES(?1,?2,'restore',?3,?4,?3,?5,1,?6,?7)",
                        rusqlite::params![batch_id, it.id, it.volume_uuid, from_vol_rel, to_vol_rel, to_size, to_mtime],
                    )?;
                    tx.execute(
                        "UPDATE files SET trashed_at = NULL, trash_path = NULL, trash_batch = NULL,
                                name = ?2
                         WHERE id = ?1",
                        rusqlite::params![it.id, new_name],
                    )?;
                    Ok(())
                });
                match recorded {
                    Ok(()) => {
                        out.moved += 1;
                        out.bytes += it.size;
                    }
                    Err(error) => {
                        let message = match move_with_sidecars(&dest, &src) {
                            Ok(()) => error.to_string(),
                            Err(rollback) => format!(
                                "DB 갱신 실패: {error}; 파일 원위치 복구도 실패: {rollback}"
                            ),
                        };
                        out.failed += 1;
                        out.failed_ids.push(it.id);
                        out.first_error.get_or_insert(message);
                    }
                }
            }
            Err(e) => {
                out.failed += 1;
                out.failed_ids.push(it.id);
                out.first_error.get_or_insert(e.to_string());
            }
        }
    }
    super::super::close_batch(db, batch_id, out.moved)?;
    Ok(out)
}

/// 휴지통을 진짜로 비운다. **되돌릴 수 없다.**
///
/// 안전장치: 지우려는 경로가 그 라이브러리의 휴지통 안인지 정규화 후 다시
/// 확인한다. 심볼릭 링크나 `..`으로 밖을 가리키면 건너뛴다.
pub fn empty(db: &Db, cache_base: &Path, ids: &[i64]) -> Result<Outcome> {
    let items = load(db, ids, true)?;
    if items.is_empty() {
        return Ok(Outcome {
            first_error: Some("휴지통이 비어 있습니다".into()),
            ..Default::default()
        });
    }
    let batch_id = super::super::open_batch(db, "delete", "휴지통 비우기")?;
    let mut out = Outcome {
        batch_id,
        ..Default::default()
    };

    let (libs, _) = lookups(db, &items)?;
    for it in &items {
        let (Some(lib_dir), Some(tp)) = (
            libs.get(&it.library_id).and_then(|l| l.dir.clone()),
            it.trash_path.as_ref(),
        ) else {
            out.failed += 1;
            out.failed_ids.push(it.id);
            out.first_error.get_or_insert(
                "라이브러리가 연결되어 있지 않거나 휴지통 경로 기록이 없습니다".into(),
            );
            continue;
        };
        let victim = lib_dir.join(tp);
        if !is_inside(&victim, &trash_root(&lib_dir)) {
            out.failed += 1;
            out.first_error
                .get_or_insert("휴지통 밖의 경로입니다".into());
            continue;
        }
        // 이미 사라진 파일은 성공으로 친다 — 목표는 "없는 상태"다
        match std::fs::remove_file(&victim) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                out.failed += 1;
                out.first_error.get_or_insert(e.to_string());
                continue;
            }
        }
        remove_sidecars(&victim);
        let thumb_rel: Option<String> = db.read(|c| {
            c.query_row(
                "SELECT rel_path FROM thumbs WHERE file_id=?1",
                [it.id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
        })?;
        if let Some(rel) = thumb_rel {
            let thumb = crate::media::cache::cache_root(cache_base, it.library_id).join(rel);
            if let Err(error) = std::fs::remove_file(&thumb) {
                log::warn!(
                    "영구 삭제한 사진의 썸네일 파일을 지우지 못했습니다: {}: {error}",
                    thumb.display()
                );
            }
        }
        db.write(|c| c.execute("DELETE FROM files WHERE id = ?1", [it.id]))?;
        out.moved += 1;
        out.bytes += it.size;
    }
    super::super::close_batch(db, batch_id, out.moved)?;
    // 파일 행이 하나도 안 남은 폴더 행은 이제 치운다 — 디스크의 폴더는 치울 때 이미 지웠다
    let folders: std::collections::BTreeSet<i64> = items.iter().map(|i| i.folder_id).collect();
    for f in folders {
        out.folders_removed += db.write(|c| {
            c.execute(
                "DELETE FROM folders WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM files WHERE folder_id = ?1)",
                [f],
            )
        })?;
    }
    Ok(out)
}

/// 정규화한 뒤에도 `root` 안에 있는가. 없는 경로는 부모까지 올라가 확인한다.
fn is_inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let real = path.canonicalize().or_else(|_| {
        path.parent()
            .map(Path::canonicalize)
            .unwrap_or_else(|| path.canonicalize())
    });
    real.map(|p| p.starts_with(&root)).unwrap_or(false)
}
