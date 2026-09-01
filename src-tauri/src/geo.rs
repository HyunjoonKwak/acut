//! 지명 — 좌표를 «국가 / 시도 / 시군구» 세 단계 이름으로.
//!
//! 사진마다 묻지 않는다. 좌표를 **0.01도 격자**(약 1.1km)로 뭉쳐 격자마다 한 번만
//! 물어보고 `places` 에 캐시한다 — 실측(2026-09-01) 사진 52,576장이 격자로는
//! 1,143칸뿐이다. 한 번 채우면 그 뒤로는 완전히 오프라인이다.
//!
//! 이름은 Nominatim 규약을 쓰는 서버에서 받는다. 배포 앱 여러 대의 요청을 합쳐
//! 초당 한 건이어야 하는 OSM 공개 서버는 배치 작업에 쓰지 않는다. 설정
//! (`geo.endpoint`)에 자체 Nominatim이나 배치 사용이 허용된 서비스를 넣었을 때만
//! 새 좌표를 묻는다. 이미 받은 캐시는 서버가 없어도 쓴다.
//!
//! 서버에는 초당 한 건만 보내고, 앱을 밝히는 User-Agent를 쓰며, HTTP 오류가 오면
//! 그 자리에서 멈춘다. 물어보는 좌표는 그 칸에 실제로 있는 사진들의 대표 좌표이고,
//! 한 번 물어본 것은 places 에 남아 두 번 묻지 않는다.

