//! 썸네일 일괄 생성 — 스캔과 분리된 두 번째 단계.
//!
//! 왜 분리하는가: 스캔은 6.5만 장에 27초지만 썸네일은 2분이 걸린다. 붙여 놓으면
//! 사용자가 2분을 기다려야 목록을 본다. 분리하면 **스캔이 끝나는 즉시 목록이 뜨고**
//! 썸네일이 뒤에서 채워진다.
//!
//! 취소 가능해야 한다. 사용자가 중간에 다른 일을 하려 할 때 붙잡아 두면 안 된다.

use crate::db::conn::Db;
use crate::media::{cache, thumbnail};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ThumbProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub reused: usize,
}

/// 썸네일이 필요한 파일 하나.
struct Job {
    file_id: i64,
    full_path: PathBuf,
    rel_path: String,
    size: i64,
    mtime: i64,
}

/// 아직 썸네일이 없거나 원본이 바뀐 파일들의 썸네일을 만든다.
///
/// `cancel`이 true가 되면 남은 작업을 중단하고 지금까지의 결과를 돌려준다.
/// `library_root`는 캐시를 둘 곳이고, `volume_mount`는 DB의 상대경로를 실제
/// 경로로 되돌리는 기준이다. 둘은 다를 수 있다 — 볼륨 안의 하위 폴더를
/// 라이브러리로 잡는 경우가 그렇다.
pub fn generate(
    db: &Db,
    volume_uuid: &str,
    volume_mount: &Path,
    library_root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&ThumbProgress) + Sync + Send,
) -> Result<ThumbProgress, super::ScanError> {
    let root = cache::cache_root(library_root);

    // 썸네일이 없거나, 있어도 원본의 크기·수정시각이 달라진 것들.
    let jobs: Vec<Job> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.rel_path, fi.name, fi.size, COALESCE(fi.modified_at, 0)
             FROM files fi
             JOIN folders fo ON fo.id = fi.folder_id
             LEFT JOIN thumbs t ON t.file_id = fi.id
             WHERE fo.volume_uuid = ?1
               AND fi.kind <> 1                        -- 영상은 아직 (AVFoundation 필요)
               AND (t.file_id IS NULL
                    OR t.state <> 1
                    OR t.src_size <> fi.size
                    OR t.src_mtime <> COALESCE(fi.modified_at, 0))",
        )?;
        let rows = st.query_map([volume_uuid], |r| {
            let rel_dir: String = r.get(1)?;
            let name: String = r.get(2)?;
            let rel_path = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            Ok(Job {
                file_id: r.get(0)?,
                full_path: PathBuf::new(), // 아래에서 채운다
                rel_path,
                size: r.get(3)?,
                mtime: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let jobs: Vec<Job> = jobs
        .into_iter()
        .map(|mut j| {
            j.full_path = volume_mount.join(&j.rel_path);
            j
        })
        .collect();

    let progress = Arc::new(std::sync::Mutex::new(ThumbProgress {
        total: jobs.len(),
        ..Default::default()
    }));
    on_progress(&progress.lock().unwrap().clone());
    if jobs.is_empty() {
        return Ok(progress.lock().unwrap().clone());
    }

    let counter = AtomicUsize::new(0);

    // (file_id, rel_path, w, h, state, error)
    let results: Vec<(i64, Option<String>, i64, i64, Option<u32>, Option<u32>, i32, Option<String>)> =
        jobs.par_iter()
            .filter_map(|j| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let key = cache::key_for(&j.rel_path, j.size as u64, j.mtime);
                let out = cache::thumb_path(&root, &key);

                // 이미 파일이 있으면 다시 만들지 않는다. DB만 새로 만든 경우가 여기 해당한다.
                if out.is_file() {
                    let mut p = progress.lock().unwrap();
                    p.reused += 1;
                    p.done += 1;
                    return Some((
                        j.file_id,
                        Some(cache::thumb_rel(&key)),
                        j.size,
                        j.mtime,
                        None,
                        None,
                        1,
                        None,
                    ));
                }

                let r = thumbnail::make(&j.full_path, &out, cache::THUMB_PX, cache::THUMB_QUALITY);
                let n = counter.fetch_add(1, Ordering::Relaxed);
                let row = match r {
                    Ok(sz) => {
                        progress.lock().unwrap().done += 1;
                        (
                            j.file_id,
                            Some(cache::thumb_rel(&key)),
                            j.size,
                            j.mtime,
                            Some(sz.width),
                            Some(sz.height),
                            1,
                            None,
                        )
                    }
                    Err(e) => {
                        let mut p = progress.lock().unwrap();
                        p.failed += 1;
                        p.done += 1;
                        (j.file_id, None, j.size, j.mtime, None, None, 2, Some(e.to_string()))
                    }
                };
                if n % 200 == 0 {
                    on_progress(&progress.lock().unwrap().clone());
                }
                Some(row)
            })
            .collect();

    db.transaction(|tx| {
        let mut up = tx.prepare(
            "INSERT INTO thumbs(file_id, rel_path, src_size, src_mtime, width, height, state, error, updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%s','now'))
             ON CONFLICT(file_id) DO UPDATE SET
               rel_path=excluded.rel_path, src_size=excluded.src_size,
               src_mtime=excluded.src_mtime, width=excluded.width, height=excluded.height,
               state=excluded.state, error=excluded.error, updated_at=excluded.updated_at",
        )?;
        for (id, rel, size, mtime, w, h, state, err) in &results {
            up.execute(rusqlite::params![id, rel, size, mtime, w, h, state, err])?;
        }
        Ok(())
    })?;

    let out = progress.lock().unwrap().clone();
    on_progress(&out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_folder;

    /// 실제 JPEG 하나를 만들어 둔다 (ImageIO가 읽을 수 있어야 하므로 복사해 온다).
    fn seed_real_jpeg(dir: &Path) -> Option<PathBuf> {
        let roots = ["/Volumes/MAIN SSD/MERGE/사진통합작업", "/Volumes/PHOTO 1"];
        for r in roots {
            let root = Path::new(r);
            if !root.is_dir() {
                continue;
            }
            let mut stack = vec![root.to_path_buf()];
            let mut seen = 0;
            while let Some(d) = stack.pop() {
                seen += 1;
                if seen > 200 {
                    break;
                }
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if p
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("jpg"))
                        .unwrap_or(false)
                    {
                        let dst = dir.join("20200101_120000.jpg");
                        if std::fs::copy(&p, &dst).is_ok() {
                            return Some(dst);
                        }
                    }
                }
            }
        }
        None
    }

    #[test]
    fn generates_and_records_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();

        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let p = generate(&db, &vol.uuid, &vol.mount_path, dir.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap();
        assert_eq!(p.total, 1);
        assert_eq!(p.done, 1);
        assert_eq!(p.failed, 0);

        let (rel, state): (Option<String>, i32) = db
            .read(|c| {
                c.query_row("SELECT rel_path, state FROM thumbs", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(state, 1, "완료 상태여야 한다");
        let rel = rel.expect("경로가 있어야 한다");
        assert!(
            cache::cache_root(dir.path()).join(&rel).is_file(),
            "실제 파일이 있어야 한다: {rel}"
        );
    }

    #[test]
    fn second_run_has_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        generate(&db, &vol.uuid, &vol.mount_path, dir.path(), cancel.clone(), |_| {}).unwrap();
        let again = generate(&db, &vol.uuid, &vol.mount_path, dir.path(), cancel, |_| {}).unwrap();
        assert_eq!(again.total, 0, "이미 만든 것은 대상이 아니다");
    }

    #[test]
    fn changed_source_invalidates_the_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let Some(src) = seed_real_jpeg(dir.path()) else { return };
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        generate(&db, &vol.uuid, &vol.mount_path, dir.path(), cancel.clone(), |_| {}).unwrap();

        // 원본을 바꾼다 → 크기가 달라지므로 다시 만들어야 한다
        std::fs::write(&src, b"now it is a broken file").unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        let after = generate(&db, &vol.uuid, &vol.mount_path, dir.path(), cancel, |_| {}).unwrap();
        assert_eq!(after.total, 1, "원본이 바뀌면 다시 대상이 된다");
        assert_eq!(after.failed, 1, "깨진 파일은 실패로 기록된다");

        let state: i32 = db
            .read(|c| c.query_row("SELECT state FROM thumbs", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(state, 2, "실패 상태로 남아야 재시도 대상이 된다");
    }

    #[test]
    fn cancellation_stops_early() {
        let dir = tempfile::tempdir().unwrap();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();

        // 시작 전에 이미 취소된 상태
        let cancel = Arc::new(AtomicBool::new(true));
        let p = generate(&db, &vol.uuid, &vol.mount_path, dir.path(), cancel, |_| {}).unwrap();
        assert_eq!(p.done, 0, "취소되면 아무것도 하지 않는다");
    }
}
