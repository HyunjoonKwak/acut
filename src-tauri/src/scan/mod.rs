//! 스캐너 — 폴더를 훑어 DB에 넣는다.
//!
//! 설계상 지켜야 할 것들
//!   - **NFC 정규화**: macOS 파일시스템은 한글을 NFD(자모 분리)로 준다. NAS(ext4)는
//!     NFC다. 정규화하지 않으면 같은 파일이 다른 이름으로 보여 대조가 어긋난다.
//!     실제로 이 프로젝트에서 중복률이 64.9%로 잘못 나온 적이 있다(실제 76.7%).
//!   - **볼륨 UUID + 상대경로**: 절대경로를 저장하지 않는다.
//!   - **배치 삽입**: 낱개 INSERT는 매번 fsync가 걸린다. 트랜잭션으로 묶는다.
//!   - **증분**: 크기와 수정시각이 그대로면 다시 읽지 않는다.

use crate::db::conn::Db;
use crate::media::{exif, taken_at};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

pub mod kinds;

pub use kinds::Kind;

/// 스캔 진행 상황. UI로 흘려보낸다.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Progress {
    pub found: usize,
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("볼륨을 인식할 수 없습니다: {0}")]
    Volume(#[from] crate::db::volumes::VolumeError),
    #[error("데이터베이스 오류: {0}")]
    Db(#[from] crate::db::conn::DbError),
    #[error("SQLite 오류: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("스캔할 폴더가 없습니다: {0}")]
    NotADirectory(PathBuf),
}

type Result<T> = std::result::Result<T, ScanError>;

/// 파일시스템에서 발견한 파일 하나 (아직 DB에 넣기 전).
#[derive(Debug)]
struct Found {
    rel_dir: String,
    name: String,
    size: u64,
    kind: Kind,
    mtime: Option<i64>,
    birthtime: Option<i64>,
    inode: u64,
    full_path: PathBuf,
}

/// 문자열을 NFC로 정규화한다. 경로·파일명은 **반드시** 이걸 거쳐야 한다.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// 폴더 하나를 스캔해 DB에 반영한다.
///
/// `area`는 이 폴더가 어느 영역인지 (0 작업대 · 1 내사진 · 2 공용 · 3 기타).
pub fn scan_folder(
    db: &Db,
    root: impl AsRef<Path>,
    area: i32,
    on_progress: impl Fn(&Progress) + Sync + Send,
) -> Result<Progress> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Err(ScanError::NotADirectory(root.to_path_buf()));
    }

    // 볼륨을 먼저 등록한다. 이 UUID가 모든 경로의 기준이 된다.
    let vol = crate::db::volumes::describe(root)?;
    db.write(|c| {
        c.execute(
            "INSERT INTO volumes(uuid,name,last_mount_path,role,total_bytes,free_bytes,is_online,last_seen_at)
             VALUES(?1,?2,?3,'library',?4,?5,1,strftime('%s','now'))
             ON CONFLICT(uuid) DO UPDATE SET
               name=excluded.name, last_mount_path=excluded.last_mount_path,
               total_bytes=excluded.total_bytes, free_bytes=excluded.free_bytes,
               is_online=1, last_seen_at=excluded.last_seen_at",
            rusqlite::params![
                vol.uuid,
                vol.name,
                vol.mount_path.to_string_lossy(),
                vol.total_bytes as i64,
                vol.free_bytes as i64
            ],
        )
    })?;

    let found = walk(root, &vol.mount_path);
    let progress = Arc::new(std::sync::Mutex::new(Progress {
        found: found.len(),
        ..Default::default()
    }));
    on_progress(&progress.lock().unwrap().clone());

    // 이미 아는 파일은 건너뛴다 — (상대경로, 이름) → (크기, 수정시각)
    let known = load_known(db, &vol.uuid)?;

    let counter = AtomicUsize::new(0);
    let now = now_secs();

    // 무거운 부분(EXIF 읽기)만 병렬로. DB 쓰기는 뒤에서 한 번에 한다.
    let rows: Vec<_> = found
        .par_iter()
        .filter_map(|f| {
            let key = (f.rel_dir.clone(), f.name.clone());
            if let Some(&(sz, mt)) = known.get(&key) {
                if sz == f.size as i64 && mt == f.mtime.unwrap_or(0) {
                    progress.lock().unwrap().skipped += 1;
                    return None; // 바뀐 게 없다
                }
            }
            let meta = if f.kind == Kind::Video {
                // 영상은 ImageIO 대상이 아니다. 나중에 AVFoundation으로.
                None
            } else {
                exif::read(&f.full_path)
            };
            let m = meta.unwrap_or_default();
            let (ts, src) = taken_at::resolve(m.taken_at, &f.name, f.mtime, f.birthtime, now);

            let n = counter.fetch_add(1, Ordering::Relaxed);
            if n % 500 == 0 {
                on_progress(&progress.lock().unwrap().clone());
            }
            Some((f, m, ts, src))
        })
        .collect();

    // 폴더를 먼저 만들고(부모→자식 순서), 그 다음 파일을 넣는다.
    let mut dirs: Vec<&String> = rows.iter().map(|(f, _, _, _)| &f.rel_dir).collect();
    dirs.sort();
    dirs.dedup();

    db.transaction(|tx| {
        for d in &dirs {
            let name = Path::new(d)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.to_string());
            tx.execute(
                "INSERT INTO folders(volume_uuid,rel_path,name,area,scanned_at)
                 VALUES(?1,?2,?3,?4,strftime('%s','now'))
                 ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET
                   scanned_at=excluded.scanned_at",
                rusqlite::params![vol.uuid, d, name, area],
            )?;
        }

        let mut ins = tx.prepare(
            "INSERT INTO files(folder_id,name,ext,size,kind,taken_at,taken_at_source,
                created_at,modified_at,width,height,orientation,
                cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,
                gps_lat,gps_lon,gps_alt,inode,scanned_at)
             VALUES((SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2),
                ?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,
                strftime('%s','now'))
             ON CONFLICT(folder_id,name) DO UPDATE SET
                size=excluded.size, taken_at=excluded.taken_at,
                taken_at_source=excluded.taken_at_source,
                modified_at=excluded.modified_at, width=excluded.width,
                height=excluded.height, scanned_at=excluded.scanned_at",
        )?;

        for (f, m, ts, src) in &rows {
            let r = ins.execute(rusqlite::params![
                vol.uuid,
                f.rel_dir,
                f.name,
                Path::new(&f.name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase()),
                f.size as i64,
                f.kind as i32,
                ts,
                *src as i32,
                f.birthtime,
                f.mtime,
                m.width,
                m.height,
                m.orientation,
                m.cam_make,
                m.cam_model,
                m.lens,
                m.iso,
                m.aperture,
                m.shutter,
                m.focal_mm,
                m.gps_lat,
                m.gps_lon,
                m.gps_alt,
                f.inode as i64,
            ]);
            let mut p = progress.lock().unwrap();
            match r {
                Ok(1) => p.inserted += 1,
                Ok(_) => p.updated += 1,
                Err(_) => p.failed += 1,
            }
        }
        Ok(())
    })?;

    // 폴더별 파일 수를 갱신한다 (사이드바에서 쓴다).
    db.write(|c| {
        c.execute(
            "UPDATE folders SET file_count =
               (SELECT COUNT(*) FROM files WHERE files.folder_id = folders.id)
             WHERE volume_uuid = ?1",
            [&vol.uuid],
        )
    })?;

    let out = progress.lock().unwrap().clone();
    on_progress(&out);
    Ok(out)
}