use crate::db::conn::{Db, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 격자 한 칸 — 0.01도. 이보다 잘게 나누면 같은 동네를 여러 번 묻는다.
const CELL: f64 = 0.01;
/// Nominatim 규칙 — 초당 한 건.
const GAP: Duration = Duration::from_millis(1100);
const UA: &str = concat!("acut/", env!("CARGO_PKG_VERSION"), " (personal photo library; github.com/HyunjoonKwak/acut)");
const ENDPOINT_KEY: &str = "geo.endpoint";
const PUBLIC_HOST: &str = "nominatim.openstreetmap.org";
/// 캐시 한 줄의 상태 — 이 셋을 섞으면 «결과 없음»을 영영 다시 묻는다 (2026-09-01 리뷰)
const OK: &str = "ok";
/// 그 자리에 이름이 없다고 **온라인 서버가 확정**했다. 다시 묻지 않는다
const NONE: &str = "none";
/// 오프라인으로 안전하게 정하지 못했다 — 온라인으로 다시 물을 수 있다.
/// none 과 섞으면 «물어볼 수 있는 것»을 영영 잃는다 (2026-09-01 리뷰)
const UNRESOLVED: &str = "unresolved";
/// 출처 — 어디서 온 값인가
const SRC_OFFLINE: &str = "offline_geonames";
const SRC_ONLINE: &str = "nominatim";
/// 정밀도 — 얼마나 믿을 만한가
const PREC_APPROX: &str = "approximate";
const PREC_BOUNDARY: &str = "boundary";
const PREC_REMOTE: &str = "remote";

/// 지도와 같은 «쓸 수 있는 좌표» 규칙 — 판정은 db::predicates 가 갖는다.
/// 통계·대상 선택·파일 갱신이 반드시 같은 조건을 써야, 처리할 수 없는 행을
/// 영원히 남은 것으로 세지 않는다. 이 모듈의 질의는 별칭 없이 files 를 읽는다.
fn valid_gps_sql() -> String {
    crate::db::predicates::valid_gps_sql("")
}

fn validate_endpoint(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(crate::db::conn::DbError::Invalid(
            "설정 › 탐색에서 자체 Nominatim 또는 배치 사용이 허용된 지명 서버를 먼저 입력해 주세요".into(),
        ));
    }
    let url = reqwest::Url::parse(raw)
        .map_err(|_| crate::db::conn::DbError::Invalid("지명 서버 주소가 올바른 URL이 아닙니다".into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(crate::db::conn::DbError::Invalid(
            "지명 서버는 http 또는 https 주소여야 합니다".into(),
        ));
    }
    if url
        .host_str()
        .is_some_and(|h| h.trim_end_matches('.').eq_ignore_ascii_case(PUBLIC_HOST))
    {
        return Err(crate::db::conn::DbError::Invalid(
            "OSM 공개 Nominatim은 배포 앱의 대량 조회에 사용할 수 없습니다 — 자체 서버나 배치 사용이 허용된 서비스를 입력해 주세요".into(),
        ));
    }
    Ok(raw.to_string())
}

fn endpoint_setting(db: &Db) -> Result<Option<String>> {
    Ok(crate::db::settings::get(db, ENDPOINT_KEY)?.filter(|s| !s.trim().is_empty()))
}

/// SQLite 로 좌표를 격자 문자열로 — `FLOOR` 는 수학 확장이 있어야 해서 쓰지 않는다.
/// CAST 는 0 쪽으로 자르므로 음수면 1을 뺀다 (내림).
fn cell_sql(lat: &str, lon: &str) -> String {
    let floor = |c: &str| {
        format!("(CAST({c} * 100.0 AS INTEGER) - ({c} * 100.0 < CAST({c} * 100.0 AS INTEGER))) / 100.0")
    };
    format!("printf('%.2f,%.2f', {}, {})", floor(lat), floor(lon))
}

/// 좌표 → 격자 열쇠. 음수도 같은 칸에 들어가게 내림으로 자른다.
pub fn cell(lat: f64, lon: f64) -> String {
    format!("{:.2},{:.2}", (lat / CELL).floor() * CELL, (lon / CELL).floor() * CELL)
}

/// 격자 열쇠 → 그 칸의 한가운데 좌표. 이 점을 물어본다.
pub fn cell_center(cell: &str) -> Option<(f64, f64)> {
    let (a, b) = cell.split_once(',')?;
    Some((a.parse::<f64>().ok()? + CELL / 2.0, b.parse::<f64>().ok()? + CELL / 2.0))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Place {
    pub country: Option<String>,
    /// 시도 — 경기도 · 서울특별시 · 뉴사우스웨일스주
    pub admin1: Option<String>,
    /// 시군구 — 수원시 · 서초구 · 시드니
    pub admin2: Option<String>,
}

impl Place {
    /// 표시용 — 가장 좁은 단계. 셋 다 비면 None
    pub fn name(&self) -> Option<String> {
        self.admin2.clone().or_else(|| self.admin1.clone()).or_else(|| self.country.clone())
    }
    pub fn is_empty(&self) -> bool {
        self.country.is_none() && self.admin1.is_none() && self.admin2.is_none()
    }
}

/// Nominatim 의 주소 조각을 세 단계로 접는다.
///
/// 나라마다 어느 칸이 오는지가 달라 «후보 목록 + 승격» 규칙을 쓴다:
/// 시도 후보가 비어 있고 시군구 후보가 둘 이상이면 첫째를 시도로 올린다.
/// (서울은 state 가 없고 city=서울특별시 · borough=서초구 로 온다)
pub fn fold(addr: &serde_json::Value) -> Place {
    let get = |k: &str| {
        addr.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let first = |keys: &[&str]| keys.iter().find_map(|k| get(k));

    let country = get("country");
    let lvl1 = first(&["state", "province", "region", "state_district"]);
    let lvl2: Vec<String> = ["city", "county", "municipality", "town", "borough", "city_district", "village", "suburb"]
        .iter()
        .filter_map(|k| get(k))
        .collect();
    // 같은 이름이 두 칸에 겹쳐 오는 경우가 있다 (city=수원시, county=수원시)
    let mut uniq: Vec<String> = Vec::new();
    for v in lvl2 {
        if !uniq.contains(&v) && Some(&v) != lvl1.as_ref() {
            uniq.push(v);
        }
    }
    match lvl1 {
        Some(a1) => Place { country, admin1: Some(a1), admin2: uniq.into_iter().next() },
        None if uniq.len() >= 2 => {
            let mut it = uniq.into_iter();
            Place { country, admin1: it.next(), admin2: it.next() }
        }
        None => Place { country, admin1: uniq.into_iter().next(), admin2: None },
    }
}

/// 물어본 결과 — 셋을 갈라야 «결과 없음»과 «잠깐 실패»를 다르게 다룬다.
enum Answer {
    Found(Place),
    /// 그 자리에 이름이 없다 — 캐시에 못 박고 다시 묻지 않는다
    Nothing,
    /// 서버 응답·연결이 정상이 아니다 — 캐시하지 않고 작업을 멈춘다
    Backoff(String),
}

/// 좌표 하나를 물어본다.
fn ask(client: &reqwest::blocking::Client, endpoint: &str, lat: f64, lon: f64, zoom: u8) -> Answer {
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{sep}lat={lat}&lon={lon}&format=jsonv2&zoom={zoom}&accept-language=ko");
    let res = match client.get(&url).header(reqwest::header::USER_AGENT, UA).send() {
        Ok(r) => r,
        Err(e) => return Answer::Backoff(format!("지명 서버에 연결하지 못했습니다: {e}")),
    };
    let status = res.status();
    if !status.is_success() {
        return Answer::Backoff(format!("서버가 {status} 로 답했습니다 — 잠시 뒤에 다시 해 주세요"));
    }
    let body: serde_json::Value = match res.json() {
        Ok(v) => v,
        Err(e) => return Answer::Backoff(format!("지명 서버 응답을 읽지 못했습니다: {e}")),
    };
    // 서버가 200 으로 오류를 싣는 경우도 있다
    if let Some(error) = body.get("error") {
        let message = error.as_str().map(str::to_string).unwrap_or_else(|| error.to_string());
        let lower = message.to_ascii_lowercase();
        return if lower.contains("unable to geocode")
            || lower.contains("not found")
            || lower.contains("no result")
        {
            Answer::Nothing
        } else {
            Answer::Backoff(format!("지명 서버 오류: {message}"))
        };
    }
    let place = body.get("address").map(fold).unwrap_or_default();
    // 위치 트리의 첫 단계이자 처리 완료 표시는 국가다. 국가가 없는 부분 응답을
    // 성공 캐시로 남기면 같은 파일이 영원히 미완료로 남는다.
    if place.country.is_none() { Answer::Nothing } else { Answer::Found(place) }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Progress {
    /// 이름이 필요한 격자 수
    pub total: usize,
    /// 여기까지 처리한 격자 수 (캐시로 채운 것 포함)
    pub done: usize,
    /// 새로 물어본 수
    pub asked: usize,
    /// 이름을 붙인 사진 수
    pub files: usize,
    /// 그 자리에 이름이 없어 다시 묻지 않기로 한 격자 수
    pub empty: usize,
    /// 서버가 그만하라고 해서 멈췄으면 그 사유
    pub stopped: Option<String>,
}

/// 캐시 한 줄을 쓰고 그 자리의 사진에 전파한다 — **한 트랜잭션**으로.
///
/// 중간에 앱이 꺼져도 places 와 files 가 어긋나지 않는다. 실패하면 둘 다 그대로다.
/// `INSERT OR REPLACE` 가 아니라 `ON CONFLICT DO UPDATE` 를 쓴다 — 행을 지웠다
/// 다시 만들면 나중에 붙일 외래 키·트리거가 조용히 깨진다 (2026-09-01 리뷰).
#[allow(clippy::too_many_arguments)]
fn write_place(
    db: &Db,
    cell_key: &str,
    place: &Place,
    status: &str,
    source: &str,
    precision: &str,
    distance_km: Option<f64>,
    dataset_version: Option<&str>,
    provider: Option<&str>,
    gps: &str,
) -> Result<usize> {
    let name = place.name();
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,
                                distance_km,dataset_version,provider,resolved_at,at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,strftime('%s','now'),strftime('%s','now'))
             ON CONFLICT(cell) DO UPDATE SET
               country=excluded.country, admin1=excluded.admin1, admin2=excluded.admin2,
               name=excluded.name, status=excluded.status, source=excluded.source,
               precision=excluded.precision, distance_km=excluded.distance_km,
               dataset_version=excluded.dataset_version, provider=excluded.provider,
               resolved_at=excluded.resolved_at",
            rusqlite::params![
                cell_key, &place.country, &place.admin1, &place.admin2, &name,
                status, source, precision, distance_km, dataset_version, provider
            ],
        )?;
        if place.country.is_none() {
            return Ok(0);
        }
        // 이 자리의 사진에 전파 — 아직 이름이 없거나, 덜 정밀한 값이 붙어 있는 것
        let n = tx.execute(
            &format!(
                "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                        geo_name = COALESCE(?4, ?3, ?2)
                 WHERE {cell} = ?1 AND {gps} AND geo_country IS NULL",
                cell = cell_sql("gps_lat", "gps_lon")
            ),
            rusqlite::params![cell_key, &place.country, &place.admin1, &place.admin2],
        )?;
        Ok(n)
    })
}

