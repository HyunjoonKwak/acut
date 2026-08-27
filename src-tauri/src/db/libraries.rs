//! 등록한 사진 폴더 — UI에서 말하는 "라이브러리".
//!
//! 하나의 카탈로그(=이 DB) 안에 여러 개가 들어간다. 서로 다른 디스크에 있어도
//! 된다. 이 층이 하는 일은 딱 하나다: **파일 하나를 받아 그 파일의 원본 경로와
//! 썸네일 캐시 폴더를 알아내는 것.**
//!
//! 이게 없던 시절에는 "지금 열린 폴더" 하나의 캐시만 봤다. 그래서 다른 디스크
//! 사진은 목록에 나오는데 썸네일은 빈 칸이고 크게 보기는 실패했다.

use crate::db::conn::{Db, Result};
use crate::db::volumes;
use crate::media::cache;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Library {
    pub id: i64,
    pub volume_uuid: String,
    pub volume_name: String,
    /// 볼륨 안에서 이 라이브러리까지. 볼륨 최상단이면 빈 문자열.
    pub rel_path: String,
    pub name: String,
    pub area: i32,
    /// 지금 이 디스크가 꽂혀 있는가
    pub online: bool,
    /// 꽂혀 있을 때의 실제 경로. 오프라인이면 None.
    pub dir: Option<PathBuf>,
    pub file_count: i64,
}

/// 볼륨 마운트 지점에 라이브러리 상대경로를 더한다.
pub fn dir_of(volume_uuid: &str, rel_path: &str) -> Option<PathBuf> {
    let mount = volumes::find_mount(volume_uuid)?;
    let dir = if rel_path.is_empty() {
        mount
    } else {
        mount.join(rel_path)
    };
    dir.is_dir().then_some(dir)
}

/// 썸네일·미리보기 캐시가 있는 곳. 라이브러리 폴더 안(`.acut`)이다.
pub fn cache_root_of(volume_uuid: &str, rel_path: &str) -> Option<PathBuf> {
    dir_of(volume_uuid, rel_path).map(|d| cache::cache_root(&d))
}

/// 등록된 것 전부. 디스크가 빠진 것도 포함한다 — 목록에서 사라지면 안 된다.
pub fn list(db: &Db) -> Result<Vec<Library>> {
    let rows: Vec<(i64, String, String, String, String, i32, i64)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT l.id, l.volume_uuid, COALESCE(v.name, l.volume_uuid), l.rel_path,
                    l.name, l.area,
                    (SELECT COUNT(*) FROM files fi
                       JOIN folders fo ON fo.id = fi.folder_id
                      WHERE fo.library_id = l.id)
             FROM libraries l
             LEFT JOIN volumes v ON v.uuid = l.volume_uuid
             ORDER BY l.added_at",
        )?;
        let it = st.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    Ok(rows
        .into_iter()
        .map(
            |(id, volume_uuid, volume_name, rel_path, name, area, file_count)| {
                let dir = dir_of(&volume_uuid, &rel_path);
                Library {
                    id,
                    volume_uuid,
                    volume_name,
                    rel_path,
                    name,
                    area,
                    online: dir.is_some(),
                    dir,
                    file_count,
                }
            },
        )
        .collect())
}

pub fn get(db: &Db, id: i64) -> Result<Option<Library>> {
    Ok(list(db)?.into_iter().find(|l| l.id == id))
}

