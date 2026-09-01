//! 서버 없이 도시를 찾는다 — GeoNames cities15000 스냅샷을 그대로 안고 다닌다.
//!
//! 크레이트(`reverse_geocoder`)를 쓰지 않은 이유: 의존성 55개, 바이너리 +7.65MB,
//! 그리고 `fixed` 가 요구하는 rustc 1.85 가 이 앱의 선언 MSRV(1.77.2)와 어긋난다.
//! 여기서 필요한 것은 «가장 가까운 도시» 하나뿐이라 전수 탐색으로 충분하다
//! (34,127행 · 위도 창으로 먼저 자른다).
//!
//! 데이터는 `scripts/build-geodata.mjs` 가 사람 손으로 만든다. 빌드나 실행 중에
//! 내려받는 일은 없다.

use std::sync::OnceLock;

/// 스냅샷 원문 — 바이너리 안에 그대로 들어간다
const CITIES: &str = include_str!("../../data/geonames/cities.tsv");
const MANIFEST: &str = include_str!("../../data/geonames/MANIFEST.json");
const COUNTRIES: &str = include_str!("../../data/boundaries/countries.tsv");

/// 지구 반지름(km) — 하버사인에 쓴다
const EARTH_KM: f64 = 6371.0088;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct City {
    pub lat: f64,
    pub lon: f64,
    /// 영문 이름 — 해외 지명은 이쪽이 알아보기 쉽다
    pub name: &'static str,
    /// 한글 이름 (한국 도시에만 있다)
    pub name_ko: &'static str,
    pub cc: &'static str,
    /// GeoNames 시도 코드 — 경계 데이터와 대조하는 열쇠
    pub admin1_code: &'static str,
    pub admin1_name: &'static str,
    /// 시군구 이름 (한국에만 있다. 도시 이름은 동까지 내려가서 쓸 수 없다)
    pub admin2_name: &'static str,
    pub population: u32,
}

impl City {
    /// 보여 줄 이름 — 한글이 있으면 한글
    pub fn label(&self) -> &'static str {
        if self.name_ko.is_empty() { self.name } else { self.name_ko }
    }
    /// 시군구로 쓸 이름 — 한국은 시군구, 그 밖은 도시 이름
    pub fn district(&self) -> &'static str {
        if self.admin2_name.is_empty() { self.label() } else { self.admin2_name }
    }
}

/// 위도 오름차순으로 정렬된 도시들 — 위도 창을 이진 탐색으로 자르기 위해서다
fn cities() -> &'static [City] {
    static CACHE: OnceLock<Vec<City>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut out: Vec<City> = CITIES
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|line| {
                let mut f = line.split('\t');
                let lat: f64 = f.next()?.parse().ok()?;
                let lon: f64 = f.next()?.parse().ok()?;
                let name = f.next()?;
                let cc = f.next()?;
                let admin1_code = f.next()?;
                let admin1_name = f.next()?;
                let admin2_name = f.next()?;
                let population: u32 = f.next()?.parse().unwrap_or(0);
                // 한글 이름 칸은 비어 있을 수 있고, 줄 끝이라 아예 없을 수도 있다
                let name_ko = f.next().unwrap_or("");
                if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                    return None;
                }
                Some(City { lat, lon, name, name_ko, cc, admin1_code, admin1_name, admin2_name, population })
            })
            .collect();
        out.sort_by(|a, b| a.lat.total_cmp(&b.lat));
        out
    })
}

/// 스냅샷 판 번호 — 어느 데이터로 붙인 이름인지 캐시에 함께 적는다
pub fn dataset_version() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(MANIFEST)
            .ok()
            .and_then(|m| m.get("dataset_version").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_else(|| "unknown".into())
    })
}

/// 국가 코드 → 사람이 읽을 이름. 한글이 있으면 한글, 없으면 영문. 표에 없으면 None.
pub fn country_name(cc: &str) -> Option<&'static str> {
    static CACHE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    let table = CACHE.get_or_init(|| {
        let mut out: Vec<(&'static str, &'static str)> = COUNTRIES
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|line| {
                let mut f = line.split('\t');
                let cc = f.next()?;
                let name = f.next()?;
                let en = f.next().unwrap_or("");
                Some((cc, if name.is_empty() { en } else { name }))
            })
            .collect();
        out.sort_by_key(|(cc, _)| *cc);
        out
    });
    table.binary_search_by_key(&cc, |(c, _)| *c).ok().map(|i| table[i].1)
}

/// 두 점 사이 거리(km) — 하버사인
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_KM * a.sqrt().clamp(0.0, 1.0).asin()
}

#[derive(Debug, Clone, Copy)]
pub struct Near {
    pub city: City,
    pub km: f64,
}

/// 조건에 맞는 가장 가까운 도시. `max_km` 밖은 없는 것으로 친다.
///
/// 위도로 먼저 창을 잘라 34,127행 전부를 재지 않는다. 경도는 자르지 않는다 —
/// 극지방에서 창이 지구를 한 바퀴 돌아 오히려 느려지고, 남은 후보가 이미 적다.
pub fn nearest_where(lat: f64, lon: f64, max_km: f64, keep: impl Fn(&City) -> bool) -> Option<Near> {
    let all = cities();
    // 위도 1도는 어디서나 약 111km 다
    let pad = max_km / 110.0;
    let lo = all.partition_point(|c| c.lat < lat - pad);
    let hi = all.partition_point(|c| c.lat <= lat + pad);
    let mut best: Option<Near> = None;
    for city in &all[lo..hi] {
        if !keep(city) {
            continue;
        }
        let km = distance_km(lat, lon, city.lat, city.lon);
        if km > max_km {
            continue;
        }
        // 같은 거리면 인구가 많은 쪽 — 결과가 데이터 순서에 흔들리지 않게
        let better = match &best {
            None => true,
            Some(b) => km < b.km || (km == b.km && city.population > b.city.population),
        };
        if better {
            best = Some(Near { city: *city, km });
        }
    }
    best
}

