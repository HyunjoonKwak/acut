//! 좌표 하나를 세 단계 지명으로 — 서버 없이.
//!
//! 두 자료를 **서로 대조**한다. 경계 폴리곤은 «여기가 어느 구역인가»를 알고,
//! 도시 표는 «이 근처에 무엇이 있고 그 행정 소속은 무엇인가»를 안다. 하나만
//! 믿으면 각자의 구멍에 빠진다:
//!
//! - 폴리곤만 믿으면: 내장한 Natural Earth 판이 인천의 섬(강화·백령·연평)을
//!   경기도로 적어 두어 강화도 사진이 경기도가 된다.
//! - 도시만 믿으면: 지리산 천왕봉에서 가장 가까운 도시가 28km 밖 다른 도에
//!   있어 도가 틀린다.
//!
//! 그래서 폴리곤을 기본으로 삼되, **아주 가까운**(5km) 도시가 다른 시도를
//! 가리키면 도시를 따른다. 그 거리면 도시 한복판이라 폴리곤 쪽이 낡은 것이다.

use super::boundary;
use super::offline;
use super::Place;

/// 이 거리 안의 도시는 폴리곤보다 믿는다 — 도시 중심에서 5km 면 사실상 그 도시 안이다
const TRUST_KM: f64 = 5.0;
/// 시군구 이름을 빌려올 수 있는 최대 거리. 이보다 멀면 이름을 붙이지 않는다.
const DISTRICT_KM: f64 = 20.0;

/// 오프라인 판정 결과
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub place: Place,
    /// 이름을 빌려온 도시까지의 거리(km). 폴리곤만으로 정했으면 None.
    pub distance_km: Option<f64>,
    /// 경계로 확정했나(boundary) 아니면 가까운 도시에서 빌렸나(approximate)
    pub precision: &'static str,
}

/// 좌표 하나를 푼다. 나라조차 모르면 None — 그 자리는 온라인이 다시 시도한다.
pub fn resolve(lat: f64, lon: f64) -> Option<Resolved> {
    let cc = boundary::country(lat, lon)?;
    let country = offline::country_name(&cc).map(str::to_string).unwrap_or_else(|| cc.clone());

    if cc == "KR" {
        return Some(korea(lat, lon, country));
    }

    // 한국 밖: 시도 폴리곤이 없다. 가까운 도시에서 빌리되 나라가 같아야 한다.
    match offline::nearest_in(lat, lon, DISTRICT_KM, &cc) {
        Some(near) => Some(Resolved {
            place: Place {
                country: Some(country),
                admin1: non_empty(near.city.admin1_name),
                admin2: non_empty(near.city.district()),
            },
            distance_km: Some(near.km),
            precision: super::PREC_APPROX,
        }),
        // 사막·바다 한가운데의 섬 — 나라는 확실하니 그것만 붙인다
        None => Some(Resolved {
            place: Place { country: Some(country), admin1: None, admin2: None },
            distance_km: None,
            precision: super::PREC_BOUNDARY,
        }),
    }
}

