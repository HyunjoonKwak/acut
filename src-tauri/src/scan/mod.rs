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
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

pub mod kinds;
pub mod watch;
pub mod thumbs;

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

/// 라이브러리 하나를 스캔해 DB에 반영한다.
///
/// `library_id`는 등록된 라이브러리, `root`는 그 실제 경로다. 찾아낸 폴더는 전부
/// 이 라이브러리에 속하게 된다 — 썸네일 캐시와 원본 경로를 나중에 이걸로 푼다.
/// `area`는 이 폴더가 어느 영역인지 (0 작업대 · 1 내사진 · 2 공용 · 3 기타).
pub fn scan_folder(
    db: &Db,
    library_id: i64,
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

    // 폴더를 훑는 동안에도 알린다. 8만 장을 다 세고 나서야 첫 알림을 보내면
    // 그때까지 화면이 «아무 반응 없음»이다 — exFAT USB면 수십 초다.
    on_progress(&Progress::default());
    let mut last_found = std::time::Instant::now();
    let found = walk(root, &vol.mount_path, |n| {
        if last_found.elapsed() >= std::time::Duration::from_millis(200) {
            last_found = std::time::Instant::now();
            on_progress(&Progress { found: n, ..Default::default() });
        }
    });
    let progress = Arc::new(std::sync::Mutex::new(Progress {
        found: found.len(),
        ..Default::default()
    }));
    on_progress(&progress.lock().unwrap().clone());

    // 이미 아는 파일은 건너뛴다 — (상대경로, 이름) → (크기, 수정시각)
    let known = load_known(db, library_id)?;

    let last_emit = std::sync::Mutex::new(std::time::Instant::now());
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
            // 영상은 ImageIO가 못 읽는다. Spotlight에서 촬영 시각·해상도를 가져온다.
            // probe는 한 번만 부른다 — 두 번 부르면 스캔이 두 배로 느려진다.
            let (m, duration_ms) = if f.kind == Kind::Video {
                let v = crate::media::video::probe(&f.full_path);
                (
                    exif::Meta {
                        taken_at: v.taken_at,
                        width: v.width.map(|x| x as u32),
                        height: v.height.map(|x| x as u32),
                        ..Default::default()
                    },
                    // 0은 "읽어 봤지만 없더라"는 뜻이다. NULL은 "아직 안 읽었다".
                    // 이 구분이 없으면 Spotlight가 모르는 영상을 스캔할 때마다
                    // 다시 뒤진다 (실측 1,357개 × 26개/초 ≈ 52초).
                    Some(v.duration_ms.unwrap_or(0)),
                )
            } else {
                (exif::read(&f.full_path).unwrap_or_default(), None)
            };
            // 영상의 taken_at_source도 0(exif)으로 남는다. 파일 안에 박힌
            // 메타데이터라는 뜻이라 의미가 같다 — 출처가 EXIF가 아니라 컨테이너일 뿐.
            let (ts, src) = if kinds::classify(&f.name) == Some(Kind::Video) {
                // 영상은 단서 중 가장 이른 것 — 컨테이너 시각은 재인코딩 날로 바뀌기 일쑤다
                let folder = f.rel_dir.rsplit('/').next().unwrap_or("");
                taken_at::resolve_video(m.taken_at, &f.name, folder, f.mtime, f.birthtime, now)
            } else {
                taken_at::resolve(m.taken_at, &f.name, f.mtime, f.birthtime, now)
            };

            // 스캔 쪽도 시간 기준으로. 500장마다면 숫자가 껑충 뛴다.
            let due = {
                let mut l = last_emit.lock().unwrap();
                if l.elapsed() >= std::time::Duration::from_millis(50) {
                    *l = std::time::Instant::now();
                    true
                } else {
                    false
                }
            };
            if due {
                on_progress(&progress.lock().unwrap().clone());
            }
            Some((f, m, ts, src, duration_ms))
        })
        .collect();

    // 폴더를 먼저 만들고(부모→자식 순서), 그 다음 파일을 넣는다.
    let mut dirs: Vec<&String> = rows.iter().map(|(f, _, _, _, _)| &f.rel_dir).collect();
    dirs.sort();
    dirs.dedup();

    db.transaction(|tx| {
        for d in &dirs {
            let name = Path::new(d)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.to_string());
            tx.execute(
                "INSERT INTO folders(volume_uuid,library_id,rel_path,name,area,scanned_at)
                 VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))
                 ON CONFLICT(volume_uuid,rel_path) DO UPDATE SET
                   library_id=excluded.library_id, scanned_at=excluded.scanned_at",
                rusqlite::params![vol.uuid, library_id, d, name, area],
            )?;
        }

        let mut ins = tx.prepare(
            "INSERT INTO files(folder_id,name,ext,size,kind,taken_at,taken_at_source,
                created_at,modified_at,width,height,orientation,duration_ms,
                cam_make,cam_model,lens,iso,aperture,shutter,focal_mm,
                gps_lat,gps_lon,gps_alt,inode,scanned_at)
             VALUES((SELECT id FROM folders WHERE volume_uuid=?1 AND rel_path=?2),
                ?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?25,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,
                strftime('%s','now'))
             ON CONFLICT(folder_id,name) DO UPDATE SET
                quick_hash=CASE WHEN files.size=excluded.size
                    AND files.modified_at IS excluded.modified_at THEN files.quick_hash END,
                full_hash=CASE WHEN files.size=excluded.size
                    AND files.modified_at IS excluded.modified_at THEN files.full_hash END,
                size=excluded.size, taken_at=excluded.taken_at,
                taken_at_source=excluded.taken_at_source,
                modified_at=excluded.modified_at, width=excluded.width,
                height=excluded.height, duration_ms=excluded.duration_ms,
                scanned_at=excluded.scanned_at",
        )?;

        for (f, m, ts, src, duration_ms) in &rows {
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
                duration_ms,
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
               (SELECT COUNT(*) FROM files
                 WHERE files.folder_id = folders.id AND files.trashed_at IS NULL)
             WHERE library_id = ?1",
            [library_id],
        )
    })?;
    db.write(|c| {
        c.execute(
            "UPDATE libraries SET scanned_at = strftime('%s','now') WHERE id = ?1",
            [library_id],
        )
    })?;

    let out = progress.lock().unwrap().clone();
    on_progress(&out);
    Ok(out)
}