/// 나라를 가리지 않는 최근접
pub fn nearest(lat: f64, lon: f64, max_km: f64) -> Option<Near> {
    nearest_where(lat, lon, max_km, |_| true)
}

/// 그 나라 안에서만 찾는다 — 경계가 이미 나라를 정했을 때 쓴다
pub fn nearest_in(lat: f64, lon: f64, max_km: f64, cc: &str) -> Option<Near> {
    nearest_where(lat, lon, max_km, |c| c.cc == cc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 스냅샷이 바뀌면 이름이 조용히 달라진다 — 판마다 사람이 확인하도록 못 박는다
    #[test]
    fn the_snapshot_matches_its_manifest() {
        use sha2::{Digest, Sha256};
        let want = serde_json::from_str::<serde_json::Value>(MANIFEST).unwrap();
        let want = want["sha256"].as_str().unwrap();
        let got = format!("{:x}", Sha256::digest(CITIES.as_bytes()));
        assert_eq!(got, want, "cities.tsv 가 MANIFEST 와 다릅니다 — build-geodata.mjs 를 다시 돌리세요");
        assert_eq!(cities().len(), want_count(), "행 수가 MANIFEST 와 다릅니다");
    }

    fn want_count() -> usize {
        serde_json::from_str::<serde_json::Value>(MANIFEST).unwrap()["record_count"].as_u64().unwrap() as usize
    }

    #[test]
    fn every_row_parses_into_a_city() {
        // filter_map 이 조용히 버린 행이 없어야 한다
        let raw = CITIES.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).count();
        assert_eq!(cities().len(), raw);
    }

    #[test]
    fn cities_are_sorted_by_latitude() {
        assert!(cities().windows(2).all(|w| w[0].lat <= w[1].lat));
    }

    #[test]
    fn it_finds_the_city_you_are_standing_in() {
        let n = nearest(37.5665, 126.9780, 20.0).unwrap();
        assert_eq!(n.city.cc, "KR");
        assert!(n.km < 1.0, "서울 한복판인데 {}km 라고 합니다", n.km);
    }

    /// 한국 도시는 한글로, 해외는 영문으로 보여 준다
    #[test]
    fn korean_cities_carry_korean_names() {
        let suwon = nearest(37.2911, 127.0089, 5.0).unwrap().city;
        assert_eq!(suwon.label(), "수원시");
        assert_eq!(suwon.district(), "수원시");
        let honolulu = nearest(21.3069, -157.8583, 20.0).unwrap().city;
        assert_eq!(honolulu.cc, "US");
        assert_eq!(honolulu.label(), "Honolulu");
    }

    /// 도시 이름이 동까지 내려가는 곳에서도 시군구는 시군구여야 한다
    #[test]
    fn a_district_is_never_a_neighbourhood() {
        let jeju = nearest_where(33.4689, 126.5275, 2.0, |c| c.name == "Ara-dong").unwrap().city;
        assert_eq!(jeju.label(), "Ara-dong", "도시 이름 자체는 원본 그대로다");
        assert_eq!(jeju.district(), "제주시", "시군구는 동이 아니라 시여야 한다");
    }

    /// 강화도는 GeoNames 상 인천이다 — 경계 데이터가 틀린 곳을 바로잡는 근거가 된다
    #[test]
    fn ganghwa_belongs_to_incheon_in_the_city_table() {
        let n = nearest_in(37.7469, 126.4880, 5.0, "KR").unwrap();
        assert_eq!(n.city.admin1_code, "12");
        assert_eq!(n.city.district(), "강화군");
        assert!(n.km < 1.0);
    }

    #[test]
    fn nothing_is_near_the_open_ocean() {
        assert!(nearest(0.0, -140.0, 20.0).is_none());
    }

    #[test]
    fn the_search_window_does_not_miss_a_city_just_inside_the_radius() {
        // 서울에서 남쪽으로 약 100km — 창(pad)이 좁으면 놓친다
        let far = nearest_where(36.6665, 126.9780, 120.0, |c| c.name == "Seoul");
        assert!(far.is_some(), "반경 안의 도시를 위도 창이 잘라 먹었습니다");
        assert!((far.unwrap().km - 100.0).abs() < 5.0);
    }

    #[test]
    fn country_names_are_korean_where_cldr_knows_them() {
        assert_eq!(country_name("KR"), Some("대한민국"));
        assert_eq!(country_name("JP"), Some("일본"));
        assert_eq!(country_name("US"), Some("미국"));
        assert_eq!(country_name("ZZ"), None, "모르는 코드는 지어내지 않는다");
    }

    #[test]
    fn distances_are_great_circle() {
        // 서울–부산 약 325km
        let d = distance_km(37.5665, 126.9780, 35.1796, 129.0756);
        assert!((d - 325.0).abs() < 10.0, "{d}km");
        assert_eq!(distance_km(37.5, 127.0, 37.5, 127.0), 0.0);
    }

    #[test]
    fn a_tie_goes_to_the_bigger_city() {
        // 같은 좌표에 두 도시가 있을 때 인구가 많은 쪽이 이긴다 (자료 순서에 흔들리지 않게)
        let n = nearest(37.5665, 126.9780, 30.0).unwrap();
        let all_within = cities().iter().filter(|c| distance_km(37.5665, 126.9780, c.lat, c.lon) <= n.km).count();
        assert!(all_within >= 1);
    }
}
