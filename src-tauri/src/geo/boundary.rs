//! 좌표가 어느 경계 안에 있나 — 나라와, 한국의 시도.
//!
//! 거리로 나라나 시도를 정하지 않는다. 가장 가까운 도시가 28km 밖에 있으면
//! 그 도시의 도(道)는 이 좌표의 도가 아니다 (지리산에서 실제로 틀렸다).
//! 나라는 `country-boundaries` 크레이트가, 한국 시도는 여기 내장한 폴리곤이 정한다.
//!
//! 전 세계 시도(Natural Earth 10m 전체 14.2MB)는 크기 때문에 넣지 않았다.
//! 한국 밖에서는 시도를 도시 표에서 가져오고 «근사»로 표시한다.

use country_boundaries::{CountryBoundaries, LatLon, BOUNDARIES_ODBL_360X180};
use std::sync::OnceLock;

const KR_ADMIN1: &str = include_str!("../../data/boundaries/kr_admin1.json");

/// 독도 — 정책으로 고정한다.
///
/// 경계 데이터가 무엇을 답하든(현재 판은 한국으로 답하고, Natural Earth 에는
/// 아예 없다) 이 좌표 둘레는 대한민국 경상북도 울릉군이다. 데이터 판이 바뀌어도
/// 흔들리지 않게 코드에 못 박고 회귀 시험으로 지킨다.
const DOKDO: (f64, f64) = (37.2411, 131.8694);
const DOKDO_KM: f64 = 5.0;

#[derive(Debug, Clone)]
pub struct Region {
    /// ISO 3166-2 — KR-11
    pub code: String,
    /// 한글 이름 — 서울특별시
    pub name: String,
    /// 이 시도를 가리키는 것으로 아는 이름들 — 한글·영문·로마자 표기
    aliases: Vec<String>,
    /// GeoNames 시도 코드 — 도시 표와 대조하는 열쇠
    pub geonames_admin1: String,
    /// [최소경도, 최소위도, 최대경도, 최대위도] — 폴리곤을 재기 전에 먼저 자른다
    bbox: [f64; 4],
    /// 폴리곤들. 각 폴리곤의 첫 링이 바깥, 나머지는 구멍이다.
    polys: Vec<Vec<Vec<[f64; 2]>>>,
}

#[derive(serde::Deserialize)]
struct RawRegion {
    code: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    geonames_admin1: String,
    bbox: [f64; 4],
    polys: Vec<Vec<Vec<[f64; 2]>>>,
}

#[derive(serde::Deserialize)]
struct RawDoc {
    regions: Vec<RawRegion>,
}

fn regions() -> &'static [Region] {
    static CACHE: OnceLock<Vec<Region>> = OnceLock::new();
    CACHE.get_or_init(|| {
        // 내장 자료가 깨져도 앱이 죽지는 않게 한다. 시도를 잃을 뿐 나라 판정은
        // 그대로 살아 있고, 사용자는 «지명이 나라까지만 나온다»를 겪는다.
        // 여기서 패닉하면 채우기 스레드가 죽어 화면의 진행 표시가 영영 남는다.
        let doc: RawDoc = match serde_json::from_str(KR_ADMIN1) {
            Ok(doc) => doc,
            Err(e) => {
                log::error!("내장된 시도 경계를 읽지 못했습니다: {e}");
                return Vec::new();
            }
        };
        doc.regions
            .into_iter()
            .map(|r| Region {
                code: r.code,
                name: r.name,
                aliases: r.aliases,
                geonames_admin1: r.geonames_admin1,
                bbox: r.bbox,
                polys: r.polys,
            })
            .collect()
    })
}

fn world() -> Option<&'static CountryBoundaries> {
    static CACHE: OnceLock<Option<CountryBoundaries>> = OnceLock::new();
    CACHE
        .get_or_init(
            || match CountryBoundaries::from_reader(BOUNDARIES_ODBL_360X180) {
                Ok(b) => Some(b),
                Err(e) => {
                    log::error!("내장된 국가 경계를 읽지 못했습니다: {e}");
                    None
                }
            },
        )
        .as_ref()
}

