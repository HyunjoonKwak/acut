//! 목록 조회 — 그리드가 쓰는 쿼리.
//!
//! **keyset 페이지네이션을 쓴다.** `OFFSET`은 앞의 행을 전부 세면서 지나가므로
//! 뒤 페이지일수록 느려진다. 6만 장에서 스크롤을 끝까지 내리면 체감된다.
//! `WHERE taken_at < ?`는 인덱스에서 그 지점을 바로 찾으므로 어디서나 같은 속도다.
//!
//! 커서는 `(taken_at, id)` 쌍이다. 같은 시각의 사진이 여럿일 수 있어 id로 동점을
//! 가른다. 이게 없으면 경계에서 사진이 빠지거나 겹친다.

use crate::db::conn::{Db, Result};

/// 그리드 한 칸에 필요한 것만. 인스펙터용 상세는 따로 가져온다.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileRow {
    pub id: i64,
    pub name: String,
    pub taken_at: i64,
    pub taken_at_source: i32,
    pub kind: i32,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub rating: i32,
    pub culling_flag: i32,
    pub favorite: bool,
    /// 캐시 루트 기준 상대경로. 없으면 아직 생성 전이다.
    pub thumb: Option<String>,
}

/// 다음 페이지를 가리키는 커서.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub taken_at: i64,
    pub id: i64,
}

/// `#[serde(default)]`가 중요하다. 프론트는 필요한 필드만 보낸다 —
/// 없는 필드에서 역직렬화가 실패하면 커맨드 전체가 거부된다.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Filter {
    /// 이 폴더와 하위 폴더. None이면 전체.
    pub folder_id: Option<i64>,
    /// 0 작업대 · 1 내사진 · 2 공용
    pub area: Option<i32>,
    /// 0 사진 · 1 영상 · 2 RAW
    pub kind: Option<i32>,
    /// 이 값 이상만
    pub min_rating: Option<i32>,
    /// 0 미판정 · 1 남김 · 2 제외
    pub culling_flag: Option<i32>,
    pub favorite_only: bool,
    /// 파일명 부분 일치
    pub name_like: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Page {
    pub rows: Vec<FileRow>,
    pub next: Option<Cursor>,
}

/// LIKE 와일드카드를 이스케이프한다. `_`가 임의 문자로 동작하면
/// `IMG_1234` 검색이 엉뚱한 것까지 잡는다.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// 필터를 WHERE 절과 파라미터로 바꾼다.
fn build_where(f: &Filter, cursor: Option<Cursor>) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut w: Vec<String> = Vec::new();
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(id) = f.folder_id {
        // 하위 폴더까지 포함한다. rel_path 접두사로 찾는다.
        w.push(
            "fi.folder_id IN (SELECT id FROM folders WHERE id = ?
              OR (volume_uuid = (SELECT volume_uuid FROM folders WHERE id = ?)
                  AND rel_path LIKE (SELECT rel_path FROM folders WHERE id = ?) || '/%'))"
                .into(),
        );
        p.push(Box::new(id));
        p.push(Box::new(id));
        p.push(Box::new(id));
    }
    if let Some(a) = f.area {
        w.push("fo.area = ?".into());
        p.push(Box::new(a));
    }
    if let Some(k) = f.kind {
        w.push("fi.kind = ?".into());
        p.push(Box::new(k));
    }
    if let Some(r) = f.min_rating {
        w.push("fi.rating >= ?".into());
        p.push(Box::new(r));
    }
    if let Some(c) = f.culling_flag {
        w.push("fi.culling_flag = ?".into());
        p.push(Box::new(c));
    }
    if f.favorite_only {
        w.push("fi.favorite = 1".into());
    }
    if let Some(q) = f.name_like.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        w.push("fi.name LIKE ? ESCAPE '\\'".into());
        p.push(Box::new(format!("%{}%", escape_like(q))));
    }
    // 커서 — 정렬과 같은 방향이어야 한다 (taken_at DESC, id DESC)
    if let Some(c) = cursor {
        w.push("(fi.taken_at < ? OR (fi.taken_at = ? AND fi.id < ?))".into());
        p.push(Box::new(c.taken_at));
        p.push(Box::new(c.taken_at));
        p.push(Box::new(c.id));
    }

    let sql = if w.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", w.join(" AND "))
    };
    (sql, p)
}

