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
use std::collections::HashSet;
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

/// 유닉스 시각을 그 기기의 지역 날짜로.
///
/// EXIF의 시각에는 시간대가 없다. 스캐너가 그렇게 넣어 두었으니 여기서도
/// 같은 규칙으로 읽어야 날짜 폴더가 어긋나지 않는다.
fn to_day(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 1970-01-01부터의 날수를 (연, 월, 일)로. Howard Hinnant의 civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 가져올 곳을 훑는다. 하위 폴더까지 본다.
///
/// 라이브러리에 이미 있는 것은 (이름, 크기)로 가려낸다. 같은 카드를 두 번
/// 꽂아도 같은 사진이 두 벌 들어가지 않게 하려는 것이다. 다른 사진인데
/// 이름과 크기가 우연히 같을 확률은 실질적으로 없다.
pub fn look(db: &Db, source: &Path, library_id: i64) -> Result<Vec<Candidate>> {
    let known: HashSet<(String, i64)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.name, fi.size FROM files fi
               JOIN folders fo ON fo.id = fi.folder_id
              WHERE fo.library_id = ?1",
        )?;
        let it = st.query_map([library_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<rusqlite::Result<HashSet<_>>>()
    })?;

    let mut out = Vec::new();
    walk(source, &mut out);

    for c in &mut out {
        c.duplicate = known.contains(&(c.name.clone(), c.size as i64));
    }
    // 날짜 순으로 — 사람이 훑을 때 카드에 담긴 순서보다 이쪽이 읽힌다
    out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<Candidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = crate::scan::nfc(&e.file_name().to_string_lossy());
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if !is_skipped_dir(&name) {
                walk(&e.path(), out);
            }
            continue;
        }
        if !ft.is_file() || classify(&name).is_none() {
            continue;
        }
        // exFAT은 파일마다 `._`로 시작하는 곁가지를 만든다. 사진이 아니다.
        if name.starts_with("._") {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let path = e.path();
        let taken = crate::media::exif::read(&path)
            .and_then(|m| m.taken_at)
            .or_else(|| {
                md.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
            })
            .unwrap_or(0);
        out.push(Candidate {
            name,
            size: md.len(),
            day: to_day(taken),
            duplicate: false,
            path,
        });
    }
}

/// 미리 보기 — 몇 장이 어느 날짜로 들어가는지.
pub fn preview(db: &Db, source: &Path, library_id: i64) -> Result<Preview> {
    let cands = look(db, source, library_id)?;
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
    source: &Path,
    library_id: i64,
    on_progress: impl Fn(&Progress),
) -> Result<(Report, Vec<PathBuf>)> {
    let cands = look(db, source, library_id)?;
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

    let batch_id = super::open_batch(db, "import", &format!("{} 가져오기", source.display()))?;
    let mut rep = Report { batch_id, ..Default::default() };
    let mut p = Progress { found: cands.len(), ..Default::default() };
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
        assert_eq!(to_day(0), "1970-01-01");
        // 2024-08-27 00:00:00 UTC
        assert_eq!(to_day(1_724_716_800), "2024-08-27");
        // 윤년 2월 29일
        assert_eq!(to_day(1_709_164_800), "2024-02-29");
    }

    /// 1970 이전(스캔 못 한 옛 사진)도 하루 앞으로 밀리면 안 된다
    #[test]
    fn dates_before_the_epoch_do_not_slip() {
        assert_eq!(to_day(-1), "1969-12-31");
        assert_eq!(to_day(-86_400), "1969-12-31");
        assert_eq!(to_day(-86_401), "1969-12-30");
    }

    /// 카드에 담긴 여러 날이 날짜별로 갈라져 들어가야 한다. 가져온 날로
    /// 묶으면 한 폴더에 뭉친다.
    #[test]
    fn photos_land_in_folders_by_the_day_they_were_taken() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"a", 1_724_716_800); // 2024-08-27
        put(&src, "IMG_2.jpg", b"bb", 1_724_716_800);
        put(&src, "IMG_3.jpg", b"ccc", 1_709_164_800); // 2024-02-29

        let (rep, dirs) = copy_in(&db, &src, lib, |_| {}).unwrap();
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
        copy_in(&db, &src, lib, |_| {}).unwrap();
        assert!(src.join("IMG_1.jpg").is_file(), "카드에 그대로 있어야 한다");
    }

    /// 같은 카드를 두 번 꽂아도 두 벌이 들어가면 안 된다.
    #[test]
    fn a_file_already_in_the_library_is_skipped() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"abc", 1_724_716_800);

        // 라이브러리에 이미 있는 것처럼 꾸민다 (이름·크기가 열쇠다)
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area)
                 SELECT 1, volume_uuid, ?1, '2024/2024-08-27', '2024-08-27', 1
                   FROM libraries WHERE id = ?1",
                [lib],
            )?;
            tx.execute(
                "INSERT INTO files(folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,'IMG_1.jpg',3,0,0,0,0)",
                [],
            )
        })
        .unwrap();

        let (rep, dirs) = copy_in(&db, &src, lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 0);
        assert_eq!(rep.skipped, 1);
        assert!(dirs.is_empty(), "건드린 폴더가 없어야 한다");
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

        let (rep, _) = copy_in(&db, &src, lib, |_| {}).unwrap();
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

        let (rep, _) = copy_in(&db, &src, lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    /// 하위 폴더까지 훑는다. 카드는 보통 `DCIM/100APPLE` 꼴이다.
    #[test]
    fn subfolders_are_walked() {
        let (_d, db, src, lib) = setup();
        let deep = src.join("DCIM/100APPLE");
        std::fs::create_dir_all(&deep).unwrap();
        put(&deep, "IMG_1.jpg", b"a", 1_724_716_800);

        let (rep, _) = copy_in(&db, &src, lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 1);
    }

    #[test]
    fn preview_counts_what_would_come_in() {
        let (_d, db, src, lib) = setup();
        put(&src, "IMG_1.jpg", b"ab", 1_724_716_800);
        put(&src, "IMG_2.jpg", b"cd", 1_709_164_800);

        let p = preview(&db, &src, lib).unwrap();
        assert_eq!(p.files, 2);
        assert_eq!(p.bytes, 4);
        assert_eq!(p.duplicates, 0);
        assert_eq!(p.day_count, 2);
        assert_eq!(p.days, vec!["2024-02-29", "2024-08-27"]);
    }

    #[test]
    fn an_empty_card_is_not_an_error() {
        let (_d, db, src, lib) = setup();
        let (rep, dirs) = copy_in(&db, &src, lib, |_| {}).unwrap();
        assert_eq!(rep.copied, 0);
        assert_eq!(rep.failed, 0);
        assert!(dirs.is_empty());
    }
}
