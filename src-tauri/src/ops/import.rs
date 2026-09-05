//! 가져오기 — 카드나 다른 폴더의 사진을 라이브러리로 들인다.
//!
//! **복사만 한다. 원본은 건드리지 않는다.** 가져오는 곳은 보통 SD 카드고,
//! 옮기다 무슨 일이 생기면 그 한 벌이 마지막 벌이었을 수 있다. 지우는 것은
//! 사람이 파인더에서 직접 할 일이다.
//!
//! 들어갈 자리는 **찍은 날**이다 (`2024/2024-08-27`). 가져온 날로 묶으면
//! 한 카드에 든 여러 날이 한 폴더에 뭉친다. 이름 붙이기는 나중에 「정리」가
//! 맡는다 — 여기서는 날짜까지만 갈라 둔다.
//!
//! 복사한 뒤에는 그 폴더만 다시 스캔한다. EXIF·해상도·썸네일을 여기서 또
//! 만들지 않기 위해서다. 스캐너는 이미 아는 파일을 건너뛰므로 새로 들어온
//! 것만 읽는다.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use crate::ops::trash::free_path;
use crate::scan::kinds::{classify, is_skipped_dir};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 가져올 후보 한 장.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    /// 찍은 날 (`2024-08-27`). 못 읽으면 파일 시각으로 갈음한다.
    pub day: String,
    /// 이미 라이브러리에 같은 것이 있는가
    pub duplicate: bool,
}

/// 시작하기 전에 보여 줄 것.
#[derive(Debug, Default, serde::Serialize)]
pub struct Preview {
    pub files: usize,
    pub bytes: u64,
    /// 이미 있는 것 (건너뛴다)
    pub duplicates: usize,
    /// 들어갈 날짜 폴더들 — 앞의 몇 개만
    pub days: Vec<String>,
    /// 서로 다른 날이 몇 개인가
    pub day_count: usize,
}

#[derive(Debug, Default, serde::Serialize, Clone)]
pub struct Report {
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
    pub bytes: u64,
    pub first_error: Option<String>,
    pub batch_id: i64,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Progress {
    pub found: usize,
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 지금 복사하고 있는 파일
    pub current: String,
}

/// 하루 폴더까지의 상대 경로 — `2024/2024-08-27`.
pub fn day_dir(day: &str) -> String {
    match day.get(0..4) {
        Some(y) if y.len() == 4 => format!("{y}/{day}"),
        _ => day.to_string(),
    }
}

/// 유닉스 시각을 이 기기의 지역 날짜로. 스캐너·UI·SQLite 필터가 모두 같은
/// 시간대 규칙을 써야 자정 근처 사진이 다른 날 폴더로 가지 않는다.
fn to_day(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

type KnownMap = HashMap<(String, i64), Vec<(String, Option<String>)>>;

/// 가져올 것들을 훑는다. 폴더는 하위까지, 파일은 그것만.
///
/// 파인더에서 끌어다 놓으면 파일 몇 개나 폴더가 섞여 온다 — 둘 다 받는다.
/// 같은 이름·크기의 후보가 있으면 전체 내용 해시까지 비교한다. 카메라는
/// `IMG_0001` 같은 이름을 되풀이하므로 이름·크기만으로 건너뛰면 사진을 잃는다.
pub fn look(db: &Db, sources: &[PathBuf], library_id: i64) -> Result<Vec<Candidate>> {
    let mount = libraries::get(db, library_id)?
        .and_then(|l| crate::db::volumes::find_mount(&l.volume_uuid));
    let known: KnownMap = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.name, fi.size, fo.rel_path, fi.full_hash FROM files fi
               JOIN folders fo ON fo.id = fi.folder_id
              WHERE fo.library_id = ?1 AND fi.trashed_at IS NULL",
        )?;
        let it = st.query_map([library_id], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, i64>(1)?),
                (r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?),
            ))
        })?;
        let mut out: KnownMap = HashMap::new();
        for row in it {
            let (key, file) = row?;
            out.entry(key).or_default().push(file);
        }
        Ok(out)
    })?;

    let mut out = Vec::new();
    for src in sources {
        if src.is_dir() {
            walk(src, &mut out);
        } else if src.is_file() {
            if let Some(c) = candidate(src) {
                out.push(c);
            }
        }
    }
    // 같은 파일을 두 번 끌어다 놓아도 한 번만
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);

    for c in &mut out {
        let Some(matches) = known.get(&(c.name.clone(), c.size as i64)) else {
            continue;
        };
        let candidate_hash = crate::core::hasher::xxhash_file(&c.path);
        // 저장된 full_hash 는 SHA-256 이다 — xxhash 와 문자열 비교하면 절대 같지 않아
        // 디스크를 못 읽는 사본이 전부 «새 사진»이 된다 (리뷰 2026-08-31). 그때만 후보를
        // SHA-256 으로 한 번 더 읽어 같은 잣대로 비교한다.
        let mut candidate_sha: Option<Option<String>> = None;
        c.duplicate = matches.iter().any(|(rel, stored_hash)| {
            if let (Some(m), Some(cand)) = (mount.as_ref(), candidate_hash.as_ref()) {
                if let Some(known_hash) =
                    crate::core::hasher::xxhash_file(&m.join(rel).join(&c.name))
                {
                    return *cand == known_hash;
                }
            }
            let Some(stored) = stored_hash else {
                return false;
            };
            candidate_sha
                .get_or_insert_with(|| crate::cull::hash::full(&c.path).ok())
                .as_deref()
                == Some(stored.as_str())
        });
    }
    // 날짜 순으로 — 사람이 훑을 때 카드에 담긴 순서보다 이쪽이 읽힌다
    out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<Candidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = crate::scan::nfc(&e.file_name().to_string_lossy());
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if !is_skipped_dir(&name) {
                walk(&e.path(), out);
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if let Some(c) = candidate(&e.path()) {
            out.push(c);
        }
    }
}

