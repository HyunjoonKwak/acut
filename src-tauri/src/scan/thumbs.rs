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
/// 1차에서 작게 나온 썸네일을 원본에서 다시 뽑는다 (2차).
///
/// 1차는 박힌 미리보기를 그대로 받아 몇 분에 끝난다. 대신 평균 243px이라
/// 그리드를 크게 놓으면 흐리다. 그래서 뒤에서 다시 키운다 — 이때는 원본을
/// 읽으므로 느리지만, 그 사이에도 앱은 쓸 수 있다.
pub fn upgrade(
    db: &Db,
    library_id: i64,
    volume_mount: &Path,
    cache_root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&ThumbProgress) + Sync + Send,
) -> Result<ThumbProgress, super::ScanError> {
    generate_with(
        db,
        library_id,
        volume_mount,
        cache_root,
        cache::THUMB_PX,
        cancel,
        on_progress,
    )
}

/// DB에 쓸 한 줄: (파일 id, 캐시 상대경로, 원본 크기, 원본 수정시각, 폭, 높이, 상태, 오류)
type Row = (i64, Option<String>, i64, i64, Option<u32>, Option<u32>, i32, Option<String>);

fn write_rows(db: &Db, rows: &[Row]) -> Result<(), super::ScanError> {
    if rows.is_empty() {
        return Ok(());
    }
    db.transaction(|tx| {
        let mut up = tx.prepare(
            "INSERT INTO thumbs(file_id, rel_path, src_size, src_mtime, width, height, state, error, updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%s','now'))
             ON CONFLICT(file_id) DO UPDATE SET
               rel_path=excluded.rel_path, src_size=excluded.src_size,
               src_mtime=excluded.src_mtime, width=excluded.width, height=excluded.height,
               state=excluded.state, error=excluded.error, updated_at=excluded.updated_at",
        )?;
        for (id, rel, size, mtime, w, h, state, err) in rows {
            up.execute(rusqlite::params![id, rel, size, mtime, w, h, state, err])?;
        }
        Ok(())
    })?;
    Ok(())
}

struct Job {
    file_id: i64,
    full_path: PathBuf,
    rel_path: String,
    size: i64,
    mtime: i64,
    /// 영상인가. ImageIO와 QuickLook 중 어느 쪽으로 갈지 정한다.
    video: bool,
}

/// 아직 썸네일이 없거나 원본이 바뀐 파일들의 썸네일을 만든다.
///
/// `cancel`이 true가 되면 남은 작업을 중단하고 지금까지의 결과를 돌려준다.
/// `cache_root`는 이 라이브러리의 캐시 폴더, `volume_mount`는 DB의 상대경로를
/// 실제 경로로 되돌리는 기준이다.
pub fn generate(
    db: &Db,
    library_id: i64,
    volume_mount: &Path,
    cache_root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&ThumbProgress) + Sync + Send,
) -> Result<ThumbProgress, super::ScanError> {
    generate_with(
        db,
        library_id,
        volume_mount,
        cache_root,
        cache::FAST_ACCEPT_PX,
        cancel,
        on_progress,
    )
}

