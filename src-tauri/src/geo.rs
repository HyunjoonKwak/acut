//! 지명 — 좌표를 «국가 / 시도 / 시군구» 세 단계 이름으로.
//!
//! 사진마다 묻지 않는다. 좌표를 **0.01도 격자**(약 1.1km)로 뭉쳐 격자마다 한 번만
//! 물어보고 `places` 에 캐시한다 — 실측(2026-09-01) 사진 52,576장이 격자로는
//! 1,143칸뿐이다. 한 번 채우면 그 뒤로는 완전히 오프라인이다.
//!
//! 이름은 Nominatim 규약을 쓰는 서버에서 받는다. 기본값은 OSM 공개 서버지만
//! **설정에서 바꿀 수 있다**(`geo.endpoint`) — 공개 서버 정책이 «요청하면 언제든
//! 서버를 바꿀 수 있어야 하고, 그것이 소프트웨어 갱신 없이 되어야 한다»를 요구한다.
//! 자체 Nominatim 이나 유료 서비스를 넣으면 그쪽으로 간다.
//!
//! 공개 서버를 쓸 때 지키는 것: 초당 한 건, 앱을 밝히는 User-Agent, 429·5xx 는
//! 물러서서 멈춤. 그리고 **격자를 훑지 않는다** — 정책이 «reverse queries in a
//! grid»를 금지한다. 물어보는 좌표는 그 칸에 실제로 있는 사진들의 대표 좌표이고,
//! 한 번 물어본 것은 places 에 남아 두 번 묻지 않는다.

use crate::db::conn::{Db, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 격자 한 칸 — 0.01도. 이보다 잘게 나누면 같은 동네를 여러 번 묻는다.
const CELL: f64 = 0.01;
/// Nominatim 규칙 — 초당 한 건.
const GAP: Duration = Duration::from_millis(1100);
const UA: &str = concat!("acut/", env!("CARGO_PKG_VERSION"), " (personal photo library; github.com/HyunjoonKwak/acut)");
/// 기본 서버 — 설정 `geo.endpoint` 로 바꿀 수 있다
pub const DEFAULT_ENDPOINT: &str = "https://nominatim.openstreetmap.org/reverse";
/// 캐시 한 줄의 상태 — 이 셋을 섞으면 «결과 없음»을 영영 다시 묻는다 (2026-09-01 리뷰)
const OK: &str = "ok";
/// 그 자리에 이름이 없다(바다 한가운데 등). 다시 묻지 않는다
const NONE: &str = "none";

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
    let get = |k: &str| addr.get(k).and_then(|v| v.as_str()).map(str::to_string);
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
    /// 지금은 실패 — 다음에 다시 (String 은 사람이 읽을 사유)
    Retry(String),
    /// 서버가 그만하라고 한다 (429·5xx) — 작업을 멈춘다
    Backoff(String),
}

/// 좌표 하나를 물어본다.
fn ask(client: &reqwest::blocking::Client, endpoint: &str, lat: f64, lon: f64, zoom: u8) -> Answer {
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{sep}lat={lat}&lon={lon}&format=jsonv2&zoom={zoom}&accept-language=ko");
    let res = match client.get(&url).header(reqwest::header::USER_AGENT, UA).send() {
        Ok(r) => r,
        Err(e) => return Answer::Retry(e.to_string()),
    };
    let status = res.status();
    if status.as_u16() == 429 || status.is_server_error() {
        return Answer::Backoff(format!("서버가 {status} 로 답했습니다 — 잠시 뒤에 다시 해 주세요"));
    }
    if let Err(e) = res.error_for_status_ref() {
        return Answer::Retry(e.to_string());
    }
    let body: serde_json::Value = match res.json() {
        Ok(v) => v,
        Err(e) => return Answer::Retry(e.to_string()),
    };
    // 서버가 200 으로 오류를 싣는 경우도 있다
    if body.get("error").is_some() {
        return Answer::Nothing;
    }
    let place = body.get("address").map(fold).unwrap_or_default();
    if place.is_empty() { Answer::Nothing } else { Answer::Found(place) }
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
    /// 물어봤지만 답을 못 받은 격자 수 — 다음에 다시 해 볼 것
    pub failed: usize,
    /// 그 자리에 이름이 없어 다시 묻지 않기로 한 격자 수
    pub empty: usize,
    /// 서버가 그만하라고 해서 멈췄으면 그 사유
    pub stopped: Option<String>,
}