/// 이미 캐시에 있는 값을 그 자리의 사진에 붙인다 (네트워크 없이)
fn propagate(db: &Db, cell_key: &str, place: &Place, gps: &str) -> Result<usize> {
    db.write(|c| {
        c.execute(
            &format!(
                "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                        geo_name = COALESCE(?4, ?3, ?2)
                 WHERE {cell} = ?1 AND {gps} AND geo_country IS NULL",
                cell = cell_sql("gps_lat", "gps_lon")
            ),
            rusqlite::params![cell_key, &place.country, &place.admin1, &place.admin2],
        )
    })
}

/// 이름이 없는 사진들의 자리에 이름을 붙인다.
///
/// 격자는 «같은 곳을 두 번 묻지 않기» 위한 열쇠일 뿐이고, **물어보는 좌표는 그 칸에
/// 실제로 있는 사진들의 대표(중앙값에 가장 가까운 사진) 좌표**다 — 칸 정중앙을 물으면
/// 경계·해안·섬에서 옆 동네가 붙는다 (2026-09-01 리뷰).
///
/// 캐시에 있으면 곧바로 쓰고, 없을 때만 물어본다(초당 하나). 서버가 HTTP 오류로
/// 답하면 그 자리에서 멈춘다 — 채운 것은 남고 다음에 이어서 한다.
pub fn fill(
    db: &Db,
    cancel: &AtomicBool,
    limit: Option<usize>,
    on_progress: impl Fn(&Progress),
) -> Result<Progress> {
    let gps = valid_gps_sql();
    // 서버가 없어도 기존 성공 캐시는 파일에 적용할 수 있다. 실제로 새 좌표를
    // 물어야 하는 순간에만 설정을 요구한다.
    let endpoint = endpoint_setting(db)?;
    let zoom: u8 = crate::db::settings::get(db, "geo.zoom")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);

    // 1) 이름이 필요한 칸과 그 칸의 대표 좌표 — 사진 좌표를 정렬해 가운데 것을 고른다.
    //    (평균은 경계 양쪽에 사진이 있으면 둘 다 아닌 자리로 간다)
    let cell_expr = cell_sql("gps_lat", "gps_lon");
    let todo: Vec<(String, f64, f64)> = db.read(|c| {
        let mut st = c.prepare(&format!(
            "WITH pts AS (
               SELECT {cell_expr} AS cell, gps_lat AS la, gps_lon AS lo,
                      ROW_NUMBER() OVER (PARTITION BY {cell_expr} ORDER BY gps_lat, gps_lon) AS rn,
                      COUNT(*) OVER (PARTITION BY {cell_expr}) AS n
                 FROM files
                WHERE {gps}
                  AND geo_country IS NULL AND trashed_at IS NULL
             )
             SELECT cell, la, lo FROM pts WHERE rn = (n + 1) / 2
              AND cell NOT IN (SELECT cell FROM places WHERE status = ?1)
             ORDER BY cell",
        ))?;
        let it = st.query_map([NONE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let todo: Vec<_> = match limit {
        Some(n) => todo.into_iter().take(n).collect(),
        None => todo,
    };
    let mut p = Progress { total: todo.len(), ..Default::default() };
    on_progress(&p);
    if todo.is_empty() {
        return Ok(p);
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        // 허용된 주소가 공개 Nominatim으로 우회되지 않게 리다이렉트도 중단한다.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| crate::db::conn::DbError::Invalid(e.to_string()))?;

    for (cell_key, lat, lon) in todo {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // 캐시부터 — 성공한 것만 쓴다
        let cached: Option<Place> = db.read(|c| {
            c.query_row(
                "SELECT country, admin1, admin2 FROM places
                  WHERE cell = ?1 AND status = ?2
                    AND country IS NOT NULL AND trim(country) <> ''",
                rusqlite::params![&cell_key, OK],
                |r| Ok(Place { country: r.get(0)?, admin1: r.get(1)?, admin2: r.get(2)? }),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })
        })?;

        let place = match cached {
            Some(place) => place,
            None => {
                let endpoint = endpoint
                    .as_deref()
                    .map(validate_endpoint)
                    .transpose()?
                    .ok_or_else(|| crate::db::conn::DbError::Invalid(
                        "설정 › 탐색에서 자체 Nominatim 또는 배치 사용이 허용된 지명 서버를 먼저 입력해 주세요".into(),
                    ))?;
                std::thread::sleep(GAP); // 같은 서버에는 초당 하나
                p.asked += 1;
                match ask(&client, endpoint.as_str(), lat, lon, zoom) {
                    Answer::Found(place) => {
                        // 캐시 갱신과 파일 전파를 한 트랜잭션에 둔다 — 중간에 앱이 꺼져도
                        // places 와 files 가 어긋나지 않는다 (2026-09-01 리뷰)
                        let host = reqwest::Url::parse(endpoint.as_str())
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string));
                        write_place(db, &cell_key, &place, OK, SRC_ONLINE, PREC_REMOTE, None, None, host.as_deref(), &gps)?;
                        place
                    }
                    Answer::Nothing => {
                        // 서버가 «없다»고 확정한 자리 — 못 박아 두고 다시 묻지 않는다
                        let host = reqwest::Url::parse(endpoint.as_str())
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string));
                        write_place(db, &cell_key, &Place::default(), NONE, SRC_ONLINE, PREC_REMOTE, None, None, host.as_deref(), &gps)?;
                        p.empty += 1;
                        p.done += 1;
                        on_progress(&p);
                        continue;
                    }
                    Answer::Backoff(e) => {
                        log::warn!("지명 조회 중단 {cell_key}: {e}");
                        p.stopped = Some(e);
                        break;
                    }
                }
            }
        };

        // 캐시 적중이면 파일 전파만 하면 된다
        if !place.is_empty() {
            p.files += propagate(db, &cell_key, &place, &gps)?;
        }
        p.done += 1;
        on_progress(&p);
    }
    Ok(p)
}