/// 한국 — 시도는 폴리곤이 정하고, 아주 가까운 도시가 다르면 도시를 따른다
fn korea(lat: f64, lon: f64, country: String) -> Resolved {
    let by_polygon = boundary::kr_admin1(lat, lon);
    let near = offline::nearest_in(lat, lon, DISTRICT_KM, "KR");

    // 5km 안의 도시가 다른 시도를 가리키면 폴리곤이 낡은 것이다 (강화도)
    let corrected = near
        .filter(|n| n.km <= TRUST_KM)
        .and_then(|n| boundary::kr_admin1_by_geonames(n.city.admin1_code))
        .filter(|r| !by_polygon.is_some_and(|p| p.code == r.code));

    let region = corrected.or(by_polygon);
    let admin1 = region.map(|r| r.name.clone());

    // 시군구는 시도가 같은 도시에서만 빌린다 — 도 경계 너머 이름이 넘어오지 않게
    let district = near.filter(|n| match region {
        Some(r) => n.city.admin1_code == r.geonames_admin1,
        None => false,
    });

    // 시도와 같은 이름은 시군구로 쓰지 않는다 (서울특별시 › 서울특별시)
    let admin2 = district
        .map(|n| n.city.district())
        .and_then(non_empty)
        .filter(|d| Some(d) != admin1.as_ref());

    Resolved {
        place: Place { country: Some(country), admin1, admin2 },
        distance_km: district.map(|n| n.km),
        // 도시에서 이름을 빌렸으면 근사, 폴리곤만으로 정했으면 경계
        precision: if district.is_some() || corrected.is_some() {
            super::PREC_APPROX
        } else {
            super::PREC_BOUNDARY
        },
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

/// 이 도시가 그 시도에 속하나 — 시험에서 읽기 좋게 뽑아 둔다
#[cfg(test)]
fn in_region(city: &offline::City, code: &str) -> bool {
    boundary::kr_admin1_by_geonames(city.admin1_code).is_some_and(|r| r.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> Resolved {
        resolve(lat, lon).unwrap_or_else(|| panic!("{lat},{lon} 을 풀지 못했습니다"))
    }

    #[test]
    fn a_city_centre_gets_all_three_levels() {
        let r = at(37.2911, 127.0089); // 수원 한복판
        assert_eq!(r.place.country.as_deref(), Some("대한민국"));
        assert_eq!(r.place.admin1.as_deref(), Some("경기도"));
        assert_eq!(r.place.admin2.as_deref(), Some("수원시"));
        assert!(r.distance_km.is_some_and(|d| d < 2.0));
        assert_eq!(r.precision, crate::geo::PREC_APPROX);
    }

    /// 산속 — 가까운 도시가 없으니 시도까지만. 28km 밖 도시의 도를 끌어오지 않는다.
    #[test]
    fn a_mountain_top_keeps_the_province_the_polygon_says() {
        let r = at(35.3369, 127.7306); // 지리산 천왕봉
        assert_eq!(r.place.admin1.as_deref(), Some("경상남도"));
        assert_eq!(r.place.admin2, None, "20km 밖 도시의 이름을 빌려 오면 안 된다");
        assert_eq!(r.precision, crate::geo::PREC_BOUNDARY);
    }

    /// 내장 폴리곤은 강화도를 경기도라 하지만, 0.2km 앞 도시 표는 인천이라 한다 — 도시가 이긴다
    #[test]
    fn a_very_near_city_corrects_a_stale_polygon() {
        assert_eq!(boundary::kr_admin1(37.7469, 126.4880).map(|r| r.name.as_str()), Some("경기도"));
        let r = at(37.7469, 126.4880);
        assert_eq!(r.place.admin1.as_deref(), Some("인천광역시"));
        assert_eq!(r.place.admin2.as_deref(), Some("강화군"));
    }

    /// 그 보정이 아무 데서나 발동하면 안 된다 — 도 경계 근처에서 도가 흔들린다
    #[test]
    fn a_far_city_never_overrides_the_polygon() {
        // 김포(경기) 한복판 — 폴리곤도 도시도 경기도라 바뀔 것이 없다
        let r = at(37.6236, 126.7142);
        assert_eq!(r.place.admin1.as_deref(), Some("경기도"));
        assert_eq!(r.place.admin2.as_deref(), Some("김포시"));
    }

    #[test]
    fn a_province_never_repeats_itself_as_a_district() {
        let r = at(37.5665, 126.9780); // 서울 한복판
        assert_eq!(r.place.admin1.as_deref(), Some("서울특별시"));
        assert_ne!(r.place.admin2.as_deref(), Some("서울특별시"), "시도와 같은 이름은 시군구가 아니다");
    }

    /// **독도는 한국 땅이다** — 오프라인 판정 전체를 통과한 결과로도 그렇다
    #[test]
    fn dokdo_resolves_to_korea() {
        let r = at(37.2411, 131.8694);
        assert_eq!(r.place.country.as_deref(), Some("대한민국"));
        assert_eq!(r.place.admin1.as_deref(), Some("경상북도"));
        assert_eq!(r.place.name().as_deref(), Some("경상북도"), "울릉도 도시가 20km 밖이라 시도까지만 나온다");
    }

    #[test]
    fn abroad_borrows_the_province_from_the_nearest_city() {
        let r = at(21.3069, -157.8583); // 호놀룰루
        assert_eq!(r.place.country.as_deref(), Some("미국"));
        assert_eq!(r.place.admin1.as_deref(), Some("Hawaii"));
        assert_eq!(r.place.admin2.as_deref(), Some("Honolulu"));

        let r = at(35.6762, 139.6503); // 도쿄
        assert_eq!(r.place.country.as_deref(), Some("일본"));
        assert_eq!(r.place.admin1.as_deref(), Some("Tokyo"));
    }

    /// 도시가 멀어도 나라는 확실하다 — 나라만 붙이고 «경계» 정밀도로 남긴다
    #[test]
    fn a_remote_place_still_gets_its_country() {
        let r = at(64.0, -50.0); // 그린란드 내륙
        assert!(r.place.country.is_some());
        assert_eq!(r.place.admin1, None);
        assert_eq!(r.precision, crate::geo::PREC_BOUNDARY);
    }

    #[test]
    fn the_open_sea_is_left_for_the_online_pass() {
        assert_eq!(resolve(38.5, 131.5), None);
        assert_eq!(resolve(0.0, -140.0), None);
    }

    #[test]
    fn a_borrowed_name_never_crosses_a_province_line() {
        // 도시가 20km 안에 있어도 시도가 다르면 시군구를 빌리지 않는다
        let r = at(35.3369, 127.7306);
        assert_eq!(r.place.admin2, None);
        // 그 근처 도시가 실제로 다른 도에 있는지 확인해 시험의 전제를 못 박는다
        let near = offline::nearest_in(35.3369, 127.7306, 40.0, "KR").unwrap();
        assert!(near.km > DISTRICT_KM || !in_region(&near.city, "KR-48"));
    }
}