/// 한 폴더 안에서 **디스크에 없어진** 파일의 행을 지운다. 지운 수를 돌려준다.
///
/// 스캔은 있는 것만 넣는다. 파인더에서 지운 것은 여기서 뺀다. 휴지통에 든
/// 것(`trashed_at`)은 원래 자리에 없는 게 정상이라 건드리지 않는다.
/// 썸네일 파일은 두고 행만 지운다 — 같은 파일이 돌아오면 캐시 키가 같아 그대로 쓴다.
pub fn prune_missing(
    db: &Db,
    mount: &Path,
    library_id: i64,
    rel_dir: &str,
) -> Result<usize> {
    let rows: Vec<(i64, String)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
               FROM files fi JOIN folders fo ON fo.id = fi.folder_id
              WHERE fo.library_id = ?1 AND fo.rel_path = ?2 AND fi.trashed_at IS NULL",
        )?;
        let it = st.query_map(rusqlite::params![library_id, rel_dir], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let gone: Vec<i64> = rows
        .into_iter()
        .filter(|(_, rel)| !mount.join(rel).exists())
        .map(|(id, _)| id)
        .collect();
    if gone.is_empty() {
        return Ok(0);
    }
    db.transaction(|tx| {
        let mut del = tx.prepare("DELETE FROM files WHERE id = ?1")?;
        for id in &gone {
            del.execute([id])?;
        }
        tx.execute(
            "UPDATE folders SET file_count =
               (SELECT COUNT(*) FROM files
                 WHERE files.folder_id = folders.id AND files.trashed_at IS NULL)
             WHERE library_id = ?1",
            [library_id],
        )?;
        Ok(())
    })?;
    Ok(gone.len())
}

/// 시험용 — 폴더를 라이브러리로 등록하고 곧바로 스캔한다.
///
/// 실제 흐름에서는 등록(`library_add`)과 스캔(`scan_start`)이 나뉘어 있지만,
/// 시험에서는 항상 붙어 다닌다.
#[cfg(test)]
pub fn scan_test(
    db: &Db,
    root: impl AsRef<Path>,
    area: i32,
    on_progress: impl Fn(&Progress) + Sync + Send,
) -> Result<Progress> {
    let root = root.as_ref();
    let lib = crate::db::libraries::add(db, root, area)
        .unwrap_or_else(|e| panic!("라이브러리 등록: {e}"));
    scan_folder(db, lib.id, root, area, on_progress)
}