/// 이미 DB에 있는 파일들의 (크기, 수정시각). 증분 스캔의 재료다.
fn load_known(
    db: &Db,
    vol_uuid: &str,
) -> Result<std::collections::HashMap<(String, String), (i64, i64)>> {
    let map = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fo.rel_path, fi.name, fi.size, COALESCE(fi.modified_at,0)
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fo.volume_uuid = ?1",
        )?;
        let rows = st.query_map([vol_uuid], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                (r.get::<_, i64>(2)?, r.get::<_, i64>(3)?),
            ))
        })?;
        let mut m = std::collections::HashMap::new();
        for row in rows {
            let (k, v) = row?;
            m.insert(k, v);
        }
        Ok(m)
    })?;
    Ok(map)
}

/// 폴더를 재귀로 훑는다. 심볼릭 링크는 따라가지 않는다(순환 방지).
fn walk(root: &Path, mount: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let name_raw = entry.file_name();
            let name = nfc(&name_raw.to_string_lossy());
            if ft.is_dir() {
                if kinds::is_skipped_dir(&name) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Some(kind) = kinds::classify(&name) else { continue };
            let Ok(md) = entry.metadata() else { continue };
            let rel_dir = dir
                .strip_prefix(mount)
                .ok()
                .map(|p| nfc(&p.to_string_lossy()))
                .unwrap_or_default();
            out.push(Found {
                rel_dir,
                name,
                size: md.len(),
                kind,
                mtime: unix(md.modified().ok()),
                birthtime: unix(md.created().ok()),
                inode: {
                    use std::os::unix::fs::MetadataExt;
                    md.ino()
                },
                full_path: path,
            });
        }
    }
    out
}