/// 최신순 한 페이지. 커서가 None이면 첫 페이지.
pub fn page(db: &Db, f: &Filter, cursor: Option<Cursor>, limit: usize) -> Result<Page> {
    let (where_sql, params) = build_where(f, cursor);
    // limit + 1을 읽어 다음 페이지가 있는지 알아낸다
    let sql = format!(
        "SELECT fi.id, fi.name, fi.taken_at, fi.taken_at_source, fi.kind, fi.size,
                fi.width, fi.height, fi.rating, fi.culling_flag, fi.favorite, t.rel_path
         FROM files fi
         JOIN folders fo ON fo.id = fi.folder_id
         LEFT JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
         {where_sql}
         ORDER BY fi.taken_at DESC, fi.id DESC
         LIMIT {}",
        limit + 1
    );

    let mut rows = db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let it = st.query_map(refs.as_slice(), |r| {
            Ok(FileRow {
                id: r.get(0)?,
                name: r.get(1)?,
                taken_at: r.get(2)?,
                taken_at_source: r.get(3)?,
                kind: r.get(4)?,
                size: r.get(5)?,
                width: r.get(6)?,
                height: r.get(7)?,
                rating: r.get(8)?,
                culling_flag: r.get(9)?,
                favorite: r.get::<_, i32>(10)? != 0,
                thumb: r.get(11)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| Cursor { taken_at: r.taken_at, id: r.id })
    } else {
        None
    };
    Ok(Page { rows, next })
}

/// 타임라인 눈금 하나 — 한 달치.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Bucket {
    pub year: i32,
    pub month: i32,
    pub count: i64,
    /// 이 달에서 가장 최근 촬영 시각. 여기로 점프한다.
    pub top: i64,
}

/// 월별 분포. 우측 스크러버가 쓴다.
///
/// keyset 페이지네이션이라 `top`만 있으면 그 시점부터 바로 읽을 수 있다.
/// OFFSET 방식이었다면 앞의 수만 행을 세고 지나가야 했다.
pub fn timeline(db: &Db, f: &Filter) -> Result<Vec<Bucket>> {
    let (where_sql, params) = build_where(f, None);
    let sql = format!(
        "SELECT CAST(strftime('%Y', fi.taken_at, 'unixepoch') AS INTEGER) y,
                CAST(strftime('%m', fi.taken_at, 'unixepoch') AS INTEGER) m,
                COUNT(*), MAX(fi.taken_at)
         FROM files fi JOIN folders fo ON fo.id = fi.folder_id
         {where_sql}
         GROUP BY y, m ORDER BY y DESC, m DESC"
    );
    db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let it = st.query_map(refs.as_slice(), |r| {
            Ok(Bucket { year: r.get(0)?, month: r.get(1)?, count: r.get(2)?, top: r.get(3)? })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 필터에 걸리는 전체 개수와 용량. 페이지마다 세지 않고 필터가 바뀔 때만 호출한다.
pub fn summary(db: &Db, f: &Filter) -> Result<(i64, i64)> {
    let (where_sql, params) = build_where(f, None);
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(fi.size),0) FROM files fi
         JOIN folders fo ON fo.id = fi.folder_id {where_sql}"
    );
    db.read(|c| {
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        c.query_row(&sql, refs.as_slice(), |r| Ok((r.get(0)?, r.get(1)?)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(2,'V','a/b','b',1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(3,'V','z','z',2)",
                [],
            )?;
            // 같은 taken_at을 여럿 두어 동점 처리를 시험한다
            for i in 1..=50 {
                let folder = if i <= 30 { 1 } else if i <= 40 { 2 } else { 3 };
                let taken = 1_000_000 + (i / 5) * 100; // 5개씩 같은 시각
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,
                        rating,culling_flag,favorite,scanned_at)
                     VALUES(?,?,?,?,?,?,0,?,?,?,0)",
                    rusqlite::params![
                        i,
                        folder,
                        format!("IMG_{i:04}.jpg"),
                        i * 100,
                        if i % 10 == 0 { 1 } else { 0 }, // 10개마다 영상
                        taken,
                        i % 6,                            // 평점 0~5
                        if i % 7 == 0 { 2 } else { 0 },   // 7개마다 제외
                        i % 11 == 0,
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
        (dir, db)
    }

    #[test]
    fn partial_filter_json_deserializes() {
        // 프론트는 { folder_id: 1 }처럼 일부만 보낸다.
        // serde(default)가 없으면 "missing field favorite_only"로 커맨드가 거부된다.
        let f: Filter = serde_json::from_str(r#"{"folder_id":1}"#).expect("일부 필드만");
        assert_eq!(f.folder_id, Some(1));
        assert!(!f.favorite_only);
        let empty: Filter = serde_json::from_str("{}").expect("빈 객체");
        assert!(empty.folder_id.is_none());
        // null도 받아들여야 한다
        let nulls: Filter =
            serde_json::from_str(r#"{"folder_id":null,"kind":null}"#).expect("null");
        assert!(nulls.folder_id.is_none());
    }

    #[test]
    fn first_page_is_newest_first() {
        let (_d, db) = seeded();
        let p = page(&db, &Filter::default(), None, 10).unwrap();
        assert_eq!(p.rows.len(), 10);
        assert!(p.next.is_some());
        // 내림차순인지
        for w in p.rows.windows(2) {
            assert!(
                (w[0].taken_at, w[0].id) > (w[1].taken_at, w[1].id),
                "최신순이어야 한다"
            );
        }
    }

    #[test]
    fn paging_covers_everything_exactly_once() {
        let (_d, db) = seeded();
        let mut seen = Vec::new();
        let mut cursor = None;
        loop {
            let p = page(&db, &Filter::default(), cursor, 7).unwrap();
            seen.extend(p.rows.iter().map(|r| r.id));
            match p.next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(seen.len(), 50, "빠짐없이");
        let mut uniq = seen.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 50, "겹침 없이");
    }

    #[test]
    fn folder_filter_includes_subfolders() {
        let (_d, db) = seeded();
        // 폴더 1(a)에는 30장, 하위 폴더 2(a/b)에 10장 → 40장
        let f = Filter { folder_id: Some(1), ..Default::default() };
        let (n, _) = summary(&db, &f).unwrap();
        assert_eq!(n, 40, "하위 폴더를 포함해야 한다");
    }

    #[test]
    fn area_filter_separates_regions() {
        let (_d, db) = seeded();
        let mine = Filter { area: Some(1), ..Default::default() };
        let shared = Filter { area: Some(2), ..Default::default() };
        assert_eq!(summary(&db, &mine).unwrap().0, 40);
        assert_eq!(summary(&db, &shared).unwrap().0, 10);
    }

    #[test]
    fn rating_and_culling_filters() {
        let (_d, db) = seeded();
        let high = Filter { min_rating: Some(4), ..Default::default() };
        let (n, _) = summary(&db, &high).unwrap();
        assert!(n > 0 && n < 50);
        for r in page(&db, &high, None, 100).unwrap().rows {
            assert!(r.rating >= 4);
        }
        let rejected = Filter { culling_flag: Some(2), ..Default::default() };
        for r in page(&db, &rejected, None, 100).unwrap().rows {
            assert_eq!(r.culling_flag, 2);
        }
    }

    #[test]
    fn name_search_escapes_wildcards() {
        let (_d, db) = seeded();
        // "IMG_0001"의 밑줄이 와일드카드로 동작하면 안 된다
        let f = Filter { name_like: Some("IMG_0001".into()), ..Default::default() };
        let p = page(&db, &f, None, 100).unwrap();
        assert_eq!(p.rows.len(), 1, "정확히 하나만");
        assert_eq!(p.rows[0].name, "IMG_0001.jpg");

        // 밑줄이 와일드카드였다면 "IMGX0001"도 걸렸을 것이다
        let f2 = Filter { name_like: Some("IMG".into()), ..Default::default() };
        assert_eq!(page(&db, &f2, None, 100).unwrap().rows.len(), 50);
    }

    #[test]
    fn thumb_is_none_until_generated() {
        let (_d, db) = seeded();
        let p = page(&db, &Filter::default(), None, 5).unwrap();
        assert!(p.rows.iter().all(|r| r.thumb.is_none()));

        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(50,'ab/abcd.jpg',1,1,1)",
                [],
            )
        })
        .unwrap();
        let p2 = page(&db, &Filter::default(), None, 5).unwrap();
        assert_eq!(
            p2.rows.iter().filter(|r| r.thumb.is_some()).count(),
            1,
            "만들어진 것만 경로가 있다"
        );
    }

    #[test]
    fn failed_thumbs_are_not_served() {
        let (_d, db) = seeded();
        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(50,'ab/x.jpg',1,1,2)", // state 2 = 실패
                [],
            )
        })
        .unwrap();
        let p = page(&db, &Filter::default(), None, 5).unwrap();
        assert!(
            p.rows.iter().all(|r| r.thumb.is_none()),
            "실패한 썸네일은 내보내지 않는다"
        );
    }

    #[test]
    fn timeline_groups_by_month_newest_first() {
        let (_d, db) = seeded();
        let b = timeline(&db, &Filter::default()).unwrap();
        assert!(!b.is_empty());
        // 내림차순
        for w in b.windows(2) {
            assert!((w[0].year, w[0].month) >= (w[1].year, w[1].month));
        }
        // 합계가 전체와 같아야 한다
        assert_eq!(b.iter().map(|x| x.count).sum::<i64>(), 50);
        // top으로 그 지점부터 읽을 수 있어야 한다
        let first = &b[0];
        let p = page(&db, &Filter::default(), Some(Cursor { taken_at: first.top + 1, id: i64::MAX }), 5)
            .unwrap();
        assert!(!p.rows.is_empty(), "점프 지점부터 읽힌다");
    }

    #[test]
    fn summary_reports_count_and_bytes() {
        let (_d, db) = seeded();
        let (n, bytes) = summary(&db, &Filter::default()).unwrap();
        assert_eq!(n, 50);
        assert_eq!(bytes, (1..=50).map(|i| i * 100).sum::<i64>());
    }
}