/// 이 좌표의 나라 — ISO 3166-1 alpha-2. 바다면 None.
pub fn country(lat: f64, lon: f64) -> Option<String> {
    // 독도는 데이터가 무엇을 답하든 한국이다
    if is_dokdo(lat, lon) {
        return Some("KR".into());
    }
    let b = world()?;
    let at = LatLon::new(lat, lon).ok()?;
    // 크레이트는 좁은 것부터 답한다(US-HI, US). 나라는 두 글자짜리다.
    b.ids(at)
        .into_iter()
        .find(|id| id.len() == 2)
        .map(str::to_string)
}

/// 독도 정책 구역 안인가
pub fn is_dokdo(lat: f64, lon: f64) -> bool {
    super::offline::distance_km(lat, lon, DOKDO.0, DOKDO.1) <= DOKDO_KM
}

/// 이 좌표의 한국 시도. 한국 밖이거나 폴리곤에 없으면 None.
pub fn kr_admin1(lat: f64, lon: f64) -> Option<&'static Region> {
    // 독도는 폴리곤에 없다 — 정책으로 울릉군이 있는 경상북도다
    if is_dokdo(lat, lon) {
        return regions().iter().find(|r| r.code == "KR-47");
    }
    regions().iter().find(|r| contains(r, lat, lon))
}

/// 시도 이름을 견주기 좋게 다듬는다.
///
/// 같은 곳을 부르는 말이 여럿이다 — «강원도»·«강원특별자치도»·«Gangwon-do»·
/// «Gangwon». 공백·붙임표를 지우고 소문자로 내린 뒤, 행정 단위 접미사를 뗀다.
/// 그래야 «경상북도»와 «경상남도»는 여전히 다르게 남는다.
fn normalize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    for suffix in [
        "특별자치도",
        "특별자치시",
        "특별시",
        "광역시",
        "자치도",
        "자치시",
        "teukbyeoljachido",
        "teukbyeolsi",
        "gwangyeoksi",
        "do",
        "si",
        "도",
        "시",
    ] {
        if let Some(rest) = s.strip_suffix(suffix) {
            if !rest.is_empty() {
                s = rest.to_string();
                break;
            }
        }
    }
    s
}

/// 이 이름이 그 시도를 가리키나 — 아는 표기 가운데 하나와 맞으면 그렇다.
///
/// 모르는 이름에는 «아니다»가 아니라 «모르겠다»가 맞다. 그래서 이 함수는
/// 판정을 내리는 곳(`admin1_matches`)에서만 쓰고, 결과가 없으면 다투지 않는다.
impl Region {
    fn names_match(&self, name: &str) -> bool {
        let want = normalize(name);
        !want.is_empty() && self.aliases.iter().any(|a| normalize(a) == want)
    }
}

/// 온라인이 말한 시도가 경계 판정과 어긋나나.
///
/// 셋 중 하나를 답한다: `Some(true)` 맞다 · `Some(false)` 어긋난다 ·
/// `None` 판단할 수 없다(우리가 모르는 표기이거나 한국 밖이다).
/// **모르는 것을 어긋난다고 하지 않는다** — 그러면 정상 응답까지 막힌다.
pub fn admin1_matches(lat: f64, lon: f64, name: &str) -> Option<bool> {
    let here = kr_admin1(lat, lon)?;
    if here.names_match(name) {
        return Some(true);
    }
    // 우리가 아는 다른 시도의 이름이라면 어긋난 것이다
    if regions().iter().any(|r| r.names_match(name)) {
        return Some(false);
    }
    None
}

/// GeoNames 시도 코드로 시도를 찾는다 — 도시 표가 가리키는 곳을 이름으로 바꾼다
pub fn kr_admin1_by_geonames(code: &str) -> Option<&'static Region> {
    regions().iter().find(|r| r.geonames_admin1 == code)
}