/// 이미 DB에 있는 파일들의 (크기, 수정시각). 증분 스캔의 재료다.
fn load_known(
    db: &Db,
    library_id: i64,
) -> Result<std::collections::HashMap<(String, String), (i64, i64)>> {
    let map = db.read(|c| {
        let mut st = c.prepare(
            // 영상인데 duration_ms가 NULL이면 아직 메타데이터를 안 읽은 것이다.
            // 크기·수정시각이 그대로여도 한 번은 다시 봐야 한다.
            "SELECT fo.rel_path, fi.name, fi.size, COALESCE(fi.modified_at,0)
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fo.library_id = ?1
               AND NOT (fi.kind = 1 AND fi.duration_ms IS NULL)",
        )?;
        let rows = st.query_map([library_id], |r| {
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
/// 폴더를 훑는다. `on_found`는 찾은 수가 늘 때마다 (호출자가 솎아 쓴다).
fn walk(root: &Path, mount: &Path, mut on_found: impl FnMut(usize)) -> Vec<Found> {
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
            on_found(out.len());
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
    /// 영상 메타데이터를 아직 안 읽었으면 다시 스캔 대상이 되어야 한다.
    /// 안 그러면 이미 들어 있는 2,828개는 영영 길이도 촬영일도 못 얻는다.
    #[test]
    fn videos_without_metadata_are_rescanned_once() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x".repeat(50)).unwrap();
        std::fs::write(dir.path().join("v.mp4"), b"y".repeat(50)).unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();

        let lib: i64 = db
            .read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap();
        // 스캔이 끝나면 영상은 0(=읽어 봤지만 없음)이 찍혀 다시 대상이 되지 않는다
        let known = load_known(&db, lib).unwrap();
        assert_eq!(known.len(), 2, "둘 다 아는 파일이어야 한다");

        // 구버전에서 넘어온 것처럼 NULL로 되돌려 본다
        db.write(|c| c.execute("UPDATE files SET duration_ms=NULL WHERE kind=1", []))
            .unwrap();
        let known = load_known(&db, lib).unwrap();
        assert_eq!(known.len(), 1, "영상은 다시 읽을 대상이 된다");
    }


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
        let p = scan_test(&db, dir.path(), 0, |_| {}).unwrap();
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
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();

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

        let first = scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(first.inserted, 1);
        assert_eq!(first.skipped, 0);

        let second = scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(second.skipped, 1, "바뀌지 않았으면 건너뛴다");
        assert_eq!(second.inserted, 0);
    }

    #[test]
    fn changed_file_is_rescanned() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260101_120000.jpg");
        std::fs::write(&f, b"hello").unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();

        // 크기를 바꾸면 다시 읽어야 한다
        std::fs::write(&f, b"hello world, longer now").unwrap();
        let again = scan_test(&db, dir.path(), 0, |_| {}).unwrap();
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
        let p = scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        assert_eq!(p.inserted, 1, "시스템 폴더는 건너뛴다");
    }

    /// 파인더에서 지운 것은 스캔이 못 본다 — prune이 뺀다. 휴지통 것은 둔다.
    #[test]
    fn prune_removes_rows_whose_files_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let sub = lib_dir.join("2024");
        std::fs::create_dir_all(&sub).unwrap();
        for n in ["a.jpg", "b.jpg", "c.jpg"] {
            std::fs::write(sub.join(n), b"x").unwrap();
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        let p = scan_test(&db, &lib_dir, 1, |_| {}).unwrap();
        assert_eq!(p.inserted, 3);
        let lib: i64 = db
            .read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap();
        let mount = crate::db::volumes::describe(&lib_dir).unwrap().mount_path;
        let rel_dir: String = db
            .read(|c| c.query_row("SELECT rel_path FROM folders WHERE name='2024'", [], |r| r.get(0)))
            .unwrap();

        // c는 휴지통에 든 것처럼 — 원래 자리에 없어도 정상
        std::fs::remove_file(sub.join("b.jpg")).unwrap();
        std::fs::remove_file(sub.join("c.jpg")).unwrap();
        db.write(|c| c.execute("UPDATE files SET trashed_at = 1 WHERE name = 'c.jpg'", []))
            .unwrap();

        let n = prune_missing(&db, &mount, lib, &rel_dir).unwrap();
        assert_eq!(n, 1, "b만 지운다");
        let names: Vec<String> = db
            .read(|c| {
                let mut st = c.prepare("SELECT name FROM files ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        assert_eq!(names, vec!["a.jpg", "c.jpg"]);
        let cnt: i64 = db
            .read(|c| c.query_row("SELECT file_count FROM folders WHERE name='2024'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(cnt, 1, "폴더 장수도 맞춘다 (휴지통 것은 안 센다)");

        // 아무것도 안 사라졌으면 0
        assert_eq!(prune_missing(&db, &mount, lib, &rel_dir).unwrap(), 0);
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("db.sqlite")).unwrap();
        assert!(scan_folder(&db, 1, "/no/such/dir", 0, |_| {}).is_err());
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// `cargo test --release --lib scan::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 전체를 스캔한다"]
    fn scan_the_whole_library() {
        // 어느 라이브러리를 잴지는 ACUT_BENCH_ROOT로 준다. 없으면 옛 자리.
        let root_s = std::env::var("ACUT_BENCH_ROOT")
            .unwrap_or_else(|_| "/Volumes/MAIN SSD/MERGE/사진통합작업".into());
        let root = Path::new(&root_s);
        if !root.is_dir() {
            eprintln!("라이브러리가 없다 — 건너뜀");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path().join("acut.db")).unwrap();

        let t0 = std::time::Instant::now();
        let last = std::sync::Mutex::new(std::time::Instant::now());
        let p = scan_test(&db, root, 1, |pr| {
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

    /// 스캔 + 썸네일 전체 파이프라인.
    /// `cargo test --release --lib scan::real::full_pipeline -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 라이브러리 전체 · 수 분 걸린다"]
    fn full_pipeline() {
        // 어느 라이브러리를 잴지는 ACUT_BENCH_ROOT로 준다. 없으면 옛 자리.
        let root_s = std::env::var("ACUT_BENCH_ROOT")
            .unwrap_or_else(|_| "/Volumes/MAIN SSD/MERGE/사진통합작업".into());
        let root = Path::new(&root_s);
        if !root.is_dir() {
            eprintln!("라이브러리 없음 — 건너뜀");
            return;
        }
        // 캐시는 쓰기 가능한 임시 폴더에 만든다 (원본 볼륨을 건드리지 않는다)
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path().join("acut.db")).unwrap();

        let t0 = std::time::Instant::now();
        let p = scan_test(&db, root, 1, |_| {}).expect("스캔");
        let scan_s = t0.elapsed().as_secs_f64();
        println!("\n═══ 1단계 스캔 ═══");
        println!("  {}장 · {:.1}초 · {:.0}장/초", p.found, scan_s, p.found as f64 / scan_s);

        let vol = crate::db::volumes::describe(root).unwrap();
        let lib: i64 = db
            .read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0)))
            .unwrap();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let t1 = std::time::Instant::now();
        let last = std::sync::Mutex::new(std::time::Instant::now());
        let tp = thumbs::generate(&db, lib, &vol.mount_path, &tmp.path().join("cache"), cancel, |pr| {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_secs() >= 5 {
                eprintln!("   썸네일 {}/{} · {:.0}s", pr.done, pr.total, t1.elapsed().as_secs_f64());
                *l = std::time::Instant::now();
            }
        })
        .expect("썸네일");
        let thumb_s = t1.elapsed().as_secs_f64();

        println!("\n═══ 2단계 썸네일 ═══");
        println!("  대상 {}장 · 성공 {} · 실패 {}", tp.total, tp.done - tp.failed, tp.failed);
        println!("  {:.1}초 · {:.0}장/초 · {:.1}ms/장",
                 thumb_s, tp.total as f64 / thumb_s, thumb_s * 1000.0 / tp.total as f64);

        let (bytes, count) = crate::media::cache::cache_stats(&tmp.path().join("cache"));
        println!("  캐시 {}개 · {:.0} MB (원본 대비 {:.1}%)",
                 count, bytes as f64 / 1024.0 / 1024.0,
                 bytes as f64 / 373.5 / 1024.0 / 1024.0 / 1024.0 * 100.0);
        println!("\n  전체 {:.1}초\n", scan_s + thumb_s);
    }
}