/// 파일 하나를 후보로. 사진이 아니거나 곁가지면 None.
fn candidate(path: &Path) -> Option<Candidate> {
    let name = crate::scan::nfc(&path.file_name()?.to_string_lossy());
    classify(&name)?;
    // exFAT은 파일마다 `._`로 시작하는 곁가지를 만든다. 사진이 아니다.
    if name.starts_with("._") {
        return None;
    }
    let md = std::fs::metadata(path).ok()?;
    let taken = crate::media::exif::read(path)
        .and_then(|m| m.taken_at)
        .or_else(|| {
            md.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        })
        .unwrap_or(0);
    Some(Candidate {
        name,
        size: md.len(),
        day: to_day(taken),
        duplicate: false,
        path: path.to_path_buf(),
    })
}

/// 미리 보기 — 몇 장이 어느 날짜로 들어가는지.
pub fn preview(db: &Db, sources: &[PathBuf], library_id: i64) -> Result<Preview> {
    let cands = look(db, sources, library_id)?;
    let mut days: Vec<String> = cands
        .iter()
        .filter(|c| !c.duplicate)
        .map(|c| c.day.clone())
        .collect();
    days.sort();
    days.dedup();
    Ok(Preview {
        files: cands.iter().filter(|c| !c.duplicate).count(),
        bytes: cands.iter().filter(|c| !c.duplicate).map(|c| c.size).sum(),
        duplicates: cands.iter().filter(|c| c.duplicate).count(),
        day_count: days.len(),
        days: days.into_iter().take(8).collect(),
    })
}

