//! 잡동사니 걸러내기 — 사람이 볼 필요 없는 것들.
//!
//! 1차 구역 18,049개 실측: 스크린샷 1,105 · 아주 작은 파일 263 · 다운로드 118 ·
//! 카톡 저장본 19. 폰 사진에서 이런 게 꾸준히 섞여 들어온다.
//!
//! **규칙만 쓴다.** 파일을 열지 않으므로 6만 장이 순식간에 끝난다. 대신 확실한
//! 것만 잡는다 — 애매하면 넘긴다. 잘못 제외하는 것이 놓치는 것보다 나쁘다.

use crate::db::conn::{Db, Result};

/// 잡동사니로 볼 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Screenshot,
    /// 사진이라기엔 너무 작다 (아이콘·썸네일 조각 등)
    TooSmall,
    /// 브라우저·메신저가 받아 둔 것
    Downloaded,
    KakaoTalk,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::Screenshot => "스크린샷",
            Reason::TooSmall => "너무 작은 파일",
            Reason::Downloaded => "다운로드본",
            Reason::KakaoTalk => "카톡 저장본",
        }
    }
}

/// 사진으로 보기엔 작은 크기. 60KB 미만은 아이콘·썸네일 조각일 가능성이 높다.
pub const TINY_BYTES: i64 = 60 * 1024;

/// 파일명과 경로로 판정한다. 확실하지 않으면 None.
pub fn classify(name: &str, rel_dir: &str, size: i64) -> Option<Reason> {
    let n = name.to_lowercase();
    let d = rel_dir.to_lowercase();

    if n.starts_with("screenshot")
        || n.starts_with("screen shot")
        || n.starts_with("스크린샷")
        || d.contains("screenshot")
        || d.contains("스크린샷")
    {
        return Some(Reason::Screenshot);
    }
    if n.starts_with("kakaotalk") || d.contains("kakaotalk") {
        return Some(Reason::KakaoTalk);
    }
    // 경로에 다운로드 폴더가 들어 있으면
    if d.split('/').any(|seg| seg == "download" || seg == "downloads" || seg == "다운로드") {
        return Some(Reason::Downloaded);
    }
    if size > 0 && size < TINY_BYTES {
        return Some(Reason::TooSmall);
    }
    None
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct JunkProgress {
    pub scanned: usize,
    pub found: usize,
    pub bytes: i64,
    /// 사유별 개수 — UI에서 "스크린샷 1,105" 식으로 보여준다
    pub by_reason: Vec<(String, usize)>,
}

/// 잡동사니를 찾아 `groups`(kind=1)에 사유별로 묶는다.
pub fn scan(db: &Db) -> Result<JunkProgress> {
    let rows: Vec<(i64, String, String, i64)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fi.name, fo.rel_path, fi.size
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id",
        )?;
        let it = st.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let scanned = rows.len();
    let mut hits: Vec<(Reason, i64, i64)> = Vec::new(); // (사유, id, size)
    for (id, name, dir, size) in rows {
        if let Some(r) = classify(&name, &dir, size) {
            hits.push((r, id, size));
        }
    }

    let mut by_reason: std::collections::HashMap<&'static str, (usize, i64, Vec<i64>)> =
        Default::default();
    for (r, id, size) in &hits {
        let e = by_reason.entry(r.label()).or_default();
        e.0 += 1;
        e.1 += size;
        e.2.push(*id);
    }

    db.transaction(|tx| {
        tx.execute("DELETE FROM groups WHERE kind = 1", [])?;
        let mut ins_g = tx.prepare(
            "INSERT INTO groups(kind, reason, size_bytes, state, created_at)
             VALUES(1, ?1, ?2, 0, strftime('%s','now'))",
        )?;
        let mut ins_m =
            tx.prepare("INSERT INTO group_members(group_id, file_id, is_best) VALUES(?1,?2,0)")?;
        for (label, (_, bytes, ids)) in &by_reason {
            ins_g.execute(rusqlite::params![label, bytes])?;
            let gid = tx.last_insert_rowid();
            for id in ids {
                ins_m.execute(rusqlite::params![gid, id])?;
            }
        }
        Ok(())
    })?;

    let mut list: Vec<(String, usize)> =
        by_reason.iter().map(|(k, v)| (k.to_string(), v.0)).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(JunkProgress {
        scanned,
        found: hits.len(),
        bytes: hits.iter().map(|(_, _, s)| s).sum(),
        by_reason: list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: i64 = 3 * 1024 * 1024;

    #[test]
    fn catches_screenshots_by_name() {
        assert_eq!(classify("Screenshot_20260101-120000.png", "", BIG), Some(Reason::Screenshot));
        assert_eq!(classify("Screen Shot 2026.png", "", BIG), Some(Reason::Screenshot));
        assert_eq!(classify("스크린샷 2026-01-01.png", "", BIG), Some(Reason::Screenshot));
    }

    #[test]
    fn catches_screenshots_by_folder() {
        assert_eq!(classify("a.jpg", "DCIM/Screenshots", BIG), Some(Reason::Screenshot));
    }

    #[test]
    fn catches_kakao_and_downloads() {
        assert_eq!(classify("KakaoTalk_20260101.jpg", "", BIG), Some(Reason::KakaoTalk));
        assert_eq!(classify("a.jpg", "Download", BIG), Some(Reason::Downloaded));
        assert_eq!(classify("a.jpg", "내폰/다운로드/2026", BIG), Some(Reason::Downloaded));
    }

    #[test]
    fn catches_tiny_files() {
        assert_eq!(classify("a.jpg", "", 1000), Some(Reason::TooSmall));
        assert_eq!(classify("a.jpg", "", TINY_BYTES - 1), Some(Reason::TooSmall));
        assert_eq!(classify("a.jpg", "", TINY_BYTES), None, "경계값은 남긴다");
    }

    #[test]
    fn leaves_ordinary_photos_alone() {
        assert_eq!(classify("20260101_120000.jpg", "2026/2026-01-01 여행", BIG), None);
        assert_eq!(classify("DSC_0031.JPG", "2018", BIG), None);
        assert_eq!(classify("IMG_0075.CR2", "2018/출사", BIG), None);
        // "download"가 폴더명의 일부일 뿐이면 걸리지 않는다
        assert_eq!(classify("a.jpg", "downloaded-photos", BIG), None);
    }

    #[test]
    fn groups_by_reason() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Screenshot_20260101-120000.png"), vec![1u8; BIG as usize])
            .unwrap();
        std::fs::write(dir.path().join("Screenshot_20260102-120000.png"), vec![2u8; BIG as usize])
            .unwrap();
        std::fs::write(dir.path().join("tiny.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("20260101_120000.jpg"), vec![3u8; BIG as usize]).unwrap();

        let db = crate::db::conn::Db::open(dir.path().join("t.db")).unwrap();
        crate::scan::scan_test(&db, dir.path(), 1, |_| {}).unwrap();

        let p = scan(&db).unwrap();
        assert_eq!(p.found, 3, "스크린샷 2 + 작은 파일 1");
        assert_eq!(p.scanned, 4);

        let groups: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM groups WHERE kind=1", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(groups, 2, "사유별로 묶인다");

        // 평범한 사진은 어느 그룹에도 없어야 한다
        let ordinary: i64 = db
            .read(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM group_members gm JOIN files f ON f.id=gm.file_id
                     WHERE f.name='20260101_120000.jpg'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(ordinary, 0);
    }
}