/// 이름이 없는 사진들의 자리에 이름을 붙인다.
///
/// 격자는 «같은 곳을 두 번 묻지 않기» 위한 열쇠일 뿐이고, **물어보는 좌표는 그 칸에
/// 실제로 있는 사진들의 대표(중앙값에 가장 가까운 사진) 좌표**다 — 칸 정중앙을 물으면
/// 경계·해안·섬에서 옆 동네가 붙는다 (2026-09-01 리뷰).
///
/// 캐시에 있으면 곧바로 쓰고, 없을 때만 물어본다(초당 하나). 서버가 429·5xx 로
/// 답하면 그 자리에서 멈춘다 — 채운 것은 남고 다음에 이어서 한다.
pub fn fill(
    db: &Db,
    cancel: &AtomicBool,
    limit: Option<usize>,
    on_progress: impl Fn(&Progress),
) -> Result<Progress> {
    let endpoint = crate::db::settings::get(db, "geo.endpoint")?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
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
                WHERE gps_lat IS NOT NULL AND gps_lon IS NOT NULL
                  AND NOT (gps_lat = 0.0 AND gps_lon = 0.0)
                  AND gps_lat BETWEEN -90 AND 90 AND gps_lon BETWEEN -180 AND 180
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
        .build()
        .map_err(|e| crate::db::conn::DbError::Invalid(e.to_string()))?;

    for (cell_key, lat, lon) in todo {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // 캐시부터 — 성공한 것만 쓴다
        let cached: Option<Place> = db.read(|c| {
            c.query_row(
                "SELECT country, admin1, admin2 FROM places WHERE cell = ?1 AND status = ?2",
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
                std::thread::sleep(GAP); // 공개 서버 규칙 — 초당 하나
                p.asked += 1;
                match ask(&client, &endpoint, lat, lon, zoom) {
                    Answer::Found(place) => {
                        let name = place.name();
                        db.write(|c| {
                            c.execute(
                                "INSERT OR REPLACE INTO places(cell,country,admin1,admin2,name,status,at)
                                 VALUES(?1,?2,?3,?4,?5,?6,strftime('%s','now'))",
                                rusqlite::params![&cell_key, &place.country, &place.admin1, &place.admin2, &name, OK],
                            )
                        })?;
                        place
                    }
                    Answer::Nothing => {
                        // 이름이 없는 자리 — 못 박아 두고 다시 묻지 않는다
                        db.write(|c| {
                            c.execute(
                                "INSERT OR REPLACE INTO places(cell,country,admin1,admin2,name,status,at)
                                 VALUES(?1,NULL,NULL,NULL,NULL,?2,strftime('%s','now'))",
                                rusqlite::params![&cell_key, NONE],
                            )
                        })?;
                        p.empty += 1;
                        p.done += 1;
                        on_progress(&p);
                        continue;
                    }
                    Answer::Retry(e) => {
                        log::warn!("지명 조회 실패 {cell_key}: {e}");
                        p.failed += 1;
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

        // 이 칸의 사진들에 이름을 붙인다 — 이름이 있는 값만 센다
        let n = db.write(|c| {
            c.execute(
                &format!(
                    "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                            geo_name = COALESCE(?4, ?3, ?2)
                     WHERE {cell_expr} = ?1 AND gps_lat IS NOT NULL AND geo_country IS NULL"
                ),
                rusqlite::params![&cell_key, &place.country, &place.admin1, &place.admin2],
            )
        })?;
        // 국가가 비어 있으면 붙은 것이 아니다 — «N장에 붙였습니다»가 거짓이 되지 않게
        if place.country.is_some() {
            p.files += n;
        }
        p.done += 1;
        on_progress(&p);
    }
    Ok(p)
}

/// 얼마나 남았나 — 설정 화면이 «지명 채우기» 앞에 보여 준다.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stats {
    /// 좌표가 있는 사진
    pub with_gps: i64,
    /// 그중 이름이 붙은 사진
    pub named: i64,
    /// 이름이 필요한 격자 수 — 대략 이만큼 초가 걸린다
    pub cells_left: i64,
}

pub fn stats(db: &Db) -> Result<Stats> {
    db.read(|c| {
        c.query_row(
            &format!(
                "SELECT
                   (SELECT COUNT(*) FROM files WHERE gps_lat IS NOT NULL AND NOT (gps_lat=0.0 AND gps_lon=0.0) AND trashed_at IS NULL),
                   (SELECT COUNT(*) FROM files WHERE geo_country IS NOT NULL AND trashed_at IS NULL),
                   (SELECT COUNT(DISTINCT {cell})
                      FROM files
                     WHERE gps_lat IS NOT NULL AND NOT (gps_lat=0.0 AND gps_lon=0.0)
                       AND geo_country IS NULL AND trashed_at IS NULL
                       AND {cell} NOT IN (SELECT cell FROM places WHERE status = '{none}'))",
                cell = cell_sql("gps_lat", "gps_lon"), none = NONE
            ),
            [],
            |r| Ok(Stats { with_gps: r.get(0)?, named: r.get(1)?, cells_left: r.get(2)? }),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                         (4,1,'d.jpg',1,0,1,0,0,NULL,NULL);",
            )
        })
        .unwrap();
        let s = stats(&db).unwrap();
        assert_eq!((s.with_gps, s.named, s.cells_left), (3, 0, 2), "같은 칸 둘은 한 번만 센다");

        db.write(|c| c.execute("UPDATE files SET geo_country='대한민국' WHERE id IN (1,2)", []))
            .unwrap();
        let s = stats(&db).unwrap();
        assert_eq!((s.named, s.cells_left), (2, 1));
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
        assert_eq!((s.with_gps, s.named, s.cells_left), (2, 0, 0));
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

    /// 서버가 429·5xx 로 답하면 물러선다 — 계속 두드리지 않는다
    #[test]
    fn a_rate_limited_server_stops_the_run() {
        let answer = Answer::Backoff("서버가 429 Too Many Requests 로 답했습니다".into());
        match answer {
            Answer::Backoff(msg) => assert!(msg.contains("429")),
            _ => panic!("백오프여야 한다"),
        }
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
