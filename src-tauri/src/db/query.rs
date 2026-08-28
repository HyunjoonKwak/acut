//! 목록 조회 — 그리드가 쓰는 쿼리.
//!
//! **keyset 페이지네이션을 쓴다.** `OFFSET`은 앞의 행을 전부 세면서 지나가므로
//! 뒤 페이지일수록 느려진다. 6만 장에서 스크롤을 끝까지 내리면 체감된다.
//! `WHERE taken_at < ?`는 인덱스에서 그 지점을 바로 찾으므로 어디서나 같은 속도다.
//!
//! 커서는 `(taken_at, id)` 쌍이다. 같은 시각의 사진이 여럿일 수 있어 id로 동점을
//! 가른다. 이게 없으면 경계에서 사진이 빠지거나 겹친다.

use crate::db::conn::{Db, Result};
use rusqlite::OptionalExtension;

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
    /// 영상 길이. 타일의 ▶ 배지에 쓴다.
    pub duration_ms: Option<i64>,
    /// 정렬 커서를 만들 때 쓴다 (생성일·수정일 기준 정렬)
    pub created_at: Option<i64>,
    pub modified_at: Option<i64>,
    /// 그룹 머리글에 쓸 값. 묶기를 끄면 비어 있다.
    pub group: Option<String>,
    /// 어느 라이브러리 소속인가. 썸네일 캐시가 라이브러리마다 따로 있어서
    /// 프론트가 `thumb://` 주소를 만들 때 필요하다.
    pub library_id: Option<i64>,
    /// 캐시 루트 기준 상대경로. 없으면 아직 생성 전이다.
    pub thumb: Option<String>,
    /// 타일 배지용 — 설정에서 ISO·셔터·조리개·초점거리 중 하나를 고른다
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter: Option<String>,
    pub focal_mm: Option<f64>,
    pub cam_model: Option<String>,
}

/// 무엇으로 정렬할까. Lap의 정렬 목록과 같다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    TakenAt,
    CreatedAt,
    ModifiedAt,
    Name,
    Size,
    Pixels,
    Duration,
}

impl SortBy {
    /// 정렬에 쓸 식. NULL은 맨 뒤로 몰리게 COALESCE로 채운다 — 안 그러면
    /// 커서 비교에서 NULL이 끼어 페이지가 끊긴다.
    fn expr(self) -> &'static str {
        match self {
            SortBy::TakenAt => "fi.taken_at",
            SortBy::CreatedAt => "COALESCE(fi.created_at, 0)",
            SortBy::ModifiedAt => "COALESCE(fi.modified_at, 0)",
            SortBy::Name => "fi.name",
            SortBy::Size => "fi.size",
            SortBy::Pixels => "COALESCE(fi.width,0) * COALESCE(fi.height,0)",
            SortBy::Duration => "COALESCE(fi.duration_ms, 0)",
        }
    }
    fn is_text(self) -> bool {
        matches!(self, SortBy::Name)
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Sort {
    pub by: SortBy,
    /// 큰 것부터. 촬영일은 최신순이 기본이다.
    pub desc: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self { by: SortBy::TakenAt, desc: true }
    }
}

/// 다음 페이지를 가리키는 커서.
///
/// 정렬 기준 값과 id를 함께 들고 다닌다. id가 없으면 같은 값이 여럿일 때
/// 경계에서 사진이 빠지거나 겹친다.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Cursor {
    /// 숫자 기준일 때의 값
    pub num: Option<i64>,
    /// 이름 기준일 때의 값
    pub text: Option<String>,
    pub id: i64,
}

impl Default for Cursor {
    fn default() -> Self {
        Self { num: None, text: None, id: 0 }
    }
}

/// `#[serde(default)]`가 중요하다. 프론트는 필요한 필드만 보낸다 —
/// 없는 필드에서 역직렬화가 실패하면 커맨드 전체가 거부된다.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Filter {
    /// 등록한 라이브러리 하나만. None이면 전체.
    pub library_id: Option<i64>,
    /// 이 폴더와 하위 폴더. None이면 전체.
    pub folder_id: Option<i64>,
    /// 볼륨 기준 폴더 경로. 이 폴더와 하위 폴더를 고른다.
    ///
    /// 사이드바 트리에는 DB에 행이 없는 중간 마디가 있어서 id로는 못 고른다.
    /// (`연도별`처럼 자기 자신엔 사진이 없고 아래에만 있는 폴더)
    pub folder_path: Option<String>,
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
    /// true면 **휴지통에 든 것만** 본다. 기본은 살아 있는 것만.
    pub trashed: bool,
    /// 무엇으로 어떤 방향으로 늘어놓을까.
    pub sort: Sort,
    /// 사이드바에서 고른 연도 (`2024`)
    pub year: Option<String>,
    /// 사이드바에서 고른 달 (`2024-08`)
    pub month: Option<String>,
    /// 사이드바에서 고른 날 (`2024-08-27`)
    pub day: Option<String>,
    /// 사이드바에서 고른 카메라 모델
    pub camera: Option<String>,
    /// 사이드바에서 고른 렌즈. 빈 문자열이면 "렌즈 정보 없음".
    pub lens: Option<String>,
    /// 사이드바에서 고른 태그
    pub tag_id: Option<i64>,
    /// 위치 — 좌표 격자 한 칸 (`37.5,127.0`). 빈 문자열이면 "위치 없음".
    pub place: Option<String>,
    /// 썸네일이 없는 것만 — 못 만들었거나 아직 안 만든 것. 상태바 «썸네일 없음
    /// N장»을 누르면 걸린다. 무엇이 안 되는지 눈으로 봐야 한다.
    #[serde(default)]
    pub no_thumb: bool,
}

/// 그리드에 머리글을 넣어 묶는 기준. Lap의 GROUP과 같다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    #[default]
    None,
    Folder,
    Day,
    Month,
    Year,
    Rating,
    Camera,
    Lens,
    FileType,
    Culling,
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