fn unix(t: Option<std::time::SystemTime>) -> Option<i64> {
    t?.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_normalizes_hangul() {
        // macOS가 주는 NFD 표기 (자모 분리)
        let nfd = "\u{1112}\u{1161}\u{11AB}"; // 한
        let nfc_str = "\u{D55C}"; // 한
        assert_ne!(nfd, nfc_str, "원래는 다른 문자열이다");
        assert_eq!(nfc(nfd), nfc_str, "NFC로 맞춰져야 한다");
        assert_eq!(nfc(nfc_str), nfc_str, "이미 NFC면 그대로");
    }

    #[test]
    fn scans_a_directory_and_stores_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("2026").join("2026-08-25 테스트");
        std::fs::create_dir_all(&sub).unwrap();
        // 실제 JPEG이 아니어도 경로·크기는 기록된다
        std::fs::write(sub.join("20260825_143000.jpg"), b"x".repeat(100)).unwrap();
        std::fs::write(sub.join("readme.txt"), b"ignored").unwrap();

        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let p = scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(p.inserted, 1, "미디어 파일만 들어가야 한다");

        // 저장된 경로가 절대경로가 아니어야 한다
        let rel: String = db
            .read(|c| {
                c.query_row(
                    "SELECT fo.rel_path FROM files fi JOIN folders fo ON fo.id=fi.folder_id",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert!(!rel.starts_with('/'), "상대경로여야 한다: {rel}");
        assert!(rel.contains("2026-08-25 테스트"), "실제 경로: {rel}");
    }

    #[test]
    fn taken_at_comes_from_the_filename_when_there_is_no_exif() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("20200505_101112.jpg"), b"x").unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();

        let (ts, src): (i64, i32) = db
            .read(|c| {
                c.query_row("SELECT taken_at, taken_at_source FROM files", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(src, taken_at::Source::Filename as i32);
        assert_eq!(ts, taken_at::civil_to_unix(2020, 5, 5, 10, 11, 12));
    }

    #[test]
    fn rescanning_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("20260101_120000.jpg"), b"hello").unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();

        let first = scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(first.inserted, 1);
        assert_eq!(first.skipped, 0);

        let second = scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(second.skipped, 1, "바뀌지 않았으면 건너뛴다");
        assert_eq!(second.inserted, 0);
    }

    #[test]
    fn changed_file_is_rescanned() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260101_120000.jpg");
        std::fs::write(&f, b"hello").unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_folder(&db, dir.path(), 0, |_| {}).unwrap();

        // 크기를 바꾸면 다시 읽어야 한다
        std::fs::write(&f, b"hello world, longer now").unwrap();
        let again = scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(again.skipped, 0);
        assert!(again.inserted + again.updated >= 1);
    }

    #[test]
    fn system_folders_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["@eaDir", ".Spotlight-V100", "#recycle"] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("20260101_120000.jpg"), b"x").unwrap();
        }
        std::fs::write(dir.path().join("20260102_120000.jpg"), b"x").unwrap();

        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        let p = scan_folder(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(p.inserted, 1, "시스템 폴더는 건너뛴다");
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        assert!(scan_folder(&db, "/no/such/dir", 0, |_| {}).is_err());
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// `cargo test --release --lib scan::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 전체를 스캔한다"]
    fn scan_the_whole_library() {
        let root = Path::new("/Volumes/MAIN SSD/MERGE/사진통합작업");
        if !root.is_dir() {
            eprintln!("라이브러리가 없다 — 건너뜀");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path().join("acut.db")).unwrap();

        let t0 = std::time::Instant::now();
        let last = std::sync::Mutex::new(std::time::Instant::now());
        let p = scan_folder(&db, root, 1, |pr| {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_secs() >= 2 {
                let done = pr.inserted + pr.updated + pr.skipped;
                eprintln!("   {done:>7}/{} · {:.0}s", pr.found, t0.elapsed().as_secs_f64());
                *l = std::time::Instant::now();
            }
        })
        .expect("스캔");
        let secs = t0.elapsed().as_secs_f64();

        println!("\n═══ 실제 라이브러리 스캔 ═══");
        println!("  발견   {:>7}", p.found);
        println!("  삽입   {:>7}", p.inserted);
        println!("  실패   {:>7}", p.failed);
        println!("  소요   {secs:>7.1}초  ({:.0}장/초)", p.found as f64 / secs);

        // 쿼리 성능 — 스캔 직후 실제 데이터로
        let bench = |label: &str, sql: &str| {
            let t = std::time::Instant::now();
            let n: i64 = db.read(|c| c.query_row(sql, [], |r| r.get(0))).unwrap();
            println!("  {label:<28} {:>7.1} ms  (n={n})", t.elapsed().as_secs_f64() * 1000.0);
        };
        println!("\n═══ 쿼리 ═══");
        bench("전체 개수", "SELECT COUNT(*) FROM files");
        bench("최신 200장", "SELECT COUNT(*) FROM (SELECT id FROM files ORDER BY taken_at DESC LIMIT 200)");
        bench("RAW만", "SELECT COUNT(*) FROM files WHERE kind=2");
        bench("GPS 있는 것", "SELECT COUNT(*) FROM files WHERE gps_lat IS NOT NULL");
        bench("카메라별", "SELECT COUNT(DISTINCT cam_model) FROM files");

        // 촬영일 출처 분포 — 폴백 체인이 실제로 어떻게 작동했는지
        println!("\n═══ 촬영일 출처 ═══");
        let rows: Vec<(i64, i64)> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT taken_at_source, COUNT(*) FROM files GROUP BY 1 ORDER BY 2 DESC",
                )?;
                let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        for (src, n) in rows {
            let label = match src {
                0 => "EXIF",
                1 => "파일명",
                2 => "파일시각",
                _ => "불명",
            };
            println!("  {label:<10} {n:>7}");
        }
        println!();
        assert!(p.found > 1000, "실제 라이브러리를 찾아야 한다");
    }
}