/// 얼마나 남았나 — 설정 화면이 «지명 채우기» 앞에 보여 준다.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stats {
    /// 쓸 수 있는 좌표가 있는 사진
    pub with_gps: i64,
    /// 그중 이름이 붙은 사진 (오프라인·온라인 합)
    pub named: i64,
    /// 이름이 붙었으나 온라인으로 더 정밀하게 만들 수 있는 사진 (오프라인 결과)
    pub approximate_files: i64,
    /// 온라인 정밀 결과가 붙은 사진
    pub precise_files: i64,
    /// 아직 이름이 없고 **처리할 수 있는** 사진 (캐시 적용 또는 조회 대상)
    pub pending_files: i64,
    /// 온라인 서버가 «이름 없음»으로 확정한 사진 — 더 할 일이 없다
    pub unavailable_files: i64,
    /// 처리할 자리 수
    pub cells_left: i64,
    /// 그중 오프라인으로 풀 수 있는 자리 (스냅샷만 있으면 된다)
    pub offline_cells_left: i64,
    /// 그중 서버에 물어야만 하는 자리 (오프라인이 이미 포기한 곳)
    pub network_cells_left: i64,
    /// 새 조회에 쓸 수 있는 비공개/허가된 서버가 설정됐나
    pub endpoint_ready: bool,
}