/// 실제로 복사한다. 복사한 자리의 볼륨 기준 경로들을 함께 돌려준다.
///
/// DB에 넣는 일은 하지 않는다 — 부르는 쪽이 그 폴더만 다시 스캔한다.
pub fn copy_in(
    db: &Db,
    sources: &[PathBuf],
    library_id: i64,
    on_progress: impl Fn(&Progress),
) -> Result<(Report, Vec<PathBuf>)> {
    let cands = look(db, sources, library_id)?;
    let Some(lib) = libraries::get(db, library_id)? else {
        return Ok((
            Report {
                first_error: Some("등록되지 않은 라이브러리입니다".into()),
                ..Default::default()
            },
            Vec::new(),
        ));
    };
    let Some(lib_dir) = lib.dir.clone() else {
        return Ok((
            Report {
                first_error: Some("디스크가 연결되어 있지 않습니다".into()),
                ..Default::default()
            },
            Vec::new(),
        ));
    };

    let label = match sources {
        [one] => format!("{} 가져오기", one.display()),
        many => format!("{}곳에서 가져오기", many.len()),
    };
    let batch_id = super::open_batch(db, "import", &label)?;
    let mut rep = Report {
        batch_id,
        ..Default::default()
    };
    let mut p = Progress {
        found: cands.len(),
        ..Default::default()
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    on_progress(&p);

    for c in &cands {
        if c.duplicate {
            rep.skipped += 1;
            p.skipped += 1;
            continue;
        }
        let dir = lib_dir.join(day_dir(&c.day));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            rep.failed += 1;
            p.failed += 1;
            rep.first_error.get_or_insert(e.to_string());
            continue;
        }
        if !dirs.contains(&dir) {
            dirs.push(dir.clone());
        }

        let dest = free_path(dir.join(&c.name));
        p.current = c.name.clone();
        match std::fs::copy(&c.path, &dest) {
            Ok(n) => {
                rep.copied += 1;
                rep.bytes += n;
                p.copied += 1;
            }
            Err(e) => {
                // 반쯤 쓰다 만 파일은 남기지 않는다. 다음 스캔이 그걸 사진으로 본다.
                let _ = std::fs::remove_file(&dest);
                rep.failed += 1;
                p.failed += 1;
                rep.first_error.get_or_insert(e.to_string());
            }
        }
        on_progress(&p);
    }

    Ok((rep, dirs))
}