fn contains(r: &Region, lat: f64, lon: f64) -> bool {
    let [min_lon, min_lat, max_lon, max_lat] = r.bbox;
    if lon < min_lon || lon > max_lon || lat < min_lat || lat > max_lat {
        return false;
    }
    r.polys.iter().any(|poly| {
        let mut it = poly.iter();
        let Some(outer) = it.next() else { return false };
        in_ring(outer, lat, lon) && !it.any(|hole| in_ring(hole, lat, lon))
    })
}

/// 광선 교차 판정. 링은 닫혀 있지 않다 — 마지막 점과 첫 점을 이어서 센다.
fn in_ring(ring: &[[f64; 2]], lat: f64, lon: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let ([xi, yi], [xj, yj]) = (ring[i], ring[j]);
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boundary_data_matches_its_manifest() {
        use sha2::{Digest, Sha256};
        let m: serde_json::Value =
            serde_json::from_str(include_str!("../../data/boundaries/MANIFEST.json")).unwrap();
        let got = format!("{:x}", Sha256::digest(KR_ADMIN1.as_bytes()));
        assert_eq!(
            got,
            m["sha256"].as_str().unwrap(),
            "kr_admin1.json 이 MANIFEST 와 다릅니다"
        );
        assert_eq!(regions().len(), 17, "한국 시도는 17개다");
    }

    #[test]
    fn every_region_knows_what_it_is_called() {
        for r in regions() {
            assert!(
                r.aliases.iter().any(|a| a == &r.name),
                "{} 의 별칭에 제 이름이 없습니다",
                r.name
            );
            assert!(r.aliases.len() >= 2, "{} 에 영문 표기가 없습니다", r.name);
            assert!(r.names_match(&r.name));
        }
        // 접미사를 떼도 서로 다른 도는 다르게 남아야 한다
        assert_ne!(normalize("경상북도"), normalize("경상남도"));
        assert_ne!(normalize("충청북도"), normalize("충청남도"));
        assert_eq!(normalize("강원도"), normalize("강원특별자치도"));
        assert_eq!(normalize("Gangwon-do"), normalize("gangwon"));
        // 한글과 영문은 정규화로 같아지지 않는다 — 그것이 별칭 목록이 있는 이유다
        let seoul = regions().iter().find(|r| r.code == "KR-11").unwrap();
        assert!(seoul.names_match("서울특별시") && seoul.names_match("Seoul"));
        assert!(!seoul.names_match("부산광역시"));
    }

    /// 같은 나라 안에서도 시도가 어긋나면 알아채야 한다
    #[test]
    fn it_can_tell_a_wrong_province_from_an_unknown_one() {
        // 수원 — 경기도
        assert_eq!(admin1_matches(37.2911, 127.0089, "경기도"), Some(true));
        assert_eq!(admin1_matches(37.2911, 127.0089, "Gyeonggi-do"), Some(true));
        assert_eq!(
            admin1_matches(37.2911, 127.0089, "경상북도"),
            Some(false),
            "다른 도는 어긋난 것이다"
        );
        assert_eq!(
            admin1_matches(37.2911, 127.0089, "Gyeongsangbuk-do"),
            Some(false)
        );
        // 우리가 모르는 이름에는 다투지 않는다 — 시군구나 옛 이름일 수 있다
        assert_eq!(admin1_matches(37.2911, 127.0089, "수원시"), None);
        assert_eq!(admin1_matches(37.2911, 127.0089, ""), None);
        // 한국 밖에는 견줄 경계가 없다
        assert_eq!(admin1_matches(35.6762, 139.6503, "Tokyo"), None);
        // **독도** — 경상북도가 맞고, 시마네는 아니다
        assert_eq!(admin1_matches(37.2411, 131.8694, "경상북도"), Some(true));
        assert_eq!(admin1_matches(37.2411, 131.8694, "강원도"), Some(false));
    }

    #[test]
    fn every_region_has_a_korean_name_and_a_city_table_key() {
        for r in regions() {
            assert!(r.code.starts_with("KR-"), "{}", r.code);
            assert!(
                !r.name.is_empty() && r.name.chars().any(|c| ('가'..='힣').contains(&c)),
                "{}",
                r.name
            );
            assert_eq!(
                r.geonames_admin1.len(),
                2,
                "{} 의 도시표 열쇠가 이상합니다",
                r.name
            );
            assert!(kr_admin1_by_geonames(&r.geonames_admin1).is_some());
        }
    }

    #[test]
    fn it_names_the_country_you_are_in() {
        assert_eq!(country(37.5665, 126.9780).as_deref(), Some("KR"));
        assert_eq!(country(35.6762, 139.6503).as_deref(), Some("JP"));
        assert_eq!(country(21.3069, -157.8583).as_deref(), Some("US"));
        assert_eq!(country(-33.8688, 151.2093).as_deref(), Some("AU"));
    }

    #[test]
    fn the_open_sea_belongs_to_no_one() {
        assert_eq!(country(38.5, 131.5), None);
        assert_eq!(country(0.0, -140.0), None);
        assert_eq!(country(91.0, 0.0), None, "지구 밖 좌표는 답이 없다");
    }

    /// **독도는 한국 땅이다.** 경계 데이터가 어떤 판으로 바뀌어도 이 답은 바뀌지 않는다.
    #[test]
    fn dokdo_is_korean() {
        assert_eq!(country(DOKDO.0, DOKDO.1).as_deref(), Some("KR"));
        let r = kr_admin1(DOKDO.0, DOKDO.1).expect("독도의 시도가 없습니다");
        assert_eq!(r.name, "경상북도");
        // 둘레 5km 안은 모두 같은 답 — 배 위에서 찍은 사진도 포함한다
        assert!(is_dokdo(DOKDO.0 + 0.03, DOKDO.1));
        assert_eq!(country(DOKDO.0 + 0.03, DOKDO.1).as_deref(), Some("KR"));
        // 그 밖은 정책 구역이 아니다 — 울릉도까지 끌고 오지 않는다
        assert!(!is_dokdo(37.4844, 130.9057));
    }

    /// 폴리곤 데이터에 독도가 아예 없다는 사실 자체를 못 박는다 —
    /// 나중에 데이터가 바뀌어 «누군가의 땅»으로 들어와도 정책이 이긴다
    #[test]
    fn the_policy_wins_even_though_the_polygons_have_no_dokdo() {
        let by_polygon = regions().iter().find(|r| contains(r, DOKDO.0, DOKDO.1));
        assert!(
            by_polygon.is_none(),
            "폴리곤이 독도를 담게 되었습니다 — 정책 우선 순서를 다시 확인하세요"
        );
        assert_eq!(kr_admin1(DOKDO.0, DOKDO.1).unwrap().code, "KR-47");
    }

    #[test]
    fn it_names_the_province_you_are_in() {
        let cases = [
            (37.4979, 127.0276, "서울특별시"),
            (37.2636, 127.0286, "경기도"),
            (36.3504, 127.3845, "대전광역시"),
            (35.1796, 129.0756, "부산광역시"),
            (33.2541, 126.5601, "제주특별자치도"),
            (37.4844, 130.9057, "경상북도"),
            (35.3369, 127.7306, "경상남도"),
            (38.1194, 128.4656, "강원도"),
        ];
        for (lat, lon, want) in cases {
            assert_eq!(
                kr_admin1(lat, lon).map(|r| r.name.as_str()),
                Some(want),
                "{lat},{lon}"
            );
        }
    }

    #[test]
    fn there_is_no_province_outside_korea() {
        assert!(kr_admin1(35.6762, 139.6503).is_none());
        assert!(kr_admin1(38.5, 131.5).is_none());
    }

    /// 내장 데이터가 인천의 섬들을 경기도로 적어 두었다 — 이 한계를 시험으로 적어 둔다.
    /// 도시 표가 이것을 바로잡는다(offline::ganghwa_belongs_to_incheon_in_the_city_table).
    #[test]
    fn the_polygons_misplace_incheons_islands() {
        assert_eq!(
            kr_admin1(37.7469, 126.4880).map(|r| r.name.as_str()),
            Some("경기도")
        );
    }
}