pub fn stats(db: &Db) -> Result<Stats> {
    let gps = valid_gps_sql();
    let endpoint_ready = endpoint_setting(db)?
        .as_deref()
        .is_some_and(|s| validate_endpoint(s).is_ok());
    let mut stats = db.read(|c| {
        // 52,000행을 먼저 1,143개 자리로 접고, places 는 PK 로 한 번만 붙인다.
        // 행마다 상관 서브쿼리를 돌리던 이전 방식은 실측 0.23초였다 (2026-09-01 리뷰)
        c.query_row(
            &format!(
                "WITH valid AS (
                   SELECT {cell} AS cell, geo_country
                     FROM files
                    WHERE {gps} AND trashed_at IS NULL
                 ),
                 by_cell AS (
                   SELECT cell,
                          COUNT(*) AS files,
                          SUM(geo_country IS NOT NULL) AS named
                     FROM valid GROUP BY cell
                 ),
                 joined AS (
                   SELECT b.cell, b.files, b.named,
                          p.status, p.source, p.precision
                     FROM by_cell b LEFT JOIN places p ON p.cell = b.cell
                 )
                 SELECT
                   SUM(files),
                   SUM(named),
                   -- 이름은 있으나 온라인으로 더 정밀해질 수 있는 것
                   SUM(CASE WHEN named > 0 AND source = '{offline}' THEN named ELSE 0 END),
                   SUM(CASE WHEN named > 0 AND source = '{online}' THEN named ELSE 0 END),
                   -- 아직 이름이 없고 처리할 수 있는 것 (none 만 제외)
                   SUM(CASE WHEN status IS NULL OR status <> '{none}' THEN files - named ELSE 0 END),
                   -- 서버가 이름 없음으로 확정한 것
                   SUM(CASE WHEN status = '{none}' THEN files - named ELSE 0 END),
                   COUNT(CASE WHEN (status IS NULL OR status <> '{none}') AND files > named THEN 1 END),
                   -- 오프라인으로 풀 수 있는 자리: 아직 아무 판정이 없는 곳
                   COUNT(CASE WHEN status IS NULL AND files > named THEN 1 END),
                   -- 서버에만 물을 수 있는 자리: 오프라인이 이미 포기한 곳
                   COUNT(CASE WHEN status = '{unresolved}' AND files > named THEN 1 END)
                 FROM joined",
                cell = cell_sql("gps_lat", "gps_lon"),
                none = NONE, unresolved = UNRESOLVED, offline = SRC_OFFLINE, online = SRC_ONLINE
            ),
            [],
            |r| {
                Ok(Stats {
                    with_gps: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    named: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    approximate_files: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    precise_files: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    pending_files: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    unavailable_files: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    cells_left: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    offline_cells_left: r.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    network_cells_left: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    endpoint_ready: false,
                })
            },
        )
    })?;
    stats.endpoint_ready = endpoint_ready;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    /// 시험용 최소 HTTP 서버 — 미리 준비한 응답을 순서대로 하나씩 돌려준다.
    ///
    /// 안전장치(2026-09-01 리뷰): 클라이언트가 오지 않아도 **스스로 끝난다**.
    /// nonblocking accept + 2초 마감 + 소켓 읽기·쓰기 시간 제한. 시험은 join 대신
    /// 채널을 recv_timeout 으로 받아 «실패»가 «영원한 대기»가 되지 않게 한다.
    struct TestServer {
        url: String,
        /// 스레드가 끝나며 «받은 요청 수»를 보낸다
        done: std::sync::mpsc::Receiver<usize>,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        /// `replies` 는 (상태줄, 본문, 여분 헤더) 목록 — 요청 순서대로 쓰인다.
        /// 목록이 다 떨어지면 마지막 것을 되풀이한다.
        fn start(replies: Vec<(&'static str, &'static str, Option<&'static str>)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}/reverse", listener.local_addr().unwrap());
            let (tx, done) = std::sync::mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let handle = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                let mut served = 0usize;
                while std::time::Instant::now() < deadline && !stop_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                            let mut buf = [0_u8; 2048];
                            let _ = stream.read(&mut buf);
                            let (status, body, extra) =
                                replies.get(served).copied().unwrap_or_else(|| *replies.last().unwrap());
                            served += 1;
                            let extra = extra.map(|h| format!("{h}\r\n")).unwrap_or_default();
                            let res = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = stream.write_all(res.as_bytes());
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx.send(served);
            });
            TestServer { url, done, stop, handle: Some(handle) }
        }

        /// 응답을 한 번만 주는 서버 — 흔한 경우
        fn once(status: &'static str, body: &'static str) -> Self {
            Self::start(vec![(status, body, None)])
        }

        /// 서버가 받은 요청 수 — 재시도가 실제로 일어났는지 센다
        fn served(&mut self) -> usize {
            self.stop.store(true, Ordering::Relaxed);
            let n = self.done.recv_timeout(Duration::from_secs(3)).expect("서버 스레드가 끝나야 한다");
            if let Some(h) = self.handle.take() {
                h.join().expect("서버 스레드 join");
            }
            n
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            // 시험이 중간에 실패해도 스레드를 남기지 않는다
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// 시험용 클라이언트 — 반드시 시간 제한을 둔다. 없으면 실패가 무한 대기가 된다
    fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[test]
    fn a_cell_is_a_hundredth_of_a_degree_and_negatives_floor_the_same_way() {
        assert_eq!(cell(37.2846, 127.0512), "37.28,127.05");
        assert_eq!(cell(37.2899, 127.0599), "37.28,127.05", "같은 칸");
        assert_eq!(cell(-33.8688, 151.2093), "-33.87,151.20");
        let (lat, lon) = cell_center("37.28,127.05").unwrap();
        assert_eq!(cell(lat, lon), "37.28,127.05", "가운데 점은 제 칸으로 돌아온다");
    }

    /// 서울은 state 가 없고 city·borough 로 온다 — 시도로 승격해야 한다
    #[test]
    fn seoul_promotes_the_city_to_the_first_level() {
        let p = fold(&json!({"borough": "서초구", "city": "서울특별시", "country": "대한민국"}));
        assert_eq!(
            p,
            Place {
                country: Some("대한민국".into()),
                admin1: Some("서울특별시".into()),
                admin2: Some("서초구".into())
            }
        );
        assert_eq!(p.name().as_deref(), Some("서초구"));
    }

    #[test]
    fn a_province_and_a_city_map_straight_through() {
        let p = fold(&json!({"province": "경기도", "city": "수원시", "country": "대한민국"}));
        assert_eq!(p.admin1.as_deref(), Some("경기도"));
        assert_eq!(p.admin2.as_deref(), Some("수원시"));
    }

    #[test]
    fn overseas_uses_state_and_city() {
        let p = fold(&json!({"state": "뉴사우스웨일스주", "city": "시드니", "country": "오스트레일리아"}));
        assert_eq!(p.admin1.as_deref(), Some("뉴사우스웨일스주"));
        assert_eq!(p.admin2.as_deref(), Some("시드니"));
    }

    /// 같은 이름이 두 칸에 겹쳐 와도 두 단계에 같은 글자를 넣지 않는다
    #[test]
    fn a_duplicated_name_is_not_repeated_across_levels() {
        let p = fold(&json!({"province": "제주특별자치도", "city": "제주특별자치도", "county": "서귀포시"}));
        assert_eq!(p.admin1.as_deref(), Some("제주특별자치도"));
        assert_eq!(p.admin2.as_deref(), Some("서귀포시"));
    }

    #[test]
    fn an_empty_address_yields_nothing_to_show() {
        let p = fold(&json!({}));
        assert!(p.is_empty());
        assert_eq!(p.name(), None);
    }

    #[test]
    fn the_public_batch_endpoint_is_refused() {
        assert!(validate_endpoint("https://nominatim.openstreetmap.org/reverse").is_err());
        assert!(validate_endpoint("https://nominatim.openstreetmap.org./reverse").is_err());
        assert!(validate_endpoint("http://127.0.0.1:8080/reverse").is_ok());
    }

    /// SQL 격자 식과 러스트 cell() 이 같은 칸을 가리켜야 한다 — 음수 좌표 포함
    #[test]
    fn the_sql_grid_matches_the_rust_one() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        for (lat, lon) in [(37.2846, 127.0512), (-33.8688, 151.2093), (21.3, -157.86), (0.005, -0.005)] {
            let got: String = db
                .read(|c| {
                    c.query_row(&format!("SELECT {}", cell_sql(&lat.to_string(), &lon.to_string())), [], |r| r.get(0))
                })
                .unwrap();
            assert_eq!(got, cell(lat, lon), "{lat},{lon}");
        }
    }

    /// 이름이 필요한 격자만 센다 — 이미 이름이 있으면 세지 않는다
    #[test]
    fn stats_count_only_what_still_needs_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512),
                         (2,1,'b.jpg',1,0,1,0,0,37.2899,127.0599),
                         (3,1,'c.jpg',1,0,1,0,0,-33.8688,151.2093),
                         (4,1,'d.jpg',1,0,1,0,0,NULL,NULL),
                         (5,1,'bad-lat.jpg',1,0,1,0,0,200.0,127.0),
                         (6,1,'no-lon.jpg',1,0,1,0,0,37.0,NULL),
                         (7,1,'null-island.jpg',1,0,1,0,0,0.0,0.0);",
            )
        })
        .unwrap();
        let s = stats(&db).unwrap();
        assert_eq!(
            (s.with_gps, s.named, s.pending_files, s.unavailable_files, s.cells_left),
            (3, 0, 3, 0, 2),
            "같은 칸 둘은 한 번만 세고 잘못된 좌표는 대상에서 뺀다"
        );
        // 아직 아무 판정이 없는 자리는 오프라인으로 풀 수 있다 — 서버가 필요 없다
        assert_eq!((s.offline_cells_left, s.network_cells_left), (2, 0));
        assert!(!s.endpoint_ready, "기본값으로 공개 배치 서버를 쓰지 않는다");

        db.write(|c| c.execute("UPDATE files SET geo_country='대한민국' WHERE id IN (1,2)", []))
            .unwrap();
        let s = stats(&db).unwrap();
        assert_eq!((s.named, s.pending_files, s.cells_left, s.offline_cells_left), (2, 1, 1, 1));
    }

    /// 캐시에 있으면 묻지 않고 곧바로 사진에 붙인다 — 네트워크 없이 도는 길
    #[test]
    fn a_cached_cell_names_its_photos_without_asking() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512),
                         (2,1,'b.jpg',1,0,1,0,0,37.2899,127.0599);
                 INSERT INTO places(cell,country,admin1,admin2,name,status,at)
                   VALUES('37.28,127.05','대한민국','경기도','수원시','수원시','ok',0);",
            )
        })
        .unwrap();

        let before = stats(&db).unwrap();
        assert_eq!(
            (before.cells_left, before.offline_cells_left, before.network_cells_left),
            (1, 0, 0),
            "성공 캐시가 있는 자리는 조회 없이 붙이기만 하면 된다"
        );

        let p = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!((p.total, p.asked, p.files), (1, 0, 2), "묻지 않고 두 장에 붙는다");

        let (c1, a2, name): (String, String, String) = db
            .read(|c| {
                c.query_row("SELECT geo_country, geo_admin2, geo_name FROM files WHERE id=1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
            })
            .unwrap();
        assert_eq!((c1.as_str(), a2.as_str(), name.as_str()), ("대한민국", "수원시", "수원시"));

        // 두 번째로 부르면 할 일이 없다
        let again = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(again.total, 0);
    }

    #[test]
    fn an_uncached_fill_requires_an_allowed_batch_server() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512);",
            )
        })
        .unwrap();

        let err = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap_err();
        assert!(err.to_string().contains("지명 서버를 먼저"));

        crate::db::settings::set(&db, ENDPOINT_KEY, "https://nominatim.openstreetmap.org/reverse").unwrap();
        let err = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap_err();
        assert!(err.to_string().contains("공개 Nominatim"));
    }

    /// 이름이 없는 자리(status='none')는 두 번 묻지 않고, 남은 곳 셈에서도 빠진다.
    /// 전에는 빈 캐시를 «성공»으로 읽어 매번 같은 칸을 다시 대상으로 삼고
    /// «N장에 붙였습니다»까지 거짓으로 셌다 (2026-09-01 리뷰)
    #[test]
    fn a_place_with_no_name_is_not_asked_again_and_is_not_counted_as_named() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,10.005,20.005),
                         (2,1,'b.jpg',1,0,1,0,0,10.006,20.006);
                 INSERT INTO places(cell,country,admin1,admin2,name,status,at)
                   VALUES('10.00,20.00',NULL,NULL,NULL,NULL,'none',0);",
            )
        })
        .unwrap();

        // 물어볼 것이 없다 — 네트워크를 건드리지 않는다
        let p = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!((p.total, p.asked, p.files), (0, 0, 0), "이름 없는 자리는 대상이 아니다");

        // 남은 곳 셈에서도 빠진다 — 전에는 영영 줄지 않았다
        let s = stats(&db).unwrap();
        assert_eq!(
            (s.with_gps, s.named, s.pending_files, s.unavailable_files, s.cells_left),
            (2, 0, 0, 2, 0)
        );
        assert_eq!((s.offline_cells_left, s.network_cells_left), (0, 0));
    }

    /// unresolved 는 «다시 물을 수 있는 것»이라 none 과 달리 대상에 남는다
    #[test]
    fn an_unresolved_cell_stays_available_for_the_online_pass() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512);
                 INSERT INTO places(cell,status,source,at)
                   VALUES('37.28,127.05','unresolved','offline_geonames',0);",
            )
        })
        .unwrap();
        let s = stats(&db).unwrap();
        assert_eq!(s.pending_files, 1, "아직 처리할 수 있는 사진이다");
        assert_eq!(s.unavailable_files, 0, "«서버에도 없음»이 아니다");
        assert_eq!((s.offline_cells_left, s.network_cells_left), (0, 1), "오프라인은 포기했고 서버만 남았다");
    }

    /// 캐시 기록과 파일 전파는 한 트랜잭션이다 — 둘이 어긋나면 안 된다
    #[test]
    fn writing_a_place_updates_the_cache_and_the_photos_together() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512),
                         (2,1,'b.jpg',1,0,1,0,0,37.2899,127.0599);",
            )
        })
        .unwrap();
        let place = Place {
            country: Some("대한민국".into()),
            admin1: Some("경기도".into()),
            admin2: Some("수원시".into()),
        };
        let gps = valid_gps_sql();
        let n = write_place(&db, "37.28,127.05", &place, OK, SRC_ONLINE, PREC_REMOTE, None, None, Some("my.server"), &gps).unwrap();
        assert_eq!(n, 2, "그 자리의 두 장에 붙는다");

        let (status, source, precision, provider): (String, String, String, String) = db
            .read(|c| {
                c.query_row(
                    "SELECT status, source, precision, provider FROM places WHERE cell='37.28,127.05'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
            })
            .unwrap();
        assert_eq!((status.as_str(), source.as_str(), precision.as_str(), provider.as_str()),
                   ("ok", "nominatim", "remote", "my.server"));

        // 두 번 써도 행이 늘지 않고 값만 바뀐다 (ON CONFLICT DO UPDATE)
        write_place(&db, "37.28,127.05", &place, OK, SRC_ONLINE, PREC_REMOTE, None, None, Some("other.server"), &gps).unwrap();
        let rows: i64 = db.read(|c| c.query_row("SELECT COUNT(*) FROM places", [], |r| r.get(0))).unwrap();
        assert_eq!(rows, 1);
    }

    /// 칸 정중앙이 아니라 그 칸 사진들의 대표(가운데) 좌표를 묻는다 —
    /// 경계·해안·섬에서 옆 동네가 붙지 않게 (2026-09-01 리뷰)
    #[test]
    fn the_asked_point_is_a_real_photo_not_the_cell_centre() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2801,127.0501),
                         (2,1,'b.jpg',1,0,1,0,0,37.2802,127.0502),
                         (3,1,'c.jpg',1,0,1,0,0,37.2803,127.0503);",
            )
        })
        .unwrap();
        // fill 이 고르는 것과 같은 식으로 대표를 뽑아 본다
        let cell_expr = cell_sql("gps_lat", "gps_lon");
        let (cell_key, la, lo): (String, f64, f64) = db
            .read(|c| {
                c.query_row(
                    &format!(
                        "WITH pts AS (
                           SELECT {cell_expr} AS cell, gps_lat AS la, gps_lon AS lo,
                                  ROW_NUMBER() OVER (PARTITION BY {cell_expr} ORDER BY gps_lat, gps_lon) AS rn,
                                  COUNT(*) OVER (PARTITION BY {cell_expr}) AS n
                             FROM files WHERE gps_lat IS NOT NULL)
                         SELECT cell, la, lo FROM pts WHERE rn = (n + 1) / 2"
                    ),
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(cell_key, "37.28,127.05");
        assert_eq!((la, lo), (37.2802, 127.0502), "가운데 사진의 실제 좌표");
        let (clat, clon) = cell_center(&cell_key).unwrap();
        assert!((la - clat).abs() > 1e-6 || (lo - clon).abs() > 1e-6, "칸 정중앙과 달라야 한다");
    }

    /// 실제 HTTP 429가 enum 이름만 검사하는 가짜 시험이 아니라 ask의 중단 경로를 탄다.
    #[test]
    fn an_http_429_stops_the_run() {
        let mut server = TestServer::once("429 Too Many Requests", r#"{"error":"slow down"}"#);
        match ask(&test_client(), &server.url, 37.5, 127.0, 12) {
            Answer::Backoff(msg) => assert!(msg.contains("429")),
            _ => panic!("백오프여야 한다"),
        }
        assert_eq!(server.served(), 1);
    }

    #[test]
    fn an_error_hidden_in_a_200_response_stops_instead_of_becoming_a_cache_miss() {
        let mut limited = TestServer::once("200 OK", r#"{"error":"rate limit exceeded"}"#);
        assert!(matches!(ask(&test_client(), &limited.url, 37.5, 127.0, 12), Answer::Backoff(_)));
        assert_eq!(limited.served(), 1);

        let mut nowhere = TestServer::once("200 OK", r#"{"error":"Unable to geocode"}"#);
        assert!(matches!(ask(&test_client(), &nowhere.url, 37.5, 127.0, 12), Answer::Nothing));
        assert_eq!(nowhere.served(), 1);
    }

    /// HTTP 성공이어도 국가가 없는 부분 응답은 성공 캐시로 저장하지 않는다.
    #[test]
    fn a_partial_place_without_a_country_is_not_success() {
        let mut server = TestServer::once("200 OK", r#"{"address":{"city":"서울특별시","borough":"서초구"}}"#);
        assert!(matches!(ask(&test_client(), &server.url, 37.5, 127.0, 12), Answer::Nothing));
        assert_eq!(server.served(), 1);
    }

    /// 아무도 연결하지 않아도 서버 스레드는 제 마감으로 끝난다 —
    /// 시험이 실패 대신 영원히 매달리던 것을 막는다 (2026-09-01 리뷰)
    #[test]
    fn the_test_server_stops_itself_when_nobody_connects() {
        let mut server = TestServer::once("200 OK", "{}");
        assert_eq!(server.served(), 0, "요청이 없었다");
    }

    /// 멈추면 그때까지 채운 것은 남는다
    #[test]
    fn cancelling_keeps_what_was_already_named() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2846,127.0512);",
            )
        })
        .unwrap();
        let p = fill(&db, &AtomicBool::new(true), None, |_| {}).unwrap();
        assert_eq!((p.asked, p.files), (0, 0), "멈춤 상태면 아무것도 묻지 않는다");
    }
}