/// 파일이 속한 라이브러리. 프로토콜 핸들러가 경로를 풀 때 쓴다.
pub fn of_file(db: &Db, file_id: i64) -> Result<Option<(Library, String)>> {
    let found: Option<(i64, String)> = db.read(|c| {
        use rusqlite::OptionalExtension;
        c.query_row(
            "SELECT fo.library_id, fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.id = ?1 AND fo.library_id IS NOT NULL",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })?;
    let Some((lib_id, vol_rel)) = found else {
        return Ok(None);
    };
    Ok(get(db, lib_id)?.map(|l| (l, vol_rel)))
}

/// 폴더를 라이브러리로 등록한다. 이미 있으면 그대로 돌려준다.
///
/// 겹치는 등록은 막는다. 한 폴더가 두 라이브러리에 속하면 `folders.library_id`가
/// 어느 쪽인지 정해지지 않아 캐시 경로가 흔들린다.
pub fn add(db: &Db, dir: &std::path::Path, area: i32) -> std::result::Result<Library, String> {
    let v = volumes::describe(dir).map_err(|e| e.to_string())?;
    let rel = rel_within(&v.mount_path, dir)
        .ok_or_else(|| format!("볼륨 안의 경로가 아닙니다: {}", dir.display()))?;

    for other in list(db).map_err(|e| e.to_string())? {
        if other.volume_uuid != v.uuid {
            continue;
        }
        if overlaps(&other.rel_path, &rel) && other.rel_path != rel {
            return Err(format!(
                "이미 등록한 「{}」와 겹칩니다. 한쪽을 지우고 다시 등록하세요.",
                other.name
            ));
        }
    }

    let name = if rel.is_empty() {
        v.name.clone()
    } else {
        rel.rsplit('/').next().unwrap_or(&rel).to_string()
    };

    db.write(|c| {
        c.execute(
            "INSERT INTO volumes(uuid,name,last_mount_path,role,total_bytes,free_bytes,is_online,last_seen_at)
             VALUES(?1,?2,?3,'library',?4,?5,1,strftime('%s','now'))
             ON CONFLICT(uuid) DO UPDATE SET
               name=excluded.name, last_mount_path=excluded.last_mount_path,
               is_online=1, last_seen_at=excluded.last_seen_at",
            rusqlite::params![
                v.uuid,
                v.name,
                v.mount_path.to_string_lossy(),
                v.total_bytes as i64,
                v.free_bytes as i64
            ],
        )?;
        c.execute(
            "INSERT INTO libraries(volume_uuid, rel_path, name, area) VALUES(?1,?2,?3,?4)
             ON CONFLICT(volume_uuid, rel_path) DO UPDATE SET name=excluded.name, area=excluded.area",
            rusqlite::params![v.uuid, rel, name, area],
        )
    })
    .map_err(|e| e.to_string())?;

    let id: i64 = db
        .read(|c| {
            c.query_row(
                "SELECT id FROM libraries WHERE volume_uuid=?1 AND rel_path=?2",
                rusqlite::params![v.uuid, rel],
                |r| r.get(0),
            )
        })
        .map_err(|e| e.to_string())?;

    get(db, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "등록 직후 찾을 수 없습니다".to_string())
}

/// 등록을 지운다. 폴더·파일·썸네일 기록도 함께 사라진다 (CASCADE).
/// **원본 사진과 디스크의 캐시 파일은 건드리지 않는다.**
pub fn remove(db: &Db, id: i64) -> Result<()> {
    db.write(|c| c.execute("DELETE FROM libraries WHERE id = ?1", [id]))?;
    Ok(())
}

/// 마운트 지점 기준 상대경로. 볼륨 최상단이면 빈 문자열.
pub fn rel_within(mount: &std::path::Path, dir: &std::path::Path) -> Option<String> {
    let rel = dir.strip_prefix(mount).ok()?;
    let s = rel.to_string_lossy();
    Some(crate::scan::nfc(s.trim_matches('/')))
}

/// 두 상대경로가 한쪽이 다른 쪽 안에 들어가는가.
///
/// 문자열 `starts_with`로는 안 된다 — `사진`이 `사진통합작업`을 잡아먹는다.
pub fn overlaps(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return true; // 볼륨 최상단은 무엇이든 품는다
    }
    if a == b {
        return true;
    }
    let (short, long) = if a.len() < b.len() { (a, b) } else { (b, a) };
    long.starts_with(short) && long.as_bytes().get(short.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rel_within_strips_the_mount() {
        assert_eq!(
            rel_within(Path::new("/Volumes/PHOTO 1"), Path::new("/Volumes/PHOTO 1")),
            Some(String::new()),
            "볼륨 최상단은 빈 문자열"
        );
        assert_eq!(
            rel_within(
                Path::new("/Volumes/MAIN SSD"),
                Path::new("/Volumes/MAIN SSD/MERGE/사진통합작업")
            ),
            Some("MERGE/사진통합작업".to_string())
        );
        assert_eq!(
            rel_within(Path::new("/Volumes/A"), Path::new("/Volumes/B/x")),
            None,
            "다른 볼륨이면 없다"
        );
    }

    #[test]
    fn overlap_respects_path_boundaries() {
        // 이름이 겹치는 형제는 겹치는 게 아니다
        assert!(!overlaps("사진", "사진통합작업"));
        assert!(overlaps("사진", "사진/2003"));
        assert!(overlaps("사진/2003", "사진"));
        assert!(overlaps("같은/것", "같은/것"));
        assert!(!overlaps("가족", "여행"));
    }

    #[test]
    fn volume_root_swallows_everything() {
        // 볼륨 최상단을 등록해 두면 그 안의 폴더는 따로 등록할 수 없다
        assert!(overlaps("", "MERGE/사진통합작업"));
        assert!(overlaps("MERGE", ""));
    }
}