/// 방금 들어온 파일들을 배치에 적어 둔다 — 되돌리기가 이걸 본다.
///
/// 복사가 끝나고 스캔이 끝난 다음에 부른다. 그래야 file_id가 있다.
pub fn record_imported(db: &Db, batch_id: i64, library_id: i64, since: i64) -> Result<usize> {
    let rows: Vec<(i64, String, String)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.volume_uuid,
                    fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
               FROM files fi JOIN folders fo ON fo.id = fi.folder_id
              WHERE fo.library_id = ?1 AND fi.scanned_at >= ?2",
        )?;
        let it = st.query_map(rusqlite::params![library_id, since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    for (id, vol, rel) in &rows {
        super::record(db, batch_id, "import", *id, vol, rel, Some(rel), Ok(()))?;
    }
    super::close_batch(db, batch_id, rows.len())?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 라이브러리 하나가 등록된 빈 DB와 «가져올 폴더» 하나.
    fn setup() -> (tempfile::TempDir, Db, PathBuf, i64) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        let lib_dir = dir.path().join("라이브러리");
        let src = dir.path().join("카드");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        let lib = libraries::add(&db, &lib_dir, 1).unwrap();
        (dir, db, src, lib.id)
    }

    /// EXIF가 없는 파일이라 수정시각이 촬영일로 쓰인다. 그 시각을 못으로 박아
    /// 어느 날짜 폴더로 갈지 정한다.
    fn put(dir: &Path, name: &str, bytes: &[u8], mtime: i64) {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        let t = filetime::FileTime::from_unix_time(mtime, 0);
        filetime::set_file_mtime(&p, t).unwrap();
    }

    #[test]
    fn days_become_year_folders() {
        assert_eq!(day_dir("2024-08-27"), "2024/2024-08-27");
        assert_eq!(day_dir("1999-01-01"), "1999/1999-01-01");
    }

    /// 날짜를 못 읽은 것까지 「2024/」 밑으로 넣으면 엉뚱한 해에 섞인다
    #[test]
    fn a_broken_day_stays_as_is() {
        assert_eq!(day_dir(""), "");
        assert_eq!(day_dir("몰라"), "몰라");
    }

    #[test]
    fn unix_time_becomes_a_local_looking_day() {
        assert_eq!(
            to_day(crate::media::taken_at::civil_to_unix(2024, 8, 27, 0, 0, 0)),
            "2024-08-27"
        );
        assert_eq!(
            to_day(crate::media::taken_at::civil_to_unix(
                2024, 2, 29, 23, 59, 59
            )),
            "2024-02-29"
        );
    }

    /// 1970 이전(스캔 못 한 옛 사진)도 하루 앞으로 밀리면 안 된다
    #[test]
    fn dates_before_the_epoch_do_not_slip() {
        assert_eq!(
            to_day(crate::media::taken_at::civil_to_unix(
                1969, 12, 31, 23, 59, 59
            )),
            "1969-12-31"
        );
    }

    /// 카드에 담긴 여러 날이 날짜별로 갈라져 들어가야 한다. 가져온 날로
    /// 묶으면 한 폴더에 뭉친다.
    #[test]
    fn photos_land_in_folders_by_the_day_they_were_taken() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800); // 2024-08-27
        put(&src, "IMG_2.jpg", b"bb", 1_724_716_800);
        put(&src, "IMG_3.jpg", b"ccc", 1_709_164_800); // 2024-02-29

        let (rep, dirs) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 3);
        assert_eq!(rep.failed, 0);
        assert_eq!(rep.bytes, 6);

        let lib_dir = libraries::get(&db, lib).unwrap().unwrap().dir.unwrap();
        assert!(lib_dir.join("2024/2024-08-27/IMG_1.jpg").is_file());
        assert!(lib_dir.join("2024/2024-08-27/IMG_2.jpg").is_file());
        assert!(lib_dir.join("2024/2024-02-29/IMG_3.jpg").is_file());
        assert_eq!(dirs.len(), 2, "날짜 폴더 둘만 건드려야 한다");
    }

    /// **원본은 건드리지 않는다.** 가져오는 곳은 보통 SD 카드고, 옮기다
    /// 무슨 일이 생기면 그 한 벌이 마지막 벌이었을 수 있다.
    #[test]
    fn the_source_is_left_alone() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800);
        copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert!(src.join("IMG_1.jpg").is_file(), "카드에 그대로 있어야 한다");
    }

    /// 같은 카드를 두 번 꽂아도 두 벌이 들어가면 안 된다.
    #[test]
    fn a_file_already_in_the_library_is_skipped() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"abc", 1_724_716_800);

        let library = libraries::get(&db, lib).unwrap().unwrap();
        let existing_dir = library.dir.clone().unwrap().join("2024/2024-08-27");
        std::fs::create_dir_all(&existing_dir).unwrap();
        std::fs::write(existing_dir.join("IMG_1.jpg"), b"abc").unwrap();
        let rel = format!("{}/2024/2024-08-27", library.rel_path)
            .trim_start_matches('/')
            .to_string();

        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area)
                 SELECT 1, volume_uuid, ?1, ?2, '2024-08-27', 1
                   FROM libraries WHERE id = ?1",
                rusqlite::params![lib, rel],
            )?;
            tx.execute(
                "INSERT INTO files(folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,'IMG_1.jpg',3,0,0,0,0)",
                [],
            )
        })
        .unwrap();

        let (rep, dirs) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 0);
        assert_eq!(rep.skipped, 1);
        assert!(dirs.is_empty(), "건드린 폴더가 없어야 한다");
    }

    /// 디스크의 기존 사본을 못 읽어도(폴더 이동·디스크 없음) 저장된 SHA-256 으로 거른다
    #[test]
    fn a_missing_disk_copy_still_matches_by_its_stored_hash() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"abc", 1_724_716_800);
        let sha = crate::cull::hash::full(src.join("IMG_1.jpg")).unwrap();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area)
                 SELECT 1, volume_uuid, ?1, 'gone/2024-08-27', '2024-08-27', 1 FROM libraries WHERE id = ?1",
                [lib],
            )?;
            tx.execute(
                "INSERT INTO files(folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,full_hash)
                 VALUES(1,'IMG_1.jpg',3,0,0,0,0,?1)",
                [&sha],
            )
        })
        .unwrap();
        let p = preview(&db, std::slice::from_ref(&src), lib).unwrap();
        assert_eq!(
            (p.files, p.duplicates),
            (0, 1),
            "디스크에 없어도 저장된 해시로 같은 사진임을 안다"
        );
    }

    #[test]
    fn same_name_and_size_but_different_content_is_not_a_duplicate() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"new", 1_724_716_800);
        let library = libraries::get(&db, lib).unwrap().unwrap();
        let existing_dir = library.dir.clone().unwrap().join("2024/2024-08-27");
        std::fs::create_dir_all(&existing_dir).unwrap();
        std::fs::write(existing_dir.join("IMG_1.jpg"), b"old").unwrap();
        let rel = format!("{}/2024/2024-08-27", library.rel_path)
            .trim_start_matches('/')
            .to_string();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area)
                 SELECT 1, volume_uuid, ?1, ?2, '2024-08-27', 1 FROM libraries WHERE id = ?1",
                rusqlite::params![lib, rel],
            )?;
            tx.execute(
                "INSERT INTO files(folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,'IMG_1.jpg',3,0,0,0,0)",
                [],
            )
        })
        .unwrap();

        let p = preview(&db, std::slice::from_ref(&src), lib).unwrap();
        assert_eq!((p.files, p.duplicates), (1, 0));
        let (report, _) = copy_in(&db, &[src], lib, |_| {}).unwrap();
        assert_eq!((report.copied, report.skipped), (1, 0));
    }

    /// 이름이 같아도 내용이 다르면 다른 사진이다 — 덮어쓰면 한 장이 사라진다.
    #[test]
    fn a_name_clash_does_not_overwrite() {
        let (_d, db, src, lib) = setup();
        let a = src.join("A");
        let b = src.join("B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        put(&a, "IMG_1.jpg", b"first", 1_724_716_800);
        put(&b, "IMG_1.jpg", b"second!!", 1_724_716_800);

        let (rep, _) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 2);

        let day = libraries::get(&db, lib)
            .unwrap()
            .unwrap()
            .dir
            .unwrap()
            .join("2024/2024-08-27");
        let mut sizes: Vec<u64> = std::fs::read_dir(&day)
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().len())
            .collect();
        sizes.sort();
        assert_eq!(sizes, vec![5, 8], "둘 다 온전히 남아야 한다");
    }

    /// 사진이 아닌 것과 exFAT 곁가지(`._`)는 들이지 않는다.
    #[test]
    fn only_media_comes_in() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800);
        put(&src, "메모.txt", b"x", 1_724_716_800);
        put(&src, "._IMG_1.jpg", b"y", 1_724_716_800);

        let (rep, _) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    /// 하위 폴더까지 훑는다. 카드는 보통 `DCIM/100APPLE` 꼴이다.
    #[test]
    fn subfolders_are_walked() {
        let (_d, db, src, lib) = setup();
        let deep = src.join("DCIM/100APPLE");
        std::fs::create_dir_all(&deep).unwrap();
        put(&deep, "IMG_1.jpg", b"a", 1_724_716_800);

        let (rep, _) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    #[test]
    fn preview_counts_what_would_come_in() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"ab", 1_724_716_800);
        put(&src, "IMG_2.jpg", b"cd", 1_709_164_800);

        let p = preview(&db, std::slice::from_ref(&src), lib).unwrap();
        assert_eq!(p.files, 2);
        assert_eq!(p.bytes, 4);
        assert_eq!(p.duplicates, 0);
        assert_eq!(p.day_count, 2);
        assert_eq!(p.days, vec!["2024-02-29", "2024-08-27"]);
    }

    /// 파인더에서 파일 하나만 끌어다 놓으면 그것만 들어온다 — 옆의 것은 아니다.
    #[test]
    fn a_dropped_file_brings_only_itself() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800);
        put(&src, "IMG_2.jpg", b"b", 1_724_716_800);
        let (rep, _) = copy_in(&db, &[src.join("IMG_1.jpg")], lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    /// 같은 것을 두 번 놓아도(파일 + 그 폴더) 한 번만
    #[test]
    fn overlapping_sources_are_deduplicated() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800);
        let (rep, _) = copy_in(&db, &[src.join("IMG_1.jpg"), src.clone()], lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    #[test]
    fn an_empty_card_is_not_an_error() {
        let (_d, db, src, lib) = setup();
        let (rep, dirs) = copy_in(&db, std::slice::from_ref(&src), lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 0);
        assert_eq!(rep.failed, 0);
        assert!(dirs.is_empty());
    }
}