/// WHERE 절이 `folders`를 실제로 보는가.
///
/// 안 볼 때는 조인을 빼야 한다. `files`만 훑으면 되는 집계에서 14만 번의
/// rowid 조회가 통째로 사라진다 (실측 타임라인 395ms -> 240ms).
fn needs_folder_join(f: &Filter) -> bool {
    f.area.is_some() || f.library_id.is_some() || f.folder_path.is_some()
}

/// 필터를 WHERE 절과 파라미터로 바꾼다.
fn build_where(f: &Filter, cursor: Option<Cursor>) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut w: Vec<String> = Vec::new();
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 버린 것은 기본적으로 안 보인다. 이게 첫 조건이어야 한다 — 빼먹으면
    // 휴지통에 넣은 사진이 목록에 계속 남아 있고 원본은 없어 썸네일만 뜬다.
    w.push(
        if f.trashed {
            "fi.trashed_at IS NOT NULL"
        } else {
            "fi.trashed_at IS NULL"
        }
        .into(),
    );

    if let Some(id) = f.library_id {
        w.push("fo.library_id = ?".into());
        p.push(Box::new(id));
    }
    if let Some(p_) = f.folder_path.as_deref().filter(|s| !s.is_empty()) {
        // LIKE는 `_`와 `%`를 와일드카드로 본다. 실제 폴더에 `#0_사진백업…`
        // 같은 이름이 있어 이스케이프가 필수다.
        w.push("(fo.rel_path = ? OR fo.rel_path LIKE ? ESCAPE '\\')".into());
        p.push(Box::new(p_.to_string()));
        p.push(Box::new(format!("{}/%", escape_like(p_))));
    }
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
    if let Some(y) = f.year.as_deref().filter(|s| !s.is_empty()) {
        w.push("strftime('%Y', fi.taken_at,'unixepoch','localtime') = ?".into());
        p.push(Box::new(y.to_string()));
    }
    if let Some(m) = f.month.as_deref().filter(|s| !s.is_empty()) {
        w.push("strftime('%Y-%m', fi.taken_at,'unixepoch','localtime') = ?".into());
        p.push(Box::new(m.to_string()));
    }
    if let Some(d) = f.day.as_deref().filter(|s| !s.is_empty()) {
        w.push("strftime('%Y-%m-%d', fi.taken_at,'unixepoch','localtime') = ?".into());
        p.push(Box::new(d.to_string()));
    }
    if let Some(cam) = f.camera.as_ref() {
        // 빈 문자열은 "카메라 정보 없음"을 뜻한다
        if cam.is_empty() {
            w.push("COALESCE(NULLIF(fi.cam_model,''),'') = ''".into());
        } else {
            w.push("fi.cam_model = ?".into());
            p.push(Box::new(cam.clone()));
        }
    }
    if let Some(l) = f.lens.as_ref() {
        if l.is_empty() {
            w.push("COALESCE(NULLIF(fi.lens,''),'') = ''".into());
        } else {
            w.push("fi.lens = ?".into());
            p.push(Box::new(l.clone()));
        }
    }
    if let Some(t) = f.tag_id {
        w.push("EXISTS (SELECT 1 FROM file_tags ft WHERE ft.file_id = fi.id AND ft.tag_id = ?)".into());
        p.push(Box::new(t));
    }
    if let Some(pl) = f.place.as_ref() {
        if pl.is_empty() {
            w.push("fi.gps_lat IS NULL".into());
        } else if let Some((a, b)) = pl.split_once(',') {
            // 격자 한 칸 = 0.1도 (위도로 약 11km). 그 칸 안이면 같은 곳으로 친다.
            if let (Ok(lat), Ok(lon)) = (a.parse::<f64>(), b.parse::<f64>()) {
                w.push(
                    "fi.gps_lat >= ? AND fi.gps_lat < ? AND fi.gps_lon >= ? AND fi.gps_lon < ?"
                        .into(),
                );
                p.push(Box::new(lat));
                p.push(Box::new(lat + 0.1));
                p.push(Box::new(lon));
                p.push(Box::new(lon + 0.1));
            }
        }
    }
    if f.favorite_only {
        w.push("fi.favorite = 1".into());
    }
    if f.no_thumb {
        w.push("NOT EXISTS (SELECT 1 FROM thumbs t WHERE t.file_id = fi.id AND t.state = 1)".into());
    }
    if let Some(q) = f.name_like.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        w.push("fi.name LIKE ? ESCAPE '\\'".into());
        p.push(Box::new(format!("%{}%", escape_like(q))));
    }
    // 커서 — 정렬과 **같은 방향**이어야 한다. 방향이 어긋나면 페이지가
    // 겹치거나 통째로 건너뛴다.
    if let Some(c) = cursor {
        let col = f.sort.by.expr();
        let cmp = if f.sort.desc { "<" } else { ">" };
        w.push(format!("({col} {cmp} ? OR ({col} = ? AND fi.id {cmp} ?))"));
        if f.sort.by.is_text() {
            let v = c.text.clone().unwrap_or_default();
            p.push(Box::new(v.clone()));
            p.push(Box::new(v));
        } else {
            let v = c.num.unwrap_or(0);
            p.push(Box::new(v));
            p.push(Box::new(v));
        }
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
pub fn page(
    db: &Db,
    f: &Filter,
    cursor: Option<Cursor>,
    limit: usize,
    group: GroupBy,
) -> Result<Page> {
    let (where_sql, params) = build_where(f, cursor);
    let dir = if f.sort.desc { "DESC" } else { "ASC" };
    let order = format!("{} {dir}, fi.id {dir}", f.sort.by.expr());
    let group_expr = group_expr(group);
    // limit + 1을 읽어 다음 페이지가 있는지 알아낸다
    let sql = format!(
        "SELECT fi.id, fi.name, fi.taken_at, fi.taken_at_source, fi.kind, fi.size,
                fi.width, fi.height, fi.rating, fi.culling_flag, fi.favorite,
                fi.duration_ms, fi.created_at, fi.modified_at, fo.library_id, t.rel_path,
                {group_expr},
                fi.iso, fi.aperture, fi.shutter, fi.focal_mm, fi.cam_model
         FROM files fi
         JOIN folders fo ON fo.id = fi.folder_id
         LEFT JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
         {where_sql}
         ORDER BY {order}
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
                duration_ms: r.get(11)?,
                created_at: r.get(12)?,
                modified_at: r.get(13)?,
                library_id: r.get(14)?,
                thumb: r.get(15)?,
                iso: r.get(17)?,
                aperture: r.get(18)?,
                shutter: r.get(19)?,
                focal_mm: r.get(20)?,
                cam_model: r.get(21)?,
                group: r.get(16)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(|r| cursor_of(r, f.sort.by))
    } else {
        None
    };
    Ok(Page { rows, next })
}

/// 그룹 머리글에 쓸 값을 SQL로 뽑는다. 프론트는 값이 바뀌는 자리에 줄을 넣는다.
///
/// 서버에서 계산하는 이유: 이어 읽는 페이지의 첫 줄이 앞 페이지의 마지막과
/// 같은 그룹인지 알아야 머리글이 중복되지 않는다. 값이 행에 붙어 있으면
/// 그 비교가 저절로 된다.
fn group_expr(g: GroupBy) -> String {
    match g {
        GroupBy::None => "NULL".into(),
        GroupBy::Folder => "fo.rel_path".into(),
        GroupBy::Day => "date(fi.taken_at,'unixepoch','localtime')".into(),
        GroupBy::Month => "strftime('%Y-%m', fi.taken_at,'unixepoch','localtime')".into(),
        GroupBy::Year => "strftime('%Y', fi.taken_at,'unixepoch','localtime')".into(),
        GroupBy::Rating => "CAST(fi.rating AS TEXT)".into(),
        GroupBy::Camera => "COALESCE(NULLIF(fi.cam_model,''),'(카메라 정보 없음)')".into(),
        GroupBy::Lens => "COALESCE(NULLIF(fi.lens,''),'(렌즈 정보 없음)')".into(),
        GroupBy::FileType => {
            "CASE fi.kind WHEN 0 THEN '사진' WHEN 1 THEN '영상' ELSE 'RAW' END".into()
        }
        GroupBy::Culling => {
            "CASE fi.culling_flag WHEN 1 THEN '남김' WHEN 2 THEN '제외' ELSE '미판정' END".into()
        }
    }
}

/// 마지막 행에서 다음 커서를 만든다. 정렬 기준에 따라 어느 값을 담을지 갈린다.
fn cursor_of(r: &FileRow, by: SortBy) -> Cursor {
    let mut c = Cursor { num: None, text: None, id: r.id };
    match by {
        SortBy::Name => c.text = Some(r.name.clone()),
        SortBy::TakenAt => c.num = Some(r.taken_at),
        SortBy::CreatedAt => c.num = Some(r.created_at.unwrap_or(0)),
        SortBy::ModifiedAt => c.num = Some(r.modified_at.unwrap_or(0)),
        SortBy::Size => c.num = Some(r.size),
        SortBy::Pixels => {
            c.num = Some(r.width.unwrap_or(0) * r.height.unwrap_or(0));
        }
        SortBy::Duration => c.num = Some(r.duration_ms.unwrap_or(0)),
    }
    c
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
    // strftime을 **한 번만** 부른다. '%Y'와 '%m'을 따로 부르면 날짜 계산이
    // 두 번 돈다 (실측 14만 행: 237ms -> 89ms). 쪼개는 건 Rust가 한다.
    let join = if needs_folder_join(f) {
        "JOIN folders fo ON fo.id = fi.folder_id"
    } else {
        ""
    };
    let sql = format!(
        "SELECT strftime('%Y-%m', fi.taken_at, 'unixepoch') ym,
                COUNT(*), MAX(fi.taken_at)
         FROM files fi
         {join}
         {where_sql}
         GROUP BY ym ORDER BY ym DESC"
    );
    db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let it = st.query_map(refs.as_slice(), |r| {
            let ym: String = r.get(0)?;
            let (y, m) = ym.split_once('-').unwrap_or(("0", "0"));
            Ok(Bucket {
                year: y.parse().unwrap_or(0),
                month: m.parse().unwrap_or(0),
                count: r.get(1)?,
                top: r.get(2)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// 전역 순번 `index`에서 페이지를 시작하려면 어떤 커서를 써야 하는가.
///
/// 스크롤바 손잡이를 끌면 "전체의 37% 지점"처럼 **순번**이 나온다. 그런데
/// `page()`는 커서 기반이라 순번을 모른다. 여기서 한 번만 OFFSET으로 그 자리의
/// 행을 찾아 커서로 바꿔 준다. 이후 페이지는 다시 keyset으로 이어 읽는다.
///
/// OFFSET을 쓰지만 `(taken_at DESC, id DESC)` 인덱스만 훑고 테이블은 건드리지
/// 않는다. 6만 행 규모에서 한 번 호출은 밀리초 단위다. 목록 전체를 OFFSET으로
/// 넘기던 옛 방식과는 비용이 다르다.
///
/// `index`가 0 이하면 맨 앞이므로 커서가 없다(None).
pub fn cursor_at(db: &Db, f: &Filter, index: i64) -> Result<Option<Cursor>> {
    if index <= 0 {
        return Ok(None);
    }
    let (where_sql, mut params) = build_where(f, None);
    params.push(Box::new(index - 1));
    let join = if needs_folder_join(f) {
        "JOIN folders fo ON fo.id = fi.folder_id"
    } else {
        ""
    };
    let dir = if f.sort.desc { "DESC" } else { "ASC" };
    let col = f.sort.by.expr();
    let sql = format!(
        "SELECT {col}, fi.id FROM files fi
         {join}
         {where_sql}
         ORDER BY {col} {dir}, fi.id {dir}
         LIMIT 1 OFFSET ?"
    );
    let text = f.sort.by.is_text();
    db.read(|c| {
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        c.query_row(&sql, refs.as_slice(), |r| {
            Ok(if text {
                Cursor { num: None, text: Some(r.get(0)?), id: r.get(1)? }
            } else {
                Cursor { num: Some(r.get(0)?), text: None, id: r.get(1)? }
            })
        })
        .optional()
    })
}

/// 사이드바의 갈래 하나 — 값·표시 이름·장수.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Facet {
    pub value: String,
    pub label: String,
    pub count: i64,
}

/// 사이드바가 훑어볼 갈래.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetKind {
    Year,
    /// 하루 단위. 달력이 한 달을 펼쳤을 때 쓴다 — 필터에 month를 함께 건다.
    Day,
    Camera,
    Lens,
    Rating,
    Kind,
    Place,
}

/// 지금 필터 안에서 각 값이 몇 장인지 센다.
///
/// 필터를 함께 거는 이유: 「2020년」을 고른 뒤 카메라 목록을 보면 그해에 쓴
/// 카메라만 나와야 한다. 전체 목록이 나오면 눌러도 0장인 것이 섞인다.
pub fn facets(db: &Db, f: &Filter, kind: FacetKind) -> Result<Vec<Facet>> {
    let (where_sql, params) = build_where(f, None);
    let join = if needs_folder_join(f) {
        "JOIN folders fo ON fo.id = fi.folder_id"
    } else {
        ""
    };
    let expr = match kind {
        FacetKind::Year => "strftime('%Y', fi.taken_at,'unixepoch','localtime')",
        FacetKind::Day => "strftime('%Y-%m-%d', fi.taken_at,'unixepoch','localtime')",
        FacetKind::Camera => "COALESCE(NULLIF(fi.cam_model,''),'')",
        FacetKind::Lens => "COALESCE(NULLIF(fi.lens,''),'')",
        FacetKind::Rating => "CAST(fi.rating AS TEXT)",
        FacetKind::Kind => "CAST(fi.kind AS TEXT)",
        // 좌표를 0.1도 격자로 내린다. 역지오코딩이 없어 지명은 못 붙이지만
        // "이 근처에서 찍은 것"을 모아 보는 데는 충분하다.
        FacetKind::Place => {
            "CASE WHEN fi.gps_lat IS NULL THEN ''
                  ELSE CAST(ROUND(fi.gps_lat*10-0.5)/10 AS TEXT) || ',' ||
                       CAST(ROUND(fi.gps_lon*10-0.5)/10 AS TEXT) END"
        }
    };
    let order = match kind {
        // 연도·평점은 값 순서로, 카메라는 많이 쓴 것부터
        FacetKind::Year | FacetKind::Day | FacetKind::Rating => "v DESC",
        FacetKind::Place => "n DESC, v",
        _ => "n DESC, v",
    };
    let sql = format!(
        "SELECT {expr} v, COUNT(*) n FROM files fi {join} {where_sql}
         GROUP BY v ORDER BY {order} LIMIT 200"
    );
    let rows: Vec<(String, i64)> = db.read(|c| {
        let mut st = c.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let it = st.query_map(refs.as_slice(), |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    Ok(rows
        .into_iter()
        .map(|(value, count)| {
            let label = match kind {
                FacetKind::Year => format!("{value}년"),
                // `2024-08-27` → `27일`. 어느 달인지는 위에 펼쳐진 줄이 말한다.
                FacetKind::Day => value
                    .rsplit('-')
                    .next()
                    .and_then(|d| d.parse::<u32>().ok())
                    .map(|d| format!("{d}일"))
                    .unwrap_or_else(|| value.clone()),
                FacetKind::Rating => match value.as_str() {
                    "0" => "평점 없음".into(),
                    n => "★".repeat(n.parse::<usize>().unwrap_or(0)),
                },
                FacetKind::Kind => match value.as_str() {
                    "0" => "사진".into(),
                    "1" => "영상".into(),
                    _ => "RAW".into(),
                },
                FacetKind::Camera => {
                    if value.is_empty() {
                        "(카메라 정보 없음)".into()
                    } else {
                        value.clone()
                    }
                }
                FacetKind::Lens => {
                    if value.is_empty() {
                        "(렌즈 정보 없음)".into()
                    } else {
                        value.clone()
                    }
                }
                FacetKind::Place => {
                    if value.is_empty() {
                        "(위치 정보 없음)".into()
                    } else {
                        // `37.5,127` → `북위 37.5° 동경 127.0°`
                        match value.split_once(',') {
                            Some((a, b)) => {
                                let lat: f64 = a.parse().unwrap_or(0.0);
                                let lon: f64 = b.parse().unwrap_or(0.0);
                                format!(
                                    "{} {:.1}° {} {:.1}°",
                                    if lat >= 0.0 { "북위" } else { "남위" },
                                    lat.abs(),
                                    if lon >= 0.0 { "동경" } else { "서경" },
                                    lon.abs(),
                                )
                            }
                            None => value.clone(),
                        }
                    }
                }
            };
            Facet { value, label, count }
        })
        .collect())
}

/// 필터에 걸리는 전체 개수와 용량. 페이지마다 세지 않고 필터가 바뀔 때만 호출한다.
pub fn summary(db: &Db, f: &Filter) -> Result<(i64, i64)> {
    let (where_sql, params) = build_where(f, None);
    let join = if needs_folder_join(f) {
        "JOIN folders fo ON fo.id = fi.folder_id"
    } else {
        ""
    };
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(fi.size),0) FROM files fi {join} {where_sql}"
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

    /// 스크롤바가 준 순번으로 커서를 얻어 그 자리부터 읽는다.
    /// 전부 읽은 목록의 같은 자리와 일치해야 한다 — 어긋나면 손잡이가 딴 데로 간다.
    #[test]
    fn cursor_at_lands_on_the_same_row_as_a_full_read() {
        let (_d, db) = seeded();
        let f = Filter::default();
        let all = page(&db, &f, None, 500, GroupBy::None).unwrap().rows;
        assert_eq!(all.len(), 50);

        for index in [0usize, 1, 7, 23, 49] {
            let c = cursor_at(&db, &f, index as i64).unwrap();
            let got = page(&db, &f, c, 3, GroupBy::None).unwrap().rows;
            assert_eq!(
                got[0].id, all[index].id,
                "{index}번째에서 시작해야 한다"
            );
        }
    }

    #[test]
    fn cursor_at_respects_the_filter() {
        let (_d, db) = seeded();
        // 영상만 — 10개마다 하나라 5장이다
        let f = Filter { kind: Some(1), ..Default::default() };
        let all = page(&db, &f, None, 500, GroupBy::None).unwrap().rows;
        assert_eq!(all.len(), 5);
        let c = cursor_at(&db, &f, 3).unwrap();
        let got = page(&db, &f, c, 5, GroupBy::None).unwrap().rows;
        assert_eq!(got[0].id, all[3].id);
        assert_eq!(got.len(), 2, "3번째부터 끝까지");
    }

    /// area 필터는 folders를 봐야 해서 조인을 살린다. 그 분기도 맞아야 한다.
    #[test]
    fn cursor_at_keeps_the_join_for_area() {
        let (_d, db) = seeded();
        let f = Filter { area: Some(2), ..Default::default() };
        let all = page(&db, &f, None, 500, GroupBy::None).unwrap().rows;
        assert_eq!(all.len(), 10, "폴더 3(area=2)에 41~50번 10장");
        let c = cursor_at(&db, &f, 4).unwrap();
        let got = page(&db, &f, c, 10, GroupBy::None).unwrap().rows;
        assert_eq!(got[0].id, all[4].id);
        assert_eq!(got.len(), 6);
    }

    #[test]
    fn cursor_at_edges() {
        let (_d, db) = seeded();
        let f = Filter::default();
        assert!(cursor_at(&db, &f, 0).unwrap().is_none(), "맨 앞은 커서가 없다");
        assert!(cursor_at(&db, &f, -5).unwrap().is_none(), "음수도 맨 앞으로");
        // 끝을 넘어가면 행이 없다 — 빈 페이지가 되지 손잡이가 깨지면 안 된다
        assert!(cursor_at(&db, &f, 9999).unwrap().is_none());
    }

    /// 경로 앞부분으로 폴더와 그 아래를 고른다. 사이드바 트리의 중간 마디는
    /// DB 행이 없어 id로는 못 고른다.
    /// 버린 사진이 목록에 남아 있으면 원본은 없는데 타일만 뜬다.
    #[test]
    fn trashed_files_disappear_from_the_default_view() {
        let (_d, db) = seeded();
        let all = page(&db, &Filter::default(), None, 500, GroupBy::None).unwrap().rows.len();
        db.write(|c| {
            c.execute("UPDATE files SET trashed_at=1 WHERE id IN (1,2,3)", [])
        })
        .unwrap();

        assert_eq!(page(&db, &Filter::default(), None, 500, GroupBy::None).unwrap().rows.len(), all - 3);
        assert_eq!(summary(&db, &Filter::default()).unwrap().0, all as i64 - 3);
        assert_eq!(
            timeline(&db, &Filter::default()).unwrap().iter().map(|b| b.count).sum::<i64>(),
            all as i64 - 3
        );

        // 휴지통 보기에서는 그것만 나온다
        let t = Filter { trashed: true, ..Default::default() };
        assert_eq!(page(&db, &t, None, 500, GroupBy::None).unwrap().rows.len(), 3);
    }

    /// 정렬 기준을 바꿔도 페이지가 끊기거나 겹치지 않아야 한다.
    /// 커서 방향이 정렬 방향과 어긋나면 딱 그 증상이 난다.
    #[test]
    fn every_sort_pages_without_gaps_or_overlaps() {
        let (_d, db) = seeded();
        for by in [
            SortBy::TakenAt,
            SortBy::CreatedAt,
            SortBy::ModifiedAt,
            SortBy::Name,
            SortBy::Size,
            SortBy::Pixels,
            SortBy::Duration,
        ] {
            for desc in [true, false] {
                let f = Filter { sort: Sort { by, desc }, ..Default::default() };
                // 한 번에 다 읽은 것과 7장씩 넘겨 읽은 것이 같아야 한다
                let all: Vec<i64> =
                    page(&db, &f, None, 500, GroupBy::None).unwrap().rows.iter().map(|r| r.id).collect();
                let mut paged = Vec::new();
                let mut cur = None;
                loop {
                    let p = page(&db, &f, cur, 7, GroupBy::None).unwrap();
                    paged.extend(p.rows.iter().map(|r| r.id));
                    match p.next {
                        Some(c) => cur = Some(c),
                        None => break,
                    }
                }
                assert_eq!(all, paged, "{by:?} desc={desc}");
                assert_eq!(all.len(), 50, "{by:?} desc={desc} — 빠진 것이 없어야 한다");
            }
        }
    }

    /// 그룹 값은 **행에 붙어** 온다. 이어 읽은 페이지의 첫 줄이 앞 페이지
    /// 마지막과 같은 그룹이면 머리글을 또 넣으면 안 되는데, 값이 붙어 있으면
    /// 그 비교가 저절로 된다.
    #[test]
    fn group_values_ride_along_with_each_row() {
        let (_d, db) = seeded();
        let f = Filter::default();

        let none = page(&db, &f, None, 5, GroupBy::None).unwrap().rows;
        assert!(none.iter().all(|r| r.group.is_none()), "안 묶으면 비어 있다");

        for g in [
            GroupBy::Folder,
            GroupBy::Day,
            GroupBy::Month,
            GroupBy::Year,
            GroupBy::Rating,
            GroupBy::FileType,
            GroupBy::Culling,
            GroupBy::Camera,
            GroupBy::Lens,
        ] {
            let rows = page(&db, &f, None, 50, g).unwrap().rows;
            assert!(
                rows.iter().all(|r| r.group.is_some()),
                "{g:?} — 모든 행에 값이 있어야 한다"
            );
        }
    }

    /// 페이지를 넘어가도 그룹 값이 이어져야 한다.
    #[test]
    fn group_values_survive_paging() {
        let (_d, db) = seeded();
        let f = Filter::default();
        let all: Vec<Option<String>> = page(&db, &f, None, 500, GroupBy::Day)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.group.clone())
            .collect();

        let mut paged = Vec::new();
        let mut cur = None;
        loop {
            let p = page(&db, &f, cur, 6, GroupBy::Day).unwrap();
            paged.extend(p.rows.iter().map(|r| r.group.clone()));
            match p.next {
                Some(c) => cur = Some(c),
                None => break,
            }
        }
        assert_eq!(all, paged);
    }

    #[test]
    fn file_type_group_is_readable() {
        let (_d, db) = seeded();
        let rows = page(&db, &Filter::default(), None, 50, GroupBy::FileType)
            .unwrap()
            .rows;
        let names: std::collections::HashSet<String> =
            rows.iter().filter_map(|r| r.group.clone()).collect();
        assert!(names.contains("사진"), "{names:?}");
        assert!(names.contains("영상"), "{names:?}");
    }

    #[test]
    fn ascending_and_descending_are_mirror_images() {
        let (_d, db) = seeded();
        let asc = Filter { sort: Sort { by: SortBy::Size, desc: false }, ..Default::default() };
        let desc = Filter { sort: Sort { by: SortBy::Size, desc: true }, ..Default::default() };
        let a: Vec<i64> = page(&db, &asc, None, 500, GroupBy::None).unwrap().rows.iter().map(|r| r.id).collect();
        let mut d: Vec<i64> =
            page(&db, &desc, None, 500, GroupBy::None).unwrap().rows.iter().map(|r| r.id).collect();
        d.reverse();
        assert_eq!(a, d);
    }

    /// 스크롤바가 준 순번은 **지금 정렬 기준**의 순번이어야 한다.
    #[test]
    fn cursor_at_follows_the_current_sort() {
        let (_d, db) = seeded();
        let f = Filter { sort: Sort { by: SortBy::Name, desc: false }, ..Default::default() };
        let all = page(&db, &f, None, 500, GroupBy::None).unwrap().rows;
        for i in [0usize, 5, 30, 49] {
            let c = cursor_at(&db, &f, i as i64).unwrap();
            let got = page(&db, &f, c, 3, GroupBy::None).unwrap().rows;
            assert_eq!(got[0].id, all[i].id, "{i}번째");
        }
    }

    #[test]
    fn folder_path_selects_the_subtree() {
        let (_d, db) = seeded();
        // 폴더 1 = 'a', 폴더 2 = 'a/b', 폴더 3 = 'z'
        let f = Filter { folder_path: Some("a".into()), ..Default::default() };
        let n = page(&db, &f, None, 500, GroupBy::None).unwrap().rows.len();
        assert_eq!(n, 40, "a(30) + a/b(10)");

        let only_b = Filter { folder_path: Some("a/b".into()), ..Default::default() };
        assert_eq!(page(&db, &only_b, None, 500, GroupBy::None).unwrap().rows.len(), 10);

        // 이름이 겹치는 형제를 잡아먹으면 안 된다
        let none = Filter { folder_path: Some("a/bb".into()), ..Default::default() };
        assert_eq!(page(&db, &none, None, 500, GroupBy::None).unwrap().rows.len(), 0);
    }

    /// LIKE의 `_`는 아무 글자나 매치한다. 실제 라이브러리에 `#0_사진백업…`
    /// 같은 폴더가 있어 이스케이프하지 않으면 엉뚱한 폴더까지 딸려온다.
    #[test]
    fn folder_path_escapes_like_wildcards() {
        let (dir, db) = seeded();
        let _ = dir;
        db.write(|c| {
            c.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES
                   (10,'V','p_q','p_q',1),(11,'V','pXq','pXq',1)",
                [],
            )?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(101,10,'a.jpg',1,0,1000,0,0),(102,11,'b.jpg',1,0,1000,0,0)",
                [],
            )
        })
        .unwrap();

        let f = Filter { folder_path: Some("p_q".into()), ..Default::default() };
        let rows = page(&db, &f, None, 500, GroupBy::None).unwrap().rows;
        assert_eq!(rows.len(), 1, "pXq까지 잡히면 안 된다");
        assert_eq!(rows[0].name, "a.jpg");
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
        let p = page(&db, &Filter::default(), None, 10, GroupBy::None).unwrap();
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
            let p = page(&db, &Filter::default(), cursor, 7, GroupBy::None).unwrap();
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
        for r in page(&db, &high, None, 100, GroupBy::None).unwrap().rows {
            assert!(r.rating >= 4);
        }
        let rejected = Filter { culling_flag: Some(2), ..Default::default() };
        for r in page(&db, &rejected, None, 100, GroupBy::None).unwrap().rows {
            assert_eq!(r.culling_flag, 2);
        }
    }

    #[test]
    fn name_search_escapes_wildcards() {
        let (_d, db) = seeded();
        // "IMG_0001"의 밑줄이 와일드카드로 동작하면 안 된다
        let f = Filter { name_like: Some("IMG_0001".into()), ..Default::default() };
        let p = page(&db, &f, None, 100, GroupBy::None).unwrap();
        assert_eq!(p.rows.len(), 1, "정확히 하나만");
        assert_eq!(p.rows[0].name, "IMG_0001.jpg");

        // 밑줄이 와일드카드였다면 "IMGX0001"도 걸렸을 것이다
        let f2 = Filter { name_like: Some("IMG".into()), ..Default::default() };
        assert_eq!(page(&db, &f2, None, 100, GroupBy::None).unwrap().rows.len(), 50);
    }

    #[test]
    fn thumb_is_none_until_generated() {
        let (_d, db) = seeded();
        let p = page(&db, &Filter::default(), None, 5, GroupBy::None).unwrap();
        assert!(p.rows.iter().all(|r| r.thumb.is_none()));

        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(50,'ab/abcd.jpg',1,1,1)",
                [],
            )
        })
        .unwrap();
        let p2 = page(&db, &Filter::default(), None, 5, GroupBy::None).unwrap();
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
        let p = page(&db, &Filter::default(), None, 5, GroupBy::None).unwrap();
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
        let p = page(&db, &Filter::default(), Some(Cursor { num: Some(first.top + 1), text: None, id: i64::MAX }), 5, GroupBy::None)
            .unwrap();
        assert!(!p.rows.is_empty(), "점프 지점부터 읽힌다");
    }

    /// `strftime('%Y-%m')` 한 번으로 바꾸면서 Rust가 문자열을 쪼갠다.
    /// 연·월이 정수로 제대로 나오는지, 필터가 걸려도 그런지 본다.
    #[test]
    fn timeline_parses_year_and_month_as_numbers() {
        let (_d, db) = seeded();
        for f in [Filter::default(), Filter { area: Some(1), ..Default::default() }] {
            let b = timeline(&db, &f).unwrap();
            assert!(!b.is_empty());
            for x in &b {
                assert!(x.year >= 1970 && x.year <= 2100, "연도: {}", x.year);
                assert!((1..=12).contains(&x.month), "월: {}", x.month);
                assert!(x.count > 0);
            }
        }
    }

    /// 조인을 빼는 최적화가 결과를 바꾸면 안 된다.
    #[test]
    fn dropping_the_join_keeps_the_same_totals() {
        let (_d, db) = seeded();
        // folders를 보는 필터(area)와 안 보는 필터(kind)가 서로 어긋나지 않아야 한다
        let all = timeline(&db, &Filter::default()).unwrap();
        assert_eq!(
            all.iter().map(|x| x.count).sum::<i64>(),
            summary(&db, &Filter::default()).unwrap().0
        );
        let area = Filter { area: Some(2), ..Default::default() };
        assert_eq!(
            timeline(&db, &area).unwrap().iter().map(|x| x.count).sum::<i64>(),
            summary(&db, &area).unwrap().0
        );
    }

    /// 갈래 목록은 **지금 필터 안에서** 세야 한다. 전체를 세면 눌러도 0장인
    /// 항목이 섞인다.
    #[test]
    fn facets_are_counted_inside_the_current_filter() {
        let (_d, db) = seeded();
        let all = facets(&db, &Filter::default(), FacetKind::Kind).unwrap();
        assert_eq!(all.iter().map(|f| f.count).sum::<i64>(), 50);

        // 영상만 걸어 두면 갈래도 영상만 남는다
        let only_video = Filter { kind: Some(1), ..Default::default() };
        let v = facets(&db, &only_video, FacetKind::Kind).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label, "영상");
        assert_eq!(v[0].count, 5);
    }

    #[test]
    fn facet_labels_are_readable() {
        let (_d, db) = seeded();
        let r = facets(&db, &Filter::default(), FacetKind::Rating).unwrap();
        assert!(r.iter().any(|f| f.label == "평점 없음"));
        assert!(r.iter().any(|f| f.label.starts_with('★')));

        let y = facets(&db, &Filter::default(), FacetKind::Year).unwrap();
        assert!(y.iter().all(|f| f.label.ends_with('년')), "{y:?}");
        // 연도는 최근이 위
        for w in y.windows(2) {
            assert!(w[0].value >= w[1].value);
        }
    }

    /// 태그는 폴더와 달리 한 장에 여럿 붙는다. 필터가 그중 하나만 걸려도
    /// 그 사진이 나와야 한다.
    #[test]
    fn tag_filter_matches_any_of_a_files_tags() {
        let (_d, db) = seeded();
        db.transaction(|tx| {
            tx.execute("INSERT INTO tags(id,name) VALUES(1,'여행'),(2,'가족')", [])?;
            // 1~5번은 여행, 4~6번은 가족 — 4·5번은 둘 다
            for i in 1..=5 {
                tx.execute("INSERT INTO file_tags(file_id,tag_id) VALUES(?,1)", [i])?;
            }
            for i in 4..=6 {
                tx.execute("INSERT INTO file_tags(file_id,tag_id) VALUES(?,2)", [i])?;
            }
            Ok(())
        })
        .unwrap();

        let f = |t: i64| Filter { tag_id: Some(t), ..Default::default() };
        assert_eq!(summary(&db, &f(1)).unwrap().0, 5);
        assert_eq!(summary(&db, &f(2)).unwrap().0, 3);

        // 겹치는 두 장이 양쪽에 다 들어 있어야 한다
        let a: Vec<i64> = page(&db, &f(1), None, 99, GroupBy::None)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id)
            .collect();
        let b: Vec<i64> = page(&db, &f(2), None, 99, GroupBy::None)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id)
            .collect();
        assert!(a.contains(&4) && b.contains(&4));
        assert!(a.contains(&5) && b.contains(&5));
        // 없는 태그는 빈 목록
        assert_eq!(summary(&db, &f(99)).unwrap().0, 0);
    }

    /// 자리 갈래는 0.1도 격자다. 같은 칸에 든 것이 한 줄로 모여야 하고,
    /// 그 값을 필터로 되돌려 걸면 같은 장수가 나와야 한다.
    #[test]
    fn place_facet_grids_coordinates_and_round_trips() {
        let (_d, db) = seeded();
        db.write(|c| {
            // 1~4번은 서울 한 칸(37.55, 126.98), 5번은 다른 칸
            c.execute(
                "UPDATE files SET gps_lat=37.55, gps_lon=126.98 WHERE id<=4",
                [],
            )?;
            c.execute(
                "UPDATE files SET gps_lat=35.15, gps_lon=129.05 WHERE id=5",
                [],
            )
        })
        .unwrap();

        let fs = facets(&db, &Filter::default(), FacetKind::Place).unwrap();
        // 좌표 없는 것들이 한 줄, 서울 한 줄, 부산 한 줄
        let none = fs.iter().find(|f| f.value.is_empty()).unwrap();
        assert_eq!(none.count, 45);
        assert_eq!(none.label, "(위치 정보 없음)");

        let seoul = fs.iter().find(|f| f.value.starts_with("37.5")).unwrap();
        assert_eq!(seoul.count, 4);
        assert_eq!(seoul.label, "북위 37.5° 동경 126.9°");

        // 갈래가 준 값을 그대로 필터로 되돌린다
        for f in &fs {
            let n = summary(
                &db,
                &Filter { place: Some(f.value.clone()), ..Default::default() },
            )
            .unwrap()
            .0;
            assert_eq!(n, f.count, "{} 되돌리기", f.label);
        }
    }

    /// 남반구·서반구 좌표도 같은 칸에서 갈라지면 안 된다 — 음수를 내림할 때
    /// 0 쪽으로 자르면 -0.05와 0.05가 같은 칸에 들어간다.
    #[test]
    fn place_grid_handles_negative_coordinates() {
        let (_d, db) = seeded();
        db.write(|c| {
            c.execute("UPDATE files SET gps_lat=-33.87, gps_lon=-70.65 WHERE id=1", [])?;
            c.execute("UPDATE files SET gps_lat=-33.83, gps_lon=-70.61 WHERE id=2", [])?;
            c.execute("UPDATE files SET gps_lat=0.05, gps_lon=0.05 WHERE id=3", [])?;
            c.execute("UPDATE files SET gps_lat=-0.05, gps_lon=-0.05 WHERE id=4", [])
        })
        .unwrap();

        let fs = facets(&db, &Filter::default(), FacetKind::Place).unwrap();
        // -33.87과 -33.83은 다른 칸(-33.9 / -33.9? 아니다: -33.9와 -33.9)
        // 중요한 건 0을 사이에 둔 3·4번이 갈라지는 것이다
        let a = fs.iter().find(|f| f.value == "0.0,0.0").map(|f| f.count);
        let b = fs.iter().find(|f| f.value == "-0.1,-0.1").map(|f| f.count);
        assert_eq!(a, Some(1), "{fs:?}");
        assert_eq!(b, Some(1), "{fs:?}");

        let south = fs.iter().find(|f| f.value.starts_with("-33")).unwrap();
        assert!(south.label.starts_with("남위"), "{}", south.label);
        assert!(south.label.contains("서경"), "{}", south.label);

        // 음수에서도 갈래가 준 값이 그대로 필터로 되돌아가야 한다.
        // 격자 상자를 `[v, v+0.1)`로 잡는데 v가 음수면 부동소수 오차가
        // 반대쪽으로 새기 쉽다.
        for f in &fs {
            let n = summary(
                &db,
                &Filter { place: Some(f.value.clone()), ..Default::default() },
            )
            .unwrap()
            .0;
            assert_eq!(n, f.count, "{} 되돌리기", f.label);
        }
    }

    /// 상태바의 «썸네일 없음 N장»과 그걸 눌렀을 때 뜨는 장수가 같아야 한다
    #[test]
    fn no_thumb_filter_matches_the_pending_count() {
        let (_d, db) = seeded();
        db.write(|c| {
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 SELECT id,'x',1,1,1 FROM files WHERE id <= 40",
                [],
            )?;
            // 하나는 실패한 것
            c.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state) VALUES(41,NULL,1,1,2)",
                [],
            )
        })
        .unwrap();
        let n = summary(&db, &Filter { no_thumb: true, ..Default::default() }).unwrap().0;
        assert_eq!(n, 10, "40장은 됐고 41은 실패, 42~50은 아직 — 열 장");
    }

    #[test]
    fn day_facet_and_filter_round_trip() {
        let (_d, db) = seeded();
        // 한 달 안에서 날짜별로 센다 — 갈래 값을 필터로 되돌리면 같은 수
        let month = facets(&db, &Filter::default(), FacetKind::Year).unwrap();
        assert!(!month.is_empty());
        let fs = facets(&db, &Filter::default(), FacetKind::Day).unwrap();
        assert!(fs.iter().all(|f| f.label.ends_with('일')), "{fs:?}");
        for f in &fs {
            let n = summary(&db, &Filter { day: Some(f.value.clone()), ..Default::default() })
                .unwrap()
                .0;
            assert_eq!(n, f.count, "{} 되돌리기", f.value);
        }
        // 최근이 위
        for w in fs.windows(2) {
            assert!(w[0].value >= w[1].value);
        }
    }

    #[test]
    fn lens_facet_and_filter_round_trip() {
        let (_d, db) = seeded();
        db.write(|c| c.execute("UPDATE files SET lens='FE 24-70' WHERE id <= 3", [])).unwrap();
        let fs = facets(&db, &Filter::default(), FacetKind::Lens).unwrap();
        assert!(fs.iter().any(|f| f.label == "(렌즈 정보 없음)"), "{fs:?}");
        for f in &fs {
            let n = summary(&db, &Filter { lens: Some(f.value.clone()), ..Default::default() })
                .unwrap()
                .0;
            assert_eq!(n, f.count, "{} 되돌리기", f.label);
        }
    }

    #[test]
    fn camera_facet_names_the_unknown() {
        let (_d, db) = seeded();
        let c = facets(&db, &Filter::default(), FacetKind::Camera).unwrap();
        assert!(c.iter().any(|f| f.label == "(카메라 정보 없음)"), "{c:?}");
    }

    #[test]
    fn summary_reports_count_and_bytes() {
        let (_d, db) = seeded();
        let (n, bytes) = summary(&db, &Filter::default()).unwrap();
        assert_eq!(n, 50);
        assert_eq!(bytes, (1..=50).map(|i| i * 100).sum::<i64>());
    }
}