/// `accept_px`는 박힌 미리보기를 받아들이는 최소 크기.
/// 1차 스캔은 [`cache::FAST_ACCEPT_PX`], 화질을 올리는 2차는 `THUMB_PX`를 쓴다.
#[allow(clippy::too_many_arguments)]
pub fn generate_with(
    db: &Db,
    library_id: i64,
    volume_mount: &Path,
    cache_root: &Path,
    accept_px: u32,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&ThumbProgress) + Sync + Send,
) -> Result<ThumbProgress, super::ScanError> {
    let root = cache_root.to_path_buf();

    // 썸네일이 없거나, 있어도 원본의 크기·수정시각이 달라진 것들.
    let jobs: Vec<Job> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.rel_path, fi.name, fi.size, COALESCE(fi.modified_at, 0), fi.kind
             FROM files fi
             JOIN folders fo ON fo.id = fi.folder_id
             LEFT JOIN thumbs t ON t.file_id = fi.id
             WHERE fo.library_id = ?1
               AND fi.trashed_at IS NULL                -- 버린 것은 만들지 않는다
               AND (t.file_id IS NULL
                    OR t.state <> 1
                    OR t.src_size <> fi.size
                    OR t.src_mtime <> COALESCE(fi.modified_at, 0)
                    -- 2차: 작게 나온 것을 다시 키운다. 원본이 더 클 때만.
                    OR (?2 = 1
                        AND MAX(COALESCE(t.width,0), COALESCE(t.height,0)) < ?3
                        AND MAX(COALESCE(fi.width,0), COALESCE(fi.height,0))
                            > MAX(COALESCE(t.width,0), COALESCE(t.height,0))))",
        )?;
        let upgrading = (accept_px >= cache::THUMB_PX) as i64;
        let rows = st.query_map(
            rusqlite::params![library_id, upgrading, cache::UPGRADE_BELOW],
            |r| {
            let rel_dir: String = r.get(1)?;
            let name: String = r.get(2)?;
            let rel_path = cache::rel_path(&rel_dir, &name);
            Ok(Job {
                file_id: r.get(0)?,
                full_path: PathBuf::new(), // 아래에서 채운다
                rel_path,
                size: r.get(3)?,
                mtime: r.get(4)?,
                video: r.get::<_, i32>(5)? == 1,
            })
            },
        )?;
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

    // **청크마다 DB에 쓴다.** 예전에는 전부 메모리에 모았다가 맨 끝에 한 번
    // 썼는데, 8만 장 도중에 앱이 죽으면 몇 분치가 통째로 사라졌다 — 실제로
    // 63,852장이 디스크에만 남고 DB에는 한 줄도 없었다. 화면도 끝날 때까지
    // 0으로 멈춰 있어 멎은 것처럼 보인다.
    const CHUNK: usize = 500;
    for part in jobs.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let results: Vec<Row> = part
            .par_iter()
            .filter_map(|j| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let key = cache::key_for(&j.rel_path, j.size as u64, j.mtime);
                let out = cache::thumb_path(&root, &key);

                // 이미 파일이 있으면 다시 만들지 않는다. DB만 새로 만든 경우가 여기 해당한다.
                // 다만 2차(화질 올리기)에서는 그 파일이 바로 키워야 할 대상이므로 건너뛴다.
                if out.is_file() && accept_px < cache::THUMB_PX {
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

                // 영상은 QuickLook이 대표 프레임을 준다. 그 뒤는 같은 길이다.
                let r = if j.video {
                    crate::media::video::thumbnail(
                        &j.full_path,
                        &out,
                        cache::THUMB_PX,
                        cache::THUMB_QUALITY,
                    )
                } else {
                    thumbnail::make_with(
                        &j.full_path,
                        &out,
                        cache::THUMB_PX,
                        cache::THUMB_QUALITY,
                        accept_px,
                    )
                };
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

        write_rows(db, &results)?;
        on_progress(&progress.lock().unwrap().clone());
    }

    // exFAT이 파일마다 만든 `._` 사이드카를 치운다. 한 장에 16KB라
    // 8만 장이면 1GB가 넘는다. 우리 캐시 폴더 안만 훑는다.
    let (n, bytes) = cache::purge_sidecars(&root);
    if n > 0 {
        log::info!("사이드카 {n}개 정리 ({:.0}MB)", bytes as f64 / 1024.0 / 1024.0);
    }

    let out = progress.lock().unwrap().clone();
    on_progress(&out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

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

    /// 캐시는 **스캔 대상 밖**에 둔다. 안에 두면 스캐너가 썸네일을 사진으로
    /// 집어가 개수가 어긋난다 (실제로 이 시험이 잡아냈다).
    fn cache_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// 방금 등록된 라이브러리의 id. 시험은 항상 하나만 만든다.
    fn lib_id(db: &Db) -> i64 {
        db.read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap()
    }

    #[test]
    fn generates_and_records_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let cache = cache_dir();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();

        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let p = generate(&db, lib_id(&db), &vol.mount_path, cache.path(), Arc::new(AtomicBool::new(false)), |_| {})
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
            cache.path().join(&rel).is_file(),
            "실제 파일이 있어야 한다: {rel}"
        );
    }

    /// 1차는 박힌 미리보기를 그대로 받아 빠르고, 2차가 그걸 다시 키운다.
    /// 이 두 단계가 맞물리지 않으면 화질이 영영 작은 채로 남는다.
    #[test]
    fn the_second_pass_enlarges_what_the_first_left_small() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_dir();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let no = || Arc::new(AtomicBool::new(false));

        generate(&db, lib_id(&db), &vol.mount_path, cache.path(), no(), |_| {}).unwrap();
        let first: i64 = db
            .read(|c| {
                c.query_row("SELECT MAX(width, height) FROM thumbs", [], |r| r.get(0))
            })
            .unwrap();

        let up = upgrade(&db, lib_id(&db), &vol.mount_path, cache.path(), no(), |_| {}).unwrap();
        let second: i64 = db
            .read(|c| {
                c.query_row("SELECT MAX(width, height) FROM thumbs", [], |r| r.get(0))
            })
            .unwrap();

        if first < cache::UPGRADE_BELOW as i64 {
            assert_eq!(up.total, 1, "작게 나온 것은 2차 대상이어야 한다");
            assert!(second >= first, "2차가 화질을 낮추면 안 된다: {first} -> {second}");
        } else {
            assert_eq!(up.total, 0, "이미 충분히 크면 2차는 건드리지 않는다");
        }

        // 2차를 또 돌려도 할 일이 없어야 한다
        let again = upgrade(&db, lib_id(&db), &vol.mount_path, cache.path(), no(), |_| {}).unwrap();
        assert_eq!(again.total, 0, "두 번째 2차는 비어야 한다");
    }

    #[test]
    fn second_run_has_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let Some(_) = seed_real_jpeg(dir.path()) else { return };
        let cache = cache_dir();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        generate(&db, lib_id(&db), &vol.mount_path, cache.path(), cancel.clone(), |_| {}).unwrap();
        let again = generate(&db, lib_id(&db), &vol.mount_path, cache.path(), cancel, |_| {}).unwrap();
        assert_eq!(again.total, 0, "이미 만든 것은 대상이 아니다");
    }

    #[test]
    fn changed_source_invalidates_the_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let Some(src) = seed_real_jpeg(dir.path()) else { return };
        let cache = cache_dir();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        generate(&db, lib_id(&db), &vol.mount_path, cache.path(), cancel.clone(), |_| {}).unwrap();

        // 원본을 바꾼다 → 크기가 달라지므로 다시 만들어야 한다
        std::fs::write(&src, b"now it is a broken file").unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let after = generate(&db, lib_id(&db), &vol.mount_path, cache.path(), cancel, |_| {}).unwrap();
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
        let cache = cache_dir();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let vol = crate::db::volumes::describe(dir.path()).unwrap();

        // 시작 전에 이미 취소된 상태
        let cancel = Arc::new(AtomicBool::new(true));
        let p = generate(&db, lib_id(&db), &vol.mount_path, cache.path(), cancel, |_| {}).unwrap();
        assert_eq!(p.done, 0, "취소되면 아무것도 하지 않는다");
    }
}
