//! 지명 — 좌표를 «국가 / 시도 / 시군구» 세 단계 이름으로.
//!
//! 사진마다 묻지 않는다. 좌표를 **0.01도 격자**(약 1.1km)로 뭉쳐 격자마다 한 번만
//! 물어보고 `places` 에 캐시한다 — 실측(2026-09-01) 사진 52,576장이 격자로는
//! 1,143칸뿐이다. 한 번 채우면 그 뒤로는 완전히 오프라인이다.
//!
//! 이름은 OpenStreetMap 의 Nominatim 에서 한국어로 받는다. 키가 없고 전 세계를
//! 덮는다 — 카카오는 국내 전용이라 해외 사진 7,003장이 이름 없이 남고, 유료
//! 서비스는 배포되는 앱에 키를 넣어야 해서 꺼낼 수 있다.
//!
//! Nominatim 이용 규칙을 지킨다: 초당 한 건, 앱을 밝히는 User-Agent.

use crate::db::conn::{Db, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 격자 한 칸 — 0.01도. 이보다 잘게 나누면 같은 동네를 여러 번 묻는다.
const CELL: f64 = 0.01;
/// Nominatim 규칙 — 초당 한 건.
const GAP: Duration = Duration::from_millis(1100);
const UA: &str = concat!("acut/", env!("CARGO_PKG_VERSION"), " (personal photo library; github.com/HyunjoonKwak/acut)");

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

/// 좌표 하나를 물어본다. 실패하면 Err — 부르는 쪽이 건너뛴다.
fn ask(client: &reqwest::blocking::Client, lat: f64, lon: f64) -> std::result::Result<Place, String> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=jsonv2&zoom=10&accept-language=ko"
    );
    let body: serde_json::Value = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, UA)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    Ok(body.get("address").map(fold).unwrap_or_default())
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
    /// 물어봤지만 답을 못 받은 격자 수
    pub failed: usize,
}

/// 이름이 없는 사진들의 격자를 모아 하나씩 이름을 붙인다.
///
/// 캐시에 있으면 곧바로 쓰고, 없을 때만 물어본다(초당 하나). 멈추면 그때까지
/// 채운 것은 남는다 — 다음에 부르면 이어서 한다.
pub fn fill(
    db: &Db,
    cancel: &AtomicBool,
    on_progress: impl Fn(&Progress),
) -> Result<Progress> {
    // 1) 이름이 필요한 격자 — 좌표가 있고 아직 국가가 비어 있는 사진들
    let cells: Vec<String> = db.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT DISTINCT {}
             FROM files
             WHERE gps_lat IS NOT NULL AND gps_lon IS NOT NULL
               AND NOT (gps_lat = 0.0 AND gps_lon = 0.0)
               AND gps_lat BETWEEN -90 AND 90 AND gps_lon BETWEEN -180 AND 180
               AND geo_country IS NULL AND trashed_at IS NULL",
            cell_sql("gps_lat", "gps_lon")
        ))?;
        let it = st.query_map([], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<String>>>()
    })?;

    let mut p = Progress { total: cells.len(), ..Default::default() };
    on_progress(&p);
    if cells.is_empty() {
        return Ok(p);
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| crate::db::conn::DbError::Invalid(e.to_string()))?;

    for cell_key in cells {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // 캐시부터
        let cached: Option<Place> = db.read(|c| {
            c.query_row(
                "SELECT country, admin1, admin2 FROM places WHERE cell = ?1",
                [&cell_key],
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
                let Some((lat, lon)) = cell_center(&cell_key) else {
                    p.done += 1;
                    continue;
                };
                std::thread::sleep(GAP); // Nominatim 규칙 — 초당 하나
                p.asked += 1;
                match ask(&client, lat, lon) {
                    Ok(place) if !place.is_empty() => {
                        let name = place.name();
                        db.write(|c| {
                            c.execute(
                                "INSERT OR REPLACE INTO places(cell,country,admin1,admin2,name,at)
                                 VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))",
                                rusqlite::params![&cell_key, &place.country, &place.admin1, &place.admin2, &name],
                            )
                        })?;
                        place
                    }
                    Ok(_) => {
                        // 바다 한가운데 등 — 이름이 없다. 다시 묻지 않게 빈 줄을 남긴다
                        db.write(|c| {
                            c.execute(
                                "INSERT OR REPLACE INTO places(cell,country,admin1,admin2,name,at)
                                 VALUES(?1,NULL,NULL,NULL,NULL,strftime('%s','now'))",
                                [&cell_key],
                            )
                        })?;
                        p.done += 1;
                        on_progress(&p);
                        continue;
                    }
                    Err(e) => {
                        log::warn!("지명 조회 실패 {cell_key}: {e}");
                        p.failed += 1;
                        p.done += 1;
                        on_progress(&p);
                        continue;
                    }
                }
            }
        };

        // 이 격자의 사진들에 이름을 붙인다
        let n = db.write(|c| {
            c.execute(
                &format!(
                    "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                            geo_name = COALESCE(?4, ?3, ?2)
                     WHERE {} = ?1 AND gps_lat IS NOT NULL AND geo_country IS NULL",
                    cell_sql("gps_lat", "gps_lon")
                ),
                rusqlite::params![&cell_key, &place.country, &place.admin1, &place.admin2],
            )
        })?;
        p.files += n;
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
                   (SELECT COUNT(DISTINCT {})
                      FROM files
                     WHERE gps_lat IS NOT NULL AND NOT (gps_lat=0.0 AND gps_lon=0.0)
                       AND geo_country IS NULL AND trashed_at IS NULL)",
                cell_sql("gps_lat", "gps_lon")
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
                 INSERT INTO places(cell,country,admin1,admin2,name,at)
                   VALUES('37.28,127.05','대한민국','경기도','수원시','수원시',0);",
            )
        })
        .unwrap();

        let p = fill(&db, &AtomicBool::new(false), |_| {}).unwrap();
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
        let again = fill(&db, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(again.total, 0);
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
        let p = fill(&db, &AtomicBool::new(true), |_| {}).unwrap();
        assert_eq!((p.asked, p.files), (0, 0), "멈춤 상태면 아무것도 묻지 않는다");
    }
}
