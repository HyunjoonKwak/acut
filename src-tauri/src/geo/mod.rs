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

pub mod boundary;
pub mod offline;
pub mod resolve;

use crate::db::conn::{Db, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 격자 한 칸 — 0.01도. 이보다 잘게 나누면 같은 동네를 여러 번 묻는다.
const CELL: f64 = 0.01;
/// Nominatim 규칙 — 초당 한 건.
const GAP: Duration = Duration::from_millis(1100);
const UA: &str = concat!("photo-desk/", env!("CARGO_PKG_VERSION"), " (personal photo library; github.com/HyunjoonKwak/photo_desk)");
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
    crate::db::predicates::valid_gps_sql(crate::db::predicates::Files::Bare)
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
    /// 몇 단계까지 채워졌나 — 0(빈 값)부터 3(시군구까지).
    ///
    /// 위가 비면 아래도 세지 않는다. «나라 없이 시군구만» 같은 결과는 트리에
    /// 걸 자리가 없어 없는 것과 같다.
    pub fn depth(&self) -> u8 {
        match (&self.country, &self.admin1, &self.admin2) {
            (Some(_), Some(_), Some(_)) => 3,
            (Some(_), Some(_), None) => 2,
            (Some(_), _, _) => 1,
            _ => 0,
        }
    }
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

/// 온라인에 물어본 결과 — **값의 출처와 다른 축이다.**
///
/// 서버가 이름을 못 찾았다고 해서 이미 가진 이름이 틀린 것은 아니다. 그래서 값은
/// 그대로 두고 여기에만 적는다. 이 기록이 없으면 같은 좌표를 볼 때마다 같은
/// 서버에 되풀이해 물어 «보강»이 영영 끝나지 않는다 (2026-09-01).
const ONLINE_OK: &str = "success";
const ONLINE_NONE: &str = "none";
const ONLINE_SHALLOW: &str = "shallow";
const ONLINE_CONFLICT: &str = "conflict";

/// 서버 답을 받아들일까 — 받아들이지 않으면 그 사유가 곧 조회 결과가 된다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Accept,
    /// 기존보다 얕거나, 국가 코드 없는 부분 응답이 검증된 국가를 바꾸려 한다
    Shallow,
    /// 국가가 내장 경계와 어긋난다 — 독도에 «일본»이 오는 것 같은 답
    Conflict,
}

impl Verdict {
    fn outcome(self) -> &'static str {
        match self {
            Verdict::Accept => ONLINE_OK,
            Verdict::Shallow => ONLINE_SHALLOW,
            Verdict::Conflict => ONLINE_CONFLICT,
        }
    }
}

/// 서버 답을 받아들일지 판정한다.
///
/// 내장 경계가 나라를 아는 자리에서는 **경계가 이긴다.** 독도처럼 정책으로
/// 못 박은 좌표도 여기서 지켜진다 — 경계가 KR 이라고 답하기 때문이다.
/// 나라 이름 글월을 견주지 않는다. 서버가 어느 말로 답하느냐에 따라
/// «대한민국»과 «South Korea» 가 달라 보이기 때문이다 — ISO 두 글자로만 견준다.
fn judge(
    boundary_cc: Option<&str>,
    answer_cc: Option<&str>,
    // 시도가 경계와 맞나 — `None` 은 «판단할 수 없다»이지 «어긋난다»가 아니다
    admin1_ok: Option<bool>,
    new_depth: u8,
    old_depth: u8,
) -> Verdict {
    if let Some(known) = boundary_cc {
        match answer_cc {
            // 나라가 어긋난다 — 값을 지키고 이 서버에는 다시 묻지 않는다
            Some(got) if !got.eq_ignore_ascii_case(known) => return Verdict::Conflict,
            // 나라를 밝히지 않은 답은 검증된 나라를 바꿀 수 없다
            None => return Verdict::Shallow,
            _ => {}
        }
    }
    // 나라가 같아도 도가 틀릴 수 있다. 격자 대표 좌표는 칸 안 어딘가일 뿐이라,
    // 도 경계에 걸친 칸에서 서버가 옆 도를 답하는 일이 실제로 생긴다.
    // 우리가 아는 시도 이름과 어긋날 때만 막는다 — 모르는 이름에는 다투지 않는다.
    if admin1_ok == Some(false) {
        return Verdict::Conflict;
    }
    // 시군구까지 있던 자리를 나라만 있는 답으로 바꾸면 두 단계를 잃는다
    if new_depth < old_depth {
        return Verdict::Shallow;
    }
    Verdict::Accept
}

/// 주소 조각에서 ISO 3166-1 alpha-2 를 꺼낸다.
///
/// Nominatim 은 소문자로 준다(`"kr"`). 두 글자 알파벳이 아니면 믿지 않는다 —
/// 어떤 서버는 `"gb-eng"` 같은 값을 넣거나 칸을 아예 빼기도 한다.
pub fn country_code(addr: &serde_json::Value) -> Option<String> {
    let raw = addr.get("country_code")?.as_str()?.trim();
    let up = raw.to_ascii_uppercase();
    if up.len() == 2 && up.bytes().all(|b| b.is_ascii_alphabetic()) { Some(up) } else { None }
}

/// 물어본 결과 — 넷을 갈라야 «결과 없음»·«잠깐 실패»·«설정이 틀림»을 다르게 다룬다.
enum Answer {
    Found(Found),
    /// 그 자리에 이름이 없다 — 캐시에 못 박고 다시 묻지 않는다
    Nothing,
    /// 잠깐 실패했다 — 조금 쉬었다 다시 물으면 된다 (5xx · 429 · 연결 끊김)
    Retryable { message: String, retry_after: Option<Duration> },
    /// 다시 물어도 소용없다 — 주소나 권한이 틀렸다. 캐시하지 않고 멈춘다.
    Fatal(String),
}

/// 서버가 준 한 자리의 답 — 이름과, 그 이름이 어느 나라 것인지.
///
/// 나라 코드를 따로 나르는 이유: 이름은 서버 언어에 따라 달라지지만 ISO 두
/// 글자는 그렇지 않다. 내장 경계와 견주려면 흔들리지 않는 열쇠가 있어야 한다.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    place: Place,
    /// ISO 3166-1 alpha-2 대문자. 서버가 밝히지 않았으면 None.
    cc: Option<String>,
}

/// 재시도 사이에 쉬는 시간 — 2초, 5초, 15초. 서버가 Retry-After 를 주면 그것을 따른다.
/// 어느 값도 `GAP`(초당 한 건)보다 짧지 않다 — 재시도가 그 약속을 깨면 안 된다.
const RETRIES: &[Duration] = &[Duration::from_secs(2), Duration::from_secs(5), Duration::from_secs(15)];
/// 서버가 «한참 뒤에 오라»고 해도 이보다 오래 붙잡고 있지 않는다
const RETRY_CAP: Duration = Duration::from_secs(30);

/// Retry-After 머리글을 읽는다 — 초 단위 숫자만 받는다(날짜 형식은 무시)
fn retry_after(res: &reqwest::blocking::Response) -> Option<Duration> {
    let raw = res.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    Some(Duration::from_secs(secs).min(RETRY_CAP))
}

/// 취소를 살피며 쉰다 — 4초 백오프 중에 «그만»을 눌러도 곧바로 멈추게
fn nap(cancel: &AtomicBool, total: Duration) -> bool {
    let step = Duration::from_millis(100);
    let mut left = total;
    while left > Duration::ZERO {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let d = step.min(left);
        std::thread::sleep(d);
        left -= d;
    }
    !cancel.load(Ordering::Relaxed)
}

/// 잠깐 실패면 세 번까지 다시 묻는다. 그 밖의 답은 그대로 돌려준다.
fn ask_with_retry(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    lat: f64,
    lon: f64,
    zoom: u8,
    cancel: &AtomicBool,
) -> Answer {
    let mut last = Answer::Fatal("지명 서버에 묻지 못했습니다".into());
    for (attempt, backoff) in RETRIES.iter().enumerate() {
        match ask(client, endpoint, lat, lon, zoom) {
            Answer::Retryable { message, retry_after } => {
                // 서버가 «곧 와도 된다»고 해도 초당 한 건은 지킨다
                let wait = retry_after.unwrap_or(*backoff).max(GAP);
                log::warn!("지명 조회 재시도 {}/{}: {message} ({}초 뒤)", attempt + 1, RETRIES.len(), wait.as_secs());
                last = Answer::Retryable { message, retry_after };
                if attempt + 1 == RETRIES.len() || !nap(cancel, wait) {
                    break;
                }
            }
            other => return other,
        }
    }
    last
}

/// 좌표 하나를 물어본다.
fn ask(client: &reqwest::blocking::Client, endpoint: &str, lat: f64, lon: f64, zoom: u8) -> Answer {
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{sep}lat={lat}&lon={lon}&format=jsonv2&zoom={zoom}&accept-language=ko");
    let res = match client.get(&url).header(reqwest::header::USER_AGENT, UA).send() {
        Ok(r) => r,
        // 연결·시간 초과는 망 사정일 때가 많다 — 다시 물어볼 값어치가 있다
        // `without_url()` 로 주소를 뗀다 — 주소에 인증 토큰이 들어 있으면
        // 오류 글월을 타고 로그 파일에 남는다
        Err(e) => {
            return Answer::Retryable {
                message: format!("지명 서버에 연결하지 못했습니다: {}", e.without_url()),
                retry_after: None,
            }
        }
    };
    let status = res.status();
    if !status.is_success() {
        let after = retry_after(&res);
        // 429(너무 잦음)와 5xx(서버 사정)는 기다리면 풀린다.
        // 그 밖의 4xx 는 주소나 권한이 틀린 것이라 다시 물어도 같은 답이 온다.
        return if status.as_u16() == 429 || status.is_server_error() {
            Answer::Retryable {
                message: format!("서버가 {status} 로 답했습니다"),
                retry_after: after,
            }
        } else {
            Answer::Fatal(format!("서버가 {status} 로 답했습니다 — 설정 › 탐색의 지명 서버 주소를 확인해 주세요"))
        };
    }
    let body: serde_json::Value = match res.json() {
        Ok(v) => v,
        // 본문이 깨진 것은 대개 중간 장비가 끼어든 경우다 — 한 번 더 물어본다
        Err(e) => {
            return Answer::Retryable {
                message: format!("지명 서버 응답을 읽지 못했습니다: {}", e.without_url()),
                retry_after: None,
            }
        }
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
            Answer::Fatal(format!("지명 서버 오류: {message}"))
        };
    }
    let addr = body.get("address");
    let place = addr.map(fold).unwrap_or_default();
    let cc = addr.and_then(country_code);
    // 위치 트리의 첫 단계이자 처리 완료 표시는 국가다. 국가가 없는 부분 응답을
    // 성공 캐시로 남기면 같은 파일이 영원히 미완료로 남는다.
    if place.country.is_none() { Answer::Nothing } else { Answer::Found(Found { place, cc }) }
}

/// 이미 이름이 붙은 사진을 다시 쓸 것인가 — B3 덮어쓰기 규칙의 유일한 갈림길이다.
///
/// 규칙은 세 줄이다: 온라인 결과는 오프라인 결과를 덮는다. 오프라인 결과는
/// 이름이 없는 곳에만 쓴다. 어느 쪽도 사람이 손댄 값을 덮지 않는다(아직 없다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overwrite {
    /// 이름이 아직 없는 사진에만
    OnlyEmpty,
    /// 이 자리의 사진 전부 — 더 정밀한 결과로 바꿔 붙인다
    All,
}

impl Overwrite {
    fn filter(self) -> &'static str {
        match self {
            Overwrite::OnlyEmpty => "AND geo_country IS NULL",
            Overwrite::All => "",
        }
    }
}

/// 어느 경로로 채우나
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// 서버 없이 — 내장 자료로 곧바로 채운다
    Offline,
    /// 서버에 물어 정밀하게 — 오프라인으로 채운 자리도 다시 묻는다
    Online,
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
    /// 멈췄으면 그 사유. 비어 있으면 «끝까지 다 했다»는 뜻이다.
    pub stopped: Option<String>,
    /// 사용자가 멈춘 것인가 — 서버 탓과 달리 경고로 보일 일이 아니다
    pub cancelled: bool,
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
    overwrite: Overwrite,
    // 온라인 조회 결과도 함께 남길 때. 오프라인 경로는 None 을 준다 —
    // 오프라인이 값을 채웠다고 해서 «서버에 물어봤다»가 되지는 않는다.
    online: Option<&str>,
) -> Result<usize> {
    let name = place.name();
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,
                                distance_km,dataset_version,provider,resolved_at,
                                online_outcome,online_provider,online_checked_at,at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,strftime('%s','now'),
                    ?12,?13,CASE WHEN ?12 IS NULL THEN NULL ELSE strftime('%s','now') END,
                    strftime('%s','now'))
             ON CONFLICT(cell) DO UPDATE SET
               country=excluded.country, admin1=excluded.admin1, admin2=excluded.admin2,
               name=excluded.name, status=excluded.status, source=excluded.source,
               precision=excluded.precision, distance_km=excluded.distance_km,
               dataset_version=excluded.dataset_version, provider=excluded.provider,
               resolved_at=excluded.resolved_at,
               -- 조회 결과는 물어봤을 때만 덮는다. 오프라인이 값을 채워도
               -- 앞서 서버가 답한 이력은 지우지 않는다.
               online_outcome=COALESCE(excluded.online_outcome, places.online_outcome),
               online_provider=COALESCE(excluded.online_provider, places.online_provider),
               online_checked_at=COALESCE(excluded.online_checked_at, places.online_checked_at)",
            rusqlite::params![
                cell_key, &place.country, &place.admin1, &place.admin2, &name,
                status, source, precision, distance_km, dataset_version, provider,
                online, if online.is_some() { provider } else { None }
            ],
        )?;
        if place.country.is_none() {
            return Ok(0);
        }
        // 이 자리의 사진에 전파
        let n = tx.execute(
            &format!(
                "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                        geo_name = COALESCE(?4, ?3, ?2)
                 WHERE {cell} = ?1 AND {gps} {only}",
                cell = cell_sql("gps_lat", "gps_lon"),
                only = overwrite.filter()
            ),
            rusqlite::params![cell_key, &place.country, &place.admin1, &place.admin2],
        )?;
        Ok(n)
    })
}

/// 이 자리에 이미 몇 단계까지 붙어 있나 — 새 답이 그보다 얕으면 덮지 않는다
fn current_depth(db: &Db, cell_key: &str) -> Result<u8> {
    let place: Option<Place> = db.read(|c| {
        c.query_row(
            "SELECT country, admin1, admin2 FROM places WHERE cell = ?1 AND status = 'ok'",
            [cell_key],
            |r| Ok(Place { country: r.get(0)?, admin1: r.get(1)?, admin2: r.get(2)? }),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })
    })?;
    Ok(place.map(|p| p.depth()).unwrap_or(0))
}

/// 값은 건드리지 않고 «이 서버에 물어봤고 결과가 이랬다»만 남긴다.
///
/// 이 기록이 대상 고르기의 열쇠다. 없으면 값이 그대로라는 이유로 같은 좌표가
/// 다음 실행에서 또 뽑혀 같은 서버에 되풀이해 묻는다 — 보강이 끝나지 않는다.
/// 서버가 바뀌면 `online_provider` 가 달라 다시 물을 수 있다.
fn record_online(db: &Db, cell_key: &str, outcome: &str, provider: Option<&str>) -> Result<()> {
    db.write(|c| {
        c.execute(
            "INSERT INTO places(cell,status,source,online_outcome,online_provider,online_checked_at,at)
             VALUES(?1,?2,?3,?4,?5,strftime('%s','now'),strftime('%s','now'))
             ON CONFLICT(cell) DO UPDATE SET
               online_outcome=excluded.online_outcome,
               online_provider=excluded.online_provider,
               online_checked_at=excluded.online_checked_at",
            rusqlite::params![cell_key, UNRESOLVED, SRC_ONLINE, outcome, provider],
        )
    })?;
    Ok(())
}

/// «그 자리에 이름이 없다»를 캐시에 못 박는다 — **이미 이름이 있으면 그대로 둔다.**
///
/// 이름을 지우는 일은 이 앱 어디에도 없어야 한다. 서버가 못 찾은 것과 그 자리에
/// 이름이 없는 것은 다르다. 이미 붙은 이름이 있으면 그것을 남기고 «물어봤다»만
/// 적는다 — 그래야 그 자리가 다음 실행에서 또 뽑히지 않는다. 돌려주는 값은
/// 실제로 못 박았는지 여부다.
fn settle_empty(db: &Db, cell_key: &str, status: &str, source: &str, provider: Option<&str>) -> Result<bool> {
    db.transaction(|tx| {
        let named: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM places
                            WHERE cell = ?1 AND country IS NOT NULL AND trim(country) <> '')",
            [cell_key],
            |r| r.get(0),
        )?;
        if named {
            // 값과 출처는 그대로 — 바뀐 것은 «이 서버가 못 찾았다»는 사실뿐이다
            tx.execute(
                "UPDATE places
                    SET online_outcome = ?2, online_provider = ?3,
                        online_checked_at = strftime('%s','now')
                  WHERE cell = ?1",
                rusqlite::params![cell_key, ONLINE_NONE, provider],
            )?;
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO places(cell,status,source,precision,provider,resolved_at,
                                online_outcome,online_provider,online_checked_at,at)
             VALUES(?1,?2,?3,?4,?5,strftime('%s','now'),?6,?5,strftime('%s','now'),strftime('%s','now'))
             ON CONFLICT(cell) DO UPDATE SET
               status=excluded.status, source=excluded.source, precision=excluded.precision,
               provider=excluded.provider, resolved_at=excluded.resolved_at,
               online_outcome=excluded.online_outcome, online_provider=excluded.online_provider,
               online_checked_at=excluded.online_checked_at",
            rusqlite::params![cell_key, status, source, PREC_REMOTE, provider, ONLINE_NONE],
        )?;
        Ok(true)
    })
}

/// 이미 캐시에 있는 값을 그 자리의 사진에 붙인다 (네트워크 없이)
fn propagate(db: &Db, cell_key: &str, place: &Place, gps: &str, overwrite: Overwrite) -> Result<usize> {
    db.write(|c| {
        c.execute(
            &format!(
                "UPDATE files SET geo_country = ?2, geo_admin1 = ?3, geo_admin2 = ?4,
                        geo_name = COALESCE(?4, ?3, ?2)
                 WHERE {cell} = ?1 AND {gps} {only}",
                cell = cell_sql("gps_lat", "gps_lon"),
                only = overwrite.filter()
            ),
            rusqlite::params![cell_key, &place.country, &place.admin1, &place.admin2],
        )
    })
}

/// 처리할 자리와 그 자리의 대표 좌표.
///
/// 격자는 «같은 곳을 두 번 묻지 않기» 위한 열쇠일 뿐이고, 대표 좌표는 그 칸에
/// **실제로 있는 사진들의 중앙값**이다 — 칸 정중앙을 쓰면 경계·해안·섬에서 옆
/// 동네가 붙는다 (2026-09-01 리뷰).
///
/// 무엇을 대상으로 삼는가가 두 경로의 유일한 차이다:
/// - 오프라인: 아직 아무 판정도 없는 자리. 온라인이 이미 정한 것은 건드리지 않는다.
/// - 온라인: 이름이 없는 자리와, 오프라인이 채워 둔 자리(정밀 보강). 서버가
///   «이름 없음»으로 확정한 자리(none)는 어느 쪽도 다시 묻지 않는다.
fn targets(db: &Db, mode: Mode, gps: &str, provider: Option<&str>) -> Result<Vec<(String, f64, f64)>> {
    let cell_expr = cell_sql("gps_lat", "gps_lon");
    let want = match mode {
        // 아직 오프라인이 손대지 않았고, 이름이 없는 사진이 있는 자리.
        //
        // 온라인이 먼저 돌아 충돌·불완전 응답을 만나면 값 없는 `unresolved` 행이
        // 생긴다. 그때 «판정이 아예 없는 자리»만 고르면 그 좌표는 오프라인으로도
        // 영영 복구되지 않는다 — 온라인이 실패했다는 이유로 내장 자료마저 막히는
        // 셈이다. 그래서 **오프라인이 스스로 포기한 자리만** 건너뛴다.
        // 이미 판정된 자리의 미전파는 propagate_all 이 따로 되메운다.
        Mode::Offline => {
            "(p.status IS NULL
              OR (p.status = 'unresolved' AND COALESCE(p.source,'') <> 'offline_geonames'))
             AND t.unnamed > 0"
        }
        // 이름이 없거나, 오프라인 결과라 더 정밀해질 수 있는 자리.
        //
        // 고르는 열쇠는 값이 아니라 **«이 서버에 물어봤나»** 다. 값만 보면 서버가
        // 못 찾았거나 얕게 답한 자리가 «값이 그대로»라는 이유로 매번 다시 뽑혀
        // 같은 서버에 되풀이해 묻는다 — 보강이 끝나지 않는다.
        //
        // 그래서 `status='none'` 을 여기서 걸러내지 않는다. «이름이 없다»는 것은
        // 그 서버의 답일 뿐이고, 다른 서버는 알 수도 있다. 서버가 바뀌면
        // online_provider 가 달라 다시 물어본다. 옛 판에서 넘어온 행은
        // online_provider 가 비어 있어 새 서버에서 꼭 한 번 다시 물어본다.
        Mode::Online => {
            "(t.unnamed > 0 OR p.source = 'offline_geonames')
             AND (p.online_outcome IS NULL
                  OR (?1 IS NOT NULL
                      AND (p.online_provider IS NULL OR p.online_provider <> ?1)))"
        }
    };
    db.read(|c| {
        let mut st = c.prepare(&format!(
            "WITH pts AS (
               SELECT {cell_expr} AS cell, gps_lat AS la, gps_lon AS lo,
                      ROW_NUMBER() OVER (PARTITION BY {cell_expr} ORDER BY gps_lat, gps_lon) AS rn,
                      COUNT(*) OVER (PARTITION BY {cell_expr}) AS n,
                      SUM(geo_country IS NULL) OVER (PARTITION BY {cell_expr}) AS unnamed
                 FROM files
                WHERE {gps} AND trashed_at IS NULL
             ),
             t AS (SELECT * FROM pts WHERE rn = (n + 1) / 2)
             SELECT t.cell, t.la, t.lo
               FROM t LEFT JOIN places p ON p.cell = t.cell
              WHERE {want}
              ORDER BY t.cell",
        ))?;
        // 오프라인 조건에는 «어느 서버에 물었나»가 없다 — 자리 표시자를 쓰지 않는
        // 질의에 값을 넘기면 SQLite 가 개수 불일치로 거절한다
        fn row(r: &rusqlite::Row) -> rusqlite::Result<(String, f64, f64)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        }
        match mode {
            Mode::Offline => st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>(),
            Mode::Online => {
                st.query_map(rusqlite::params![provider], row)?.collect::<rusqlite::Result<Vec<_>>>()
            }
        }
    })
}

/// 이미 쓸 수 있는 이름이 캐시에 있는데 아직 붙지 않은 사진이 있는 자리 수.
///
/// 스캔으로 새 사진이 들어오거나 좌표가 바뀌어 지명이 지워지면 이 자리가 생긴다.
/// 서버도 내장 자료도 필요 없다 — 가진 값을 옮겨 붙이기만 하면 된다. 화면이 이
/// 수를 세지 않으면 «처리할 곳 0» 이라 단추가 꺼지고, 사용자는 서버를 설정하지
/// 않는 한 새 사진에 이름을 붙일 길이 없어진다 (2026-09-01).
fn cache_cells_left(db: &Db, gps: &str) -> Result<i64> {
    let cell_expr = cell_sql("gps_lat", "gps_lon");
    db.read(|c| {
        c.query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                   SELECT {cell_expr} AS cell
                     FROM files
                    WHERE {gps} AND trashed_at IS NULL AND geo_country IS NULL
                    GROUP BY cell
                 ) t
                 JOIN places p ON p.cell = t.cell
                WHERE p.status = 'ok' AND p.country IS NOT NULL AND trim(p.country) <> ''",
            ),
            [],
            |r| r.get(0),
        )
    })
}

/// 이 서버를 가리키는 이름 — 조회 이력의 열쇠.
///
/// 호스트만으로는 모자란다. 같은 기계에서 포트나 경로만 다르게 띄운 두 서버는
/// 서로 다른 자료를 가질 수 있는데, 호스트만 보면 «같은 서버»로 여겨 한쪽이
/// 못 찾은 자리를 다른 쪽에 물어보지 않는다.
///
/// **물음표 뒤(query)와 조각(fragment)은 뺀다.** 자체 Nominatim 을 `?key=...` 로
/// 지키는 구성이 흔한데, 그 열쇠가 DB 에 남으면 안 된다. 대소문자와 끝 빗금도
/// 고르게 만들어 같은 서버를 다르게 세지 않는다.
fn provider_of(endpoint: Option<&str>) -> Option<String> {
    let raw = endpoint?;
    validate_endpoint(raw).ok()?;
    let url = reqwest::Url::parse(raw).ok()?;
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url.host_str()?.to_ascii_lowercase();
    // 그 scheme 의 기본 포트면 Url 이 None 을 준다 — 적지 않아야 같은 것이 같아진다
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    Some(format!("{scheme}://{host}{port}{path}"))
}

/// 캐시에 이미 있는 이름을 이름 없는 사진에 **한 번에** 붙인다.
///
/// 자리마다 UPDATE 를 돌리면 그때마다 파일 5만 행을 훑는다(실측 자리당 139ms,
/// 1,120곳에 156초). 격자 열쇠가 계산식이라 인덱스를 못 쓰기 때문이다. 그래서
/// 한 번만 훑고 places 를 PK 로 붙인다.
///
/// 이 걸음은 언제 돌려도 안전하고, 판정과 전파가 중간에 끊겨 어긋난 것도 여기서
/// 되메워진다 — 그래서 오프라인 채우기는 늘 이것으로 끝난다.
fn propagate_all(db: &Db, gps: &str) -> Result<usize> {
    propagate_scoped(db, gps, None)
}

/// 한 라이브러리 안에서만 붙인다 — 스캔 뒤에 쓴다.
///
/// 감시는 폴더가 바뀔 때마다 스캔을 부르므로, 그때마다 파일 표 전체를 훑으면
/// 라이브러리가 여럿일수록 헛일이 커진다.
pub fn propagate_library(db: &Db, library_id: i64) -> Result<usize> {
    propagate_scoped(db, &valid_gps_sql(), Some(library_id))
}

fn propagate_scoped(db: &Db, gps: &str, library_id: Option<i64>) -> Result<usize> {
    let cell = cell_sql("gps_lat", "gps_lon");
    let scope = if library_id.is_some() {
        "AND folder_id IN (SELECT id FROM folders WHERE library_id = ?1)"
    } else {
        ""
    };
    db.write(|c| {
        let sql = format!(
                "UPDATE files SET
                   geo_country = (SELECT p.country FROM places p WHERE p.cell = {cell}),
                   geo_admin1  = (SELECT p.admin1  FROM places p WHERE p.cell = {cell}),
                   geo_admin2  = (SELECT p.admin2  FROM places p WHERE p.cell = {cell}),
                   geo_name    = (SELECT p.name    FROM places p WHERE p.cell = {cell})
                 WHERE {gps} AND geo_country IS NULL
                   AND EXISTS(SELECT 1 FROM places p
                               WHERE p.cell = {cell} AND p.status = 'ok'
                                 AND p.country IS NOT NULL AND trim(p.country) <> '')
                   {scope}",
            );
        match library_id {
            Some(id) => c.execute(&sql, [id]),
            None => c.execute(&sql, []),
        }
    })
}

/// 이미 아는 이름을 아직 못 받은 사진에 붙인다 — 서버도 내장 자료도 필요 없다.
///
/// 스캔이 끝날 때마다 한 번 돈다. 새 사진이 이미 처리한 자리에 들어오는 일은
/// 흔한데, 그때마다 사용자가 «채우기»를 눌러야 한다면 그 사진은 대개 이름 없이
/// 남는다. 붙인 사진 수를 돌려준다.
pub fn propagate_cached(db: &Db) -> Result<usize> {
    propagate_all(db, &valid_gps_sql())
}

/// 서버 없이 채운다 — 내장한 도시·경계 자료로 곧바로 판정한다.
///
/// 망도, 설정도, 기다림도 없다. 결과는 «근사»로 표시되고 나중에 정밀 보강이
/// 덮어쓸 수 있다. 판정하지 못한 자리(바다 위 등)는 `unresolved` 로 남겨 두어
/// 온라인 경로가 다시 시도한다.
///
/// 판정과 전파를 나눈다: 자리를 다 판정해 캐시에 적은 뒤, 사진에는 마지막에
/// 한 번만 붙인다. 중간에 멈춰도 캐시는 남고, 다음 실행의 마지막 걸음이 전파를
/// 마저 한다.
pub fn fill_offline(
    db: &Db,
    cancel: &AtomicBool,
    limit: Option<usize>,
    on_progress: impl Fn(&Progress),
) -> Result<Progress> {
    let gps = valid_gps_sql();
    let todo = targets(db, Mode::Offline, &gps, None)?;
    let todo: Vec<_> = match limit {
        Some(n) => todo.into_iter().take(n).collect(),
        None => todo,
    };
    let version = offline::dataset_version();
    // 판정할 자리와 «가진 값을 옮겨 붙이기만 하면 되는» 자리를 함께 센다 —
    // 화면이 안내한 수와 실제로 하는 일이 같아야 한다
    let cached = cache_cells_left(db, &gps)? as usize;
    let mut p = Progress { total: todo.len() + cached, ..Default::default() };
    on_progress(&p);

    // 한 트랜잭션이 너무 길면 그동안 다른 작업이 DB 를 못 쓴다
    const CHUNK: usize = 500;
    for chunk in todo.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            p.stopped = Some("멈췄습니다".into());
            p.cancelled = true;
            break;
        }
        let judged: Vec<(&str, f64, f64, Option<resolve::Resolved>)> =
            chunk.iter().map(|(k, lat, lon)| (k.as_str(), *lat, *lon, resolve::resolve(*lat, *lon))).collect();
        let empty = judged.iter().filter(|(_, _, _, r)| r.is_none()).count();

        db.transaction(|tx| {
            for (cell_key, _, _, r) in &judged {
                let (place, status, precision, distance) = match r {
                    Some(r) => (r.place.clone(), OK, r.precision, r.distance_km),
                    None => (Place::default(), UNRESOLVED, PREC_APPROX, None),
                };
                tx.execute(
                    "INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,
                                        distance_km,dataset_version,resolved_at,at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,strftime('%s','now'),strftime('%s','now'))
                     ON CONFLICT(cell) DO UPDATE SET
                       country=excluded.country, admin1=excluded.admin1, admin2=excluded.admin2,
                       name=excluded.name, status=excluded.status, source=excluded.source,
                       precision=excluded.precision, distance_km=excluded.distance_km,
                       dataset_version=excluded.dataset_version, resolved_at=excluded.resolved_at",
                    rusqlite::params![
                        cell_key, &place.country, &place.admin1, &place.admin2, place.name(),
                        status, SRC_OFFLINE, precision, distance, version
                    ],
                )?;
            }
            Ok(())
        })?;

        p.done += judged.len();
        p.empty += empty;
        on_progress(&p);
    }

    // 사진에 붙이는 것은 마지막에 한 번 — 파일 표를 한 번만 훑는다.
    // 이 걸음이 캐시만 있으면 되는 자리까지 함께 메운다.
    p.files = propagate_all(db, &gps)?;
    if p.stopped.is_none() {
        p.done = p.total;
    }
    on_progress(&p);
    Ok(p)
}

/// 서버에 물어 정밀하게 채운다.
///
/// 이름이 없는 자리와, 오프라인이 채워 둔 자리(정밀 보강)를 대상으로 한다.
/// 캐시에 이미 **온라인** 결과가 있으면 묻지 않고 사진에만 붙인다 — 오프라인
/// 결과는 캐시로 치지 않는다(그것을 더 낫게 만드는 것이 이 경로의 일이다).
///
/// 서버가 잠깐 흔들리면 세 번까지 다시 묻고(1·2·4초, Retry-After 존중), 주소나
/// 권한이 틀린 답이면 그 자리에서 멈춘다 — 채운 것은 남고 다음에 이어서 한다.
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

    let provider = provider_of(endpoint.as_deref());
    let todo = targets(db, Mode::Online, &gps, provider.as_deref())?;

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
            p.stopped = Some("멈췄습니다".into());
            p.cancelled = true;
            break;
        }
        // 캐시부터 — 성공한 것만 쓴다
        // 캐시로 치는 것은 **온라인** 성공뿐이다. 오프라인 결과를 캐시로 삼으면
        // 정밀 보강이 영영 일어나지 않는다.
        let cached: Option<Place> = db.read(|c| {
            c.query_row(
                "SELECT country, admin1, admin2 FROM places
                  WHERE cell = ?1 AND status = ?2 AND source = ?3
                    AND country IS NOT NULL AND trim(country) <> ''",
                rusqlite::params![&cell_key, OK, SRC_ONLINE],
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
                // 고르는 열쇠와 적는 열쇠는 반드시 같은 함수에서 나와야 한다
                let host = provider_of(Some(endpoint.as_str()));
                match ask_with_retry(&client, endpoint.as_str(), lat, lon, zoom, cancel) {
                    Answer::Found(found) => {
                        // 내장 경계가 나라를 아는 자리에서는 경계가 이긴다 —
                        // 독도에 «일본»이 오는 답으로 정책이 뒤집히지 않게.
                        let known = boundary::country(lat, lon);
                        let admin1_ok = found
                            .place
                            .admin1
                            .as_deref()
                            .and_then(|a| boundary::admin1_matches(lat, lon, a));
                        let verdict = judge(
                            known.as_deref(),
                            found.cc.as_deref(),
                            admin1_ok,
                            found.place.depth(),
                            current_depth(db, &cell_key)?,
                        );
                        if verdict != Verdict::Accept {
                            log::info!(
                                "서버 답을 받아들이지 않습니다({}): {cell_key} — 경계 {known:?} · 답 {:?}",
                                verdict.outcome(), found.cc
                            );
                            // 값은 그대로 두고 «물어봤다»만 남긴다 — 같은 서버에 되풀이해 묻지 않게
                            record_online(db, &cell_key, verdict.outcome(), host.as_deref())?;
                            p.done += 1;
                            on_progress(&p);
                            continue;
                        }
                        // 캐시 갱신과 파일 전파를 한 트랜잭션에 둔다 — 중간에 앱이 꺼져도
                        // places 와 files 가 어긋나지 않는다 (2026-09-01 리뷰)
                        write_place(
                            db, &cell_key, &found.place, OK, SRC_ONLINE, PREC_REMOTE, None, None,
                            host.as_deref(), &gps, Overwrite::All, Some(ONLINE_OK),
                        )?;
                        found.place
                    }
                    Answer::Nothing => {
                        // 서버가 «없다»고 확정한 자리 — 못 박아 두고 다시 묻지 않는다.
                        //
                        // 단, **이미 이름이 붙은 자리는 지우지 않는다.** 새 서버가 못
                        // 찾았다는 것이 오프라인이 찾아 둔 이름이 틀렸다는 뜻은 아니다
                        // (2026-09-01 외부 검토).
                        if !settle_empty(db, &cell_key, NONE, SRC_ONLINE, host.as_deref())? {
                            log::info!("지명 없음이라 하지만 이미 붙은 이름이 있어 그대로 둡니다: {cell_key}");
                        }
                        p.empty += 1;
                        p.done += 1;
                        on_progress(&p);
                        continue;
                    }
                    Answer::Retryable { message, .. } => {
                        // 세 번을 다 쓰고도 안 됐다 — 채운 것은 남기고 멈춘다
                        log::warn!("지명 조회 중단 {cell_key}: {message}");
                        p.stopped = Some(format!("{message} — 잠시 뒤에 다시 해 주세요"));
                        break;
                    }
                    Answer::Fatal(e) => {
                        log::warn!("지명 조회 중단 {cell_key}: {e}");
                        p.stopped = Some(e);
                        break;
                    }
                }
            }
        };

        // 캐시 적중이면 파일 전파만 하면 된다
        if !place.is_empty() {
            p.files += propagate(db, &cell_key, &place, &gps, Overwrite::All)?;
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
    /// 그중 오프라인으로 새로 판정할 자리 (스냅샷만 있으면 된다)
    pub offline_cells_left: i64,
    /// 이미 캐시에 이름이 있는데 아직 안 붙은 자리 — 옮겨 붙이기만 하면 된다
    pub cache_cells_left: i64,
    /// 그중 서버에 물어야만 하는 자리 (오프라인이 이미 포기한 곳)
    pub network_cells_left: i64,
    /// 서버에 물을 수 있는 자리 전부 — 못 채운 곳과 오프라인으로 채워 둔 곳(정밀 보강)
    pub online_cells_left: i64,
    /// 새 조회에 쓸 수 있는 비공개/허가된 서버가 설정됐나
    pub endpoint_ready: bool,
}

pub fn stats(db: &Db) -> Result<Stats> {
    let gps = valid_gps_sql();
    let endpoint = endpoint_setting(db)?;
    let endpoint_ready = endpoint.as_deref().is_some_and(|s| validate_endpoint(s).is_ok());
    // 온라인으로 «물어볼 곳»은 어느 서버에 물을지에 달렸다 — 이미 이 서버가
    // 답한 자리는 세지 않아야 화면의 수가 0까지 줄어든다
    let provider = provider_of(endpoint.as_deref());
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
                          p.status, p.source, p.precision, p.country,
                          p.online_outcome, p.online_provider
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
                   -- 오프라인이 아직 손대지 않은 자리 — targets(Offline) 과 같은 조건
                   COUNT(CASE WHEN files > named
                               AND (status IS NULL
                                    OR (status = '{unresolved}'
                                        AND COALESCE(source,'') <> '{offline}')) THEN 1 END),
                   -- 서버에만 물을 수 있는 자리: 오프라인이 이미 포기한 곳
                   COUNT(CASE WHEN status = '{unresolved}' AND files > named THEN 1 END),
                   -- 서버에 물을 수 있는 자리 — targets(Online) 과 같은 조건이어야
                   -- 화면의 수와 실제로 도는 수가 어긋나지 않는다
                   COUNT(CASE WHEN (files > named OR source = '{offline}')
                               AND (online_outcome IS NULL
                                    OR (?1 IS NOT NULL
                                        AND (online_provider IS NULL
                                             OR online_provider <> ?1))) THEN 1 END),
                   -- 가진 값을 옮겨 붙이기만 하면 되는 자리. 파일 표를 한 번 더
                   -- 훑지 않으려고 같은 질의 안에서 센다 (실측 115→284ms 였다)
                   COUNT(CASE WHEN files > named AND status = '{ok}'
                               AND country IS NOT NULL AND trim(country) <> '' THEN 1 END)
                 FROM joined",
                cell = cell_sql("gps_lat", "gps_lon"),
                none = NONE, unresolved = UNRESOLVED, offline = SRC_OFFLINE, online = SRC_ONLINE,
                ok = OK
            ),
            rusqlite::params![provider],
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
                    online_cells_left: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    cache_cells_left: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
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
                let deadline = std::time::Instant::now() + Duration::from_secs(8);
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
            .connect_timeout(Duration::from_millis(500))
            .redirect(reqwest::redirect::Policy::none())
            // CI 나 macOS 의 프록시 환경 변수가 127.0.0.1 요청을 가로채지 않게
            .no_proxy()
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

    /// 서버 주소에 열쇠가 들어 있으면 오류 글월을 타고 로그에 남는다 — 떼어 낸다.
    ///
    /// 자체 Nominatim 을 `?key=...` 로 지키는 구성이 흔하다. 연결이 실패했을 뿐인데
    /// 그 열쇠가 로그 파일에 남으면 안 된다.
    #[test]
    fn a_failure_never_leaks_the_key_in_the_server_address() {
        // 아무도 듣지 않는 자리 — 반드시 연결에 실패한다
        let secret = "s3cr3t-token-do-not-log";
        let dead = format!("http://127.0.0.1:1/reverse?key={secret}");
        let answer = ask(&test_client(), &dead, 37.5, 127.0, 12);
        match answer {
            Answer::Retryable { message, .. } => {
                assert!(!message.contains(secret), "열쇠가 오류 글월에 남았습니다: {message}");
                assert!(!message.contains("127.0.0.1"), "주소가 오류 글월에 남았습니다: {message}");
                assert!(message.contains("연결하지 못했습니다"), "무슨 일인지는 말해야 한다: {message}");
            }
            other => panic!("다시 물어볼 답이어야 한다: {}", matches!(other, Answer::Fatal(_))),
        }
    }

    /// 잠깐 흔들린 서버에는 다시 물어본다 — 한 번 실패했다고 20분 작업을 버리지 않는다
    #[test]
    fn a_shaky_server_gets_another_chance() {
        let mut server = TestServer::start(vec![
            // 첫 답은 «곧 다시 와도 된다» — 그래도 초당 한 건은 지킨다
            ("503 Service Unavailable", r#"{}"#, Some("Retry-After: 0")),
            ("200 OK", r#"{"address":{"country":"대한민국","state":"경기도","city":"수원시"}}"#, None),
        ]);
        let began = std::time::Instant::now();
        let answer = ask_with_retry(&test_client(), &server.url, 37.5, 127.0, 12, &AtomicBool::new(false));
        let waited = began.elapsed();
        assert_eq!(server.served(), 2, "두 번 물어봐야 한다");
        match answer {
            Answer::Found(f) => assert_eq!(f.place.admin2.as_deref(), Some("수원시")),
            _ => panic!("두 번째 답을 받아야 한다"),
        }
        assert!(waited >= GAP, "재시도가 초당 한 건 약속을 깨면 안 된다 — {waited:?}");
    }

    /// 세 번을 다 쓰고도 안 되면 멈춘다 — 끝없이 두드리지 않는다
    #[test]
    fn a_dead_server_is_not_hammered_forever() {
        let cancel = AtomicBool::new(false);
        let mut server = TestServer::start(vec![("503 Service Unavailable", r#"{}"#, Some("Retry-After: 0"))]);
        let answer = ask_with_retry(&test_client(), &server.url, 37.5, 127.0, 12, &cancel);
        assert_eq!(server.served(), RETRIES.len(), "정해진 횟수만 물어본다");
        assert!(matches!(answer, Answer::Retryable { .. }));
    }

    /// 백오프 중에 «그만»을 누르면 곧바로 멈춘다 — 15초를 다 기다리지 않는다
    #[test]
    fn stopping_during_a_backoff_takes_effect_at_once() {
        let cancel = AtomicBool::new(true);
        let began = std::time::Instant::now();
        assert!(!nap(&cancel, Duration::from_secs(15)));
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    /// 서버가 못 찾았다고 해서 이미 붙은 이름을 지우면 안 된다
    #[test]
    fn a_server_that_finds_nothing_never_erases_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2911,127.0089);",
            )
        })
        .unwrap();
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();

        let settled = settle_empty(&db, "37.29,127.00", NONE, SRC_ONLINE, Some("my.server")).unwrap();
        assert!(!settled, "이미 이름이 있으면 못 박지 않는다");

        let (country, status): (Option<String>, String) = db
            .read(|c| {
                c.query_row("SELECT country, status FROM places WHERE cell='37.29,127.00'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(country.as_deref(), Some("대한민국"), "이름이 지워졌습니다");
        assert_eq!(status, OK);

        // 이름이 없는 자리는 정상적으로 못 박는다
        assert!(settle_empty(&db, "10.00,10.00", NONE, SRC_ONLINE, None).unwrap());
    }

    /// 서버 답이 기존보다 얕으면 그대로 둔다 — 시군구까지 있는 자리가 나라만 남으면 후퇴다
    #[test]
    fn a_shallower_answer_never_replaces_a_deeper_one() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,at)
                   VALUES('1,1','대한민국','경기도','수원시','수원시','ok','offline_geonames','approximate',0);",
            )
        })
        .unwrap();
        assert_eq!(current_depth(&db, "1,1").unwrap(), 3);
        assert_eq!(current_depth(&db, "9,9").unwrap(), 0, "없는 자리는 0이라 무엇이든 들어간다");

        let only_country = Place { country: Some("대한민국".into()), ..Default::default() };
        assert!(only_country.depth() < current_depth(&db, "1,1").unwrap());
        // 위가 비면 아래도 세지 않는다
        assert_eq!(Place { admin2: Some("수원시".into()), ..Default::default() }.depth(), 0);
    }

    /// 사용자가 멈춘 것도 결과에 적는다 — 안 그러면 화면이 «다 했습니다»라고 한다
    #[test]
    fn stopping_is_reported_as_a_stop_not_a_success() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2911,127.0089);",
            )
        })
        .unwrap();
        let p = fill_offline(&db, &AtomicBool::new(true), None, |_| {}).unwrap();
        assert_eq!(p.total, 1, "할 일은 있었다");
        assert_eq!(p.done, 0);
        assert_eq!(p.stopped.as_deref(), Some("멈췄습니다"));
        assert!(p.cancelled, "사용자가 멈춘 것과 서버가 막은 것은 다르게 보여야 한다");
    }

    /// 지도가 세는 사진과 지명이 처리하는 사진은 **같은 사진**이어야 한다.
    ///
    /// 좌표 조건이 두 곳에 따로 적혀 있던 시절, 한쪽만 고치면 지도에는 보이는데
    /// 지명은 영영 «처리할 수 없는» 사진이 생겼다. 경계값을 한 상 차려 두고
    /// 두 숫자가 같은지 본다.
    #[test]
    fn the_map_and_the_place_names_count_the_same_photos() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        let coords: &[(Option<f64>, Option<f64>)] = &[
            (Some(37.5), Some(127.0)),
            (Some(90.0), Some(180.0)),
            (Some(-90.0), Some(-180.0)),
            (Some(0.0), Some(127.0)),
            (Some(37.5), Some(0.0)),
            (Some(0.0), Some(0.0)),
            (None, Some(127.0)),
            (Some(37.5), None),
            (Some(90.1), Some(127.0)),
            (Some(37.5), Some(180.1)),
            (None, None),
        ];
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);",
            )
        })
        .unwrap();
        for (i, (lat, lon)) in coords.iter().enumerate() {
            let id = i as i64 + 1;
            db.write(|c| {
                c.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                     VALUES(?1,1,?2,1,0,1,0,0,?3,?4)",
                    rusqlite::params![id, format!("f{id}.jpg"), lat, lon],
                )
            })
            .unwrap();
        }
        // 휴지통에 든 사진은 어느 쪽도 세지 않는다
        db.write(|c| {
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon,trashed_at)
                 VALUES(99,1,'gone.jpg',1,0,1,0,0,37.5,127.0,1)",
                [],
            )
        })
        .unwrap();

        let by_map = crate::db::query::map_overview(&db, &crate::db::query::Filter::default()).unwrap();
        let by_geo = stats(&db).unwrap();
        assert_eq!(by_geo.with_gps, by_map.total, "지도와 지명이 다른 사진을 셉니다");
        assert_eq!(by_geo.with_gps, 5, "경계값을 포함해 다섯 장이 유효하다");
        // 셀 수 있는 것은 모두 처리할 수 있어야 한다 — 세기만 하고 못 붙이는 사진이 없게
        assert_eq!(by_geo.pending_files, by_geo.with_gps);
        let cells = targets(&db, Mode::Offline, &valid_gps_sql(), None).unwrap().len() as i64;
        assert_eq!(cells, by_geo.offline_cells_left, "처리할 자리 수도 같아야 한다");
    }

    /// 사진 몇 장과 좌표만 있는 최소한의 DB
    fn db_with(coords: &[(i64, f64, f64)]) -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO libraries(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO folders(id,volume_uuid,library_id,rel_path,name,area)
                   VALUES(1,'V',1,'a','a',1);",
            )
        })
        .unwrap();
        for (id, lat, lon) in coords {
            db.write(|c| {
                c.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                     VALUES(?1,1,?2,1,0,1,0,0,?3,?4)",
                    rusqlite::params![id, format!("f{id}.jpg"), lat, lon],
                )
            })
            .unwrap();
        }
        (dir, db)
    }

    fn geo_of(db: &Db, id: i64) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        db.read(|c| {
            c.query_row(
                "SELECT geo_country, geo_admin1, geo_admin2, geo_name FROM files WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
        })
        .unwrap()
    }

    fn set_endpoint(db: &Db, url: &str) {
        crate::db::settings::set(db, "geo.endpoint", url).unwrap();
    }

    // ── [1] 이미 아는 이름이 새 사진에 붙어야 한다 ──────────────────────────

    /// 처리한 자리에 새 사진이 들어오면 서버 없이 곧바로 이름이 붙어야 한다.
    ///
    /// 예전에는 오프라인 대상이 «판정이 아예 없는 자리»뿐이라, 캐시가 있는 자리는
    /// 화면의 «처리할 곳»이 0 이 되어 단추가 꺼졌다. 서버를 설정하지 않은 사람은
    /// 새 사진에 이름을 붙일 길이 아예 없었다 (2026-09-01).
    #[test]
    fn a_new_photo_in_a_known_place_gets_its_name_without_a_server() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"));

        // 같은 자리에 새 사진이 들어온다 (스캔이 하는 일)
        db.write(|c| {
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                 VALUES(2,1,'new.jpg',1,0,1,0,0,37.2915,127.0092)",
                [],
            )
        })
        .unwrap();

        // 화면이 «할 일 있음»으로 보여야 단추를 누를 수 있다
        let st = stats(&db).unwrap();
        assert_eq!(st.pending_files, 1, "새 사진이 처리 대기로 잡혀야 한다");
        assert_eq!(st.offline_cells_left, 0, "새로 판정할 자리는 없다");
        assert_eq!(st.cache_cells_left, 1, "가진 값을 옮기기만 하면 되는 자리가 하나");
        assert!(!st.endpoint_ready, "서버는 설정하지 않았다");

        // 화면이 부르는 바로 그 경로로 처리된다
        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(p.asked, 0, "서버에 묻지 않는다");
        assert_eq!(p.total, 1, "화면이 안내한 수와 실제로 한 일이 같아야 한다");
        assert_eq!(p.files, 1);
        assert_eq!(
            geo_of(&db, 2),
            (Some("대한민국".into()), Some("경기도".into()), Some("수원시".into()), Some("수원시".into()))
        );
        assert_eq!(stats(&db).unwrap().cache_cells_left, 0);
    }

    /// 온라인으로 받아 둔 이름도 서버 없이 새 사진에 적용된다
    #[test]
    fn an_online_name_also_reaches_new_photos_without_a_server() {
        let (_d, db) = db_with(&[(1, 37.5665, 126.9780)]);
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,at)
                   VALUES('37.56,126.97','대한민국','서울특별시','중구','중구','ok','nominatim','remote',0);",
            )
        })
        .unwrap();
        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(p.asked, 0);
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("중구"));

        // 온라인 값이 오프라인 값으로 바뀌지 않았다
        let (source, precision): (String, String) = db
            .read(|c| {
                c.query_row("SELECT source, precision FROM places WHERE cell='37.56,126.97'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!((source.as_str(), precision.as_str()), (SRC_ONLINE, PREC_REMOTE));
    }

    /// 화면이 세는 수와 실행이 세는 수는 **같은 질의가 아니다** — 어긋나지 않는지 본다
    #[test]
    fn the_screen_and_the_run_count_the_same_cells() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089), (2, 37.5665, 126.9780), (3, 33.4996, 126.5312)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        // 세 자리 모두 이름이 있는 상태에서 두 자리에 새 사진을 넣는다
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(4,1,'n1.jpg',1,0,1,0,0,37.2915,127.0092),
                         (5,1,'n2.jpg',1,0,1,0,0,37.5668,126.9783);",
            )
        })
        .unwrap();
        let st = stats(&db).unwrap();
        assert_eq!(st.cache_cells_left, 2);
        assert_eq!(st.cache_cells_left, cache_cells_left(&db, &valid_gps_sql()).unwrap());
        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(p.total, 2, "화면이 안내한 수만큼 처리해야 한다");
        assert_eq!(p.files, 2);
        assert_eq!(stats(&db).unwrap().cache_cells_left, 0);
    }

    /// 라이브러리 범위 전파는 그 라이브러리만 건드린다 —
    /// 감시는 폴더가 바뀔 때마다 스캔을 부르므로 헛일을 좁혀야 한다
    #[test]
    fn a_library_scoped_pass_leaves_other_libraries_alone() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('W','t2','library');
                 INSERT INTO libraries(id,volume_uuid,rel_path,name,area)
                   VALUES(2,'W','b','b',1);
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area,library_id)
                   VALUES(2,'W','b','b',1,2);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(2,2,'other.jpg',1,0,1,0,0,37.2915,127.0092);",
            )
        })
        .unwrap();
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        // 둘 다 같은 자리라 둘 다 붙었다
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"));
        assert_eq!(geo_of(&db, 2).2.as_deref(), Some("수원시"));

        // 지우고 한쪽만 다시 붙인다
        db.write(|c| c.execute("UPDATE files SET geo_country=NULL, geo_admin1=NULL, geo_admin2=NULL, geo_name=NULL", []))
            .unwrap();
        assert_eq!(propagate_library(&db, 1).unwrap(), 1, "제 라이브러리만 붙인다");
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"));
        assert_eq!(geo_of(&db, 2).2, None, "다른 라이브러리는 그대로다");
    }

    /// 스캔이 끝나면 저절로 붙는다 — 사용자가 단추를 누르지 않아도
    #[test]
    fn a_scan_applies_what_we_already_know() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        db.write(|c| {
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                 VALUES(2,1,'new.jpg',1,0,1,0,0,37.2915,127.0092)",
                [],
            )
        })
        .unwrap();
        assert_eq!(propagate_cached(&db).unwrap(), 1);
        assert_eq!(geo_of(&db, 2).2.as_deref(), Some("수원시"));
        assert_eq!(propagate_cached(&db).unwrap(), 0, "두 번째는 할 일이 없다");
    }

    // ── [2] 같은 서버에 같은 좌표를 되풀이해 묻지 않는다 ────────────────────

    /// 서버가 «이름 없음»이라 해도 기존 이름은 남고, 두 번째 실행은 묻지 않는다
    #[test]
    fn a_no_result_is_remembered_so_we_never_ask_that_server_again() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut server = TestServer::start(vec![("200 OK", r#"{"error":"Unable to geocode"}"#, None)]);
        set_endpoint(&db, &server.url);

        let first = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(first.asked, 1, "한 번은 물어본다");
        assert_eq!(server.served(), 1);
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"), "기존 이름은 그대로다");
        let (status, source, outcome): (String, String, String) = db
            .read(|c| {
                c.query_row(
                    "SELECT status, source, online_outcome FROM places WHERE cell='37.29,127.00'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(status, OK, "값이 살아 있으니 여전히 ok");
        assert_eq!(source, SRC_OFFLINE, "온라인이 못 찾았다고 출처를 거짓으로 바꾸지 않는다");
        assert_eq!(outcome, ONLINE_NONE);

        // 주소는 그대로 둔다 — 서버는 이미 멈췄으므로, 만약 묻는다면 연결이
        // 실패해 asked 가 올라간다. 요청이 아예 없어야 통과한다.
        assert_eq!(stats(&db).unwrap().online_cells_left, 0, "물어볼 곳이 0 이어야 한다");
        let second = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!((second.total, second.asked), (0, 0), "같은 서버에 다시 묻지 않는다");
        assert_eq!(second.stopped, None, "요청이 없으니 멈출 일도 없다");
    }

    /// 더 얕은 답도 마찬가지 — 값은 지키고, 두 번째 실행은 묻지 않는다
    #[test]
    fn a_shallow_answer_is_remembered_so_we_never_ask_that_server_again() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"대한민국","country_code":"kr"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);

        let first = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(first.asked, 1);
        assert_eq!(server.served(), 1);
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"), "시군구를 잃으면 안 된다");
        let outcome: String = db
            .read(|c| c.query_row("SELECT online_outcome FROM places WHERE cell='37.29,127.00'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(outcome, ONLINE_SHALLOW);

        // 주소를 그대로 둔 채 다시 돌린다 — 물으려 하면 죽은 서버에 걸려 asked 가 오른다
        assert_eq!(stats(&db).unwrap().online_cells_left, 0);
        let second = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!((second.total, second.asked), (0, 0));
        assert_eq!(second.stopped, None);
    }

    /// 서버를 바꾸면 다시 물어볼 수 있다 — 다른 서버는 다른 답을 알 수 있다
    #[test]
    fn changing_the_server_makes_a_settled_cell_worth_asking_again() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut first = TestServer::start(vec![("200 OK", r#"{"error":"Unable to geocode"}"#, None)]);
        set_endpoint(&db, &first.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        first.served();
        assert_eq!(stats(&db).unwrap().online_cells_left, 0);

        crate::db::settings::set(&db, "geo.endpoint", "http://other.example/reverse").unwrap();
        assert_eq!(stats(&db).unwrap().online_cells_left, 1, "서버가 바뀌면 다시 물어볼 수 있어야 한다");
        assert_eq!(
            targets(&db, Mode::Online, &valid_gps_sql(), Some("http://other.example/reverse")).unwrap().len(),
            1
        );
    }

    /// 앱을 껐다 켜도 «물어봤다»는 사실이 남아야 한다
    #[test]
    fn what_the_server_answered_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(&path).unwrap();
            db.write(|c| {
                c.execute_batch(
                    "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                     INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                     INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                       VALUES(1,1,'a.jpg',1,0,1,0,0,37.2911,127.0089);",
                )
            })
            .unwrap();
            fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
            let mut server = TestServer::start(vec![("200 OK", r#"{"error":"Unable to geocode"}"#, None)]);
            crate::db::settings::set(&db, "geo.endpoint", &server.url).unwrap();
            fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
            server.served();
        }
        // 다시 연다 — 마이그레이션이 한 번 더 돌아도 기록이 지워지면 안 된다
        let db = Db::open(&path).unwrap();
        assert_eq!(stats(&db).unwrap().online_cells_left, 0);
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"));
    }

    // ── [3] 경계로 검증한 나라가 온라인 응답에 밀리지 않는다 ──────────────────

    /// **독도는 한국 땅이다** — 서버가 더 자세한 일본 주소를 줘도 바뀌지 않는다
    #[test]
    fn no_server_can_move_dokdo_to_another_country() {
        let (_d, db) = db_with(&[(1, 37.2411, 131.8694)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(geo_of(&db, 1).1.as_deref(), Some("경상북도"));

        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"日本","country_code":"jp","state":"Shimane","city":"Okinoshima"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);

        assert_eq!(
            geo_of(&db, 1),
            (Some("대한민국".into()), Some("경상북도".into()), None, Some("경상북도".into())),
            "정책으로 못 박은 좌표가 서버 답에 뒤집혔습니다"
        );
        let outcome: String = db
            .read(|c| c.query_row("SELECT online_outcome FROM places WHERE cell='37.24,131.86'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(outcome, ONLINE_CONFLICT);
        assert_eq!(stats(&db).unwrap().online_cells_left, 0, "같은 서버에 다시 묻지 않는다");
    }

    /// 한국 좌표에 «일본»이라는 답이 오면 기존 값을 지킨다
    #[test]
    fn a_country_that_disagrees_with_the_boundary_never_wins() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"日本","country_code":"JP","state":"Tokyo","city":"Chiyoda"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);
        assert_eq!(geo_of(&db, 1).0.as_deref(), Some("대한민국"));
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("수원시"));
    }

    /// 나라가 맞으면 더 좁은 단위로 제대로 갱신된다 — 이것이 보강의 본래 일이다
    #[test]
    fn a_matching_country_refines_the_name() {
        let (_d, db) = db_with(&[(1, 37.5665, 126.9780)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(geo_of(&db, 1).1.as_deref(), Some("서울특별시"));

        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"대한민국","country_code":"kr","city":"서울특별시","borough":"중구"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);

        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("중구"), "더 좁은 단위로 갱신돼야 한다");
        let (source, outcome): (String, String) = db
            .read(|c| {
                c.query_row(
                    "SELECT source, online_outcome FROM places WHERE cell='37.56,126.97'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!((source.as_str(), outcome.as_str()), (SRC_ONLINE, ONLINE_OK));
    }

    /// 경계가 나라를 모르는 자리(바다)에서는 서버 답을 그대로 쓴다
    #[test]
    fn at_sea_the_server_is_the_only_authority() {
        let (_d, db) = db_with(&[(1, 38.5, 131.5)]);
        assert_eq!(boundary::country(38.5, 131.5), None, "이 좌표는 경계가 모르는 곳이어야 한다");
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();

        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"일본","country_code":"jp","state":"Shimane"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);
        assert_eq!(geo_of(&db, 1).0.as_deref(), Some("일본"));
    }

    /// 나라를 밝히지 않은 답은 경계로 검증된 나라를 바꿀 수 없다
    #[test]
    fn an_answer_without_a_country_code_cannot_replace_a_verified_country() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"어딘가","state":"어느도","city":"어느시","county":"어느군"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);
        assert_eq!(geo_of(&db, 1).0.as_deref(), Some("대한민국"));
        assert_eq!(stats(&db).unwrap().online_cells_left, 0, "다시 묻지 않는다");
    }

    /// 국가 코드는 ISO 두 글자만 믿는다 — 이름 글월은 번역 때문에 견줄 수 없다
    #[test]
    fn only_a_two_letter_code_counts_as_a_country() {
        let cc = |v: &str| country_code(&serde_json::from_str::<serde_json::Value>(v).unwrap());
        assert_eq!(cc(r#"{"country_code":"kr"}"#).as_deref(), Some("KR"));
        assert_eq!(cc(r#"{"country_code":" Jp "}"#).as_deref(), Some("JP"));
        assert_eq!(cc(r#"{"country_code":"gb-eng"}"#), None);
        assert_eq!(cc(r#"{"country_code":""}"#), None);
        assert_eq!(cc(r#"{"country_code":82}"#), None);
        assert_eq!(cc(r#"{"country":"대한민국"}"#), None);
    }

    /// 서버를 가리키는 이름은 포트·경로까지 보고, 열쇠는 담지 않는다
    #[test]
    fn a_server_is_identified_by_more_than_its_host() {
        let of = |u: &str| provider_of(Some(u));
        // 포트가 다르면 다른 서버다 — 한 기계에 두 서버를 띄우는 일은 흔하다
        assert_ne!(of("http://127.0.0.1:8080/reverse"), of("http://127.0.0.1:9090/reverse"));
        // 경로가 다르면 다른 서버다
        assert_ne!(of("http://a.example/one"), of("http://a.example/two"));
        // 대소문자와 끝 빗금은 같은 것으로 본다
        assert_eq!(of("http://A.Example/reverse/"), of("http://a.example/reverse"));
        // 기본 포트를 적었든 안 적었든 같다
        assert_eq!(of("http://a.example:80/reverse"), of("http://a.example/reverse"));
        assert_eq!(of("https://a.example:443/reverse"), of("https://a.example/reverse"));
        // scheme 이 다르면 다른 서버다
        assert_ne!(of("http://a.example/reverse"), of("https://a.example/reverse"));
        // **열쇠는 담지 않는다**
        let with_key = of("http://a.example/reverse?key=s3cr3t").unwrap();
        assert!(!with_key.contains("s3cr3t"), "{with_key}");
        assert_eq!(Some(with_key), of("http://a.example/reverse"));
        // 쓸 수 없는 주소는 이름도 없다
        assert_eq!(of("https://nominatim.openstreetmap.org/reverse"), None);
        assert_eq!(of("그냥 글자"), None);
        assert_eq!(provider_of(None), None);
    }

    /// 판정 규칙만 따로 — 경계가 이기고, 얕은 답은 물러난다
    #[test]
    fn the_boundary_decides_before_the_depth_does() {
        let j = |b, a, ok, nd, od| judge(b, a, ok, nd, od);
        assert_eq!(j(Some("KR"), Some("JP"), None, 3, 1), Verdict::Conflict, "더 깊어도 나라가 다르면 진다");
        assert_eq!(j(Some("KR"), Some("kr"), None, 3, 2), Verdict::Accept, "대소문자는 상관없다");
        assert_eq!(j(Some("KR"), None, None, 3, 1), Verdict::Shallow, "나라를 안 밝히면 못 바꾼다");
        assert_eq!(j(None, None, None, 3, 1), Verdict::Accept, "경계가 모르면 서버를 믿는다");
        assert_eq!(j(None, Some("JP"), None, 1, 3), Verdict::Shallow, "얕아지면 물러난다");
        assert_eq!(j(Some("KR"), Some("KR"), None, 2, 2), Verdict::Accept, "같은 깊이는 받아들인다");
        // 나라가 같아도 도가 어긋나면 막는다
        assert_eq!(j(Some("KR"), Some("KR"), Some(false), 3, 1), Verdict::Conflict, "도가 틀리면 진다");
        assert_eq!(j(Some("KR"), Some("KR"), Some(true), 3, 2), Verdict::Accept, "도가 맞으면 받아들인다");
        // 모르는 시도 이름에는 다투지 않는다 — 그러면 정상 응답까지 막힌다
        assert_eq!(j(Some("KR"), Some("KR"), None, 3, 2), Verdict::Accept);
    }

    /// **나라가 같아도 도가 다르면 기존 경계 판정을 지킨다.**
    ///
    /// 격자 대표 좌표는 칸 안 어딘가일 뿐이라, 도 경계에 걸친 칸에서 서버가 옆
    /// 도를 답하는 일이 실제로 생긴다 (2026-09-01).
    #[test]
    fn a_wrong_province_in_the_right_country_never_wins() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(geo_of(&db, 1).1.as_deref(), Some("경기도"));

        // 나라는 맞지만 도가 틀린 답 — 시군구까지 있어 «더 깊다»
        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"대한민국","country_code":"kr","state":"경상북도","city":"경주시"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);

        assert_eq!(
            geo_of(&db, 1),
            (
                Some("대한민국".into()),
                Some("경기도".into()),
                Some("수원시".into()),
                Some("수원시".into())
            ),
            "경계가 정한 도가 서버 답에 밀렸습니다"
        );
        let outcome: String = db
            .read(|c| c.query_row("SELECT online_outcome FROM places WHERE cell='37.29,127.00'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(outcome, ONLINE_CONFLICT);
        assert_eq!(stats(&db).unwrap().online_cells_left, 0, "같은 서버에 다시 묻지 않는다");
    }

    /// 영문으로 답해도 같은 판정이어야 한다 — 표기 차이로 막히면 안 된다
    #[test]
    fn an_english_province_name_is_understood_too() {
        let (_d, db) = db_with(&[(1, 37.2911, 127.0089)]);
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let mut server = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"South Korea","country_code":"kr","state":"Gyeonggi-do","city":"Suwon-si","borough":"Yeongtong-gu"}}"#,
            None,
        )]);
        set_endpoint(&db, &server.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(server.served(), 1);
        // 도가 맞으므로 받아들인다. 시군구는 fold 의 차례대로 city 가 먼저다.
        assert_eq!(geo_of(&db, 1).1.as_deref(), Some("Gyeonggi-do"));
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("Suwon-si"));
    }

    /// 서버 없이 채운다 — 내장 자료만으로 세 단계가 다 붙는다
    #[test]
    fn the_offline_pass_names_photos_without_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'suwon.jpg',1,0,1,0,0,37.2911,127.0089),
                         (2,1,'suwon2.jpg',1,0,1,0,0,37.2915,127.0092),
                         (3,1,'dokdo.jpg',1,0,1,0,0,37.2411,131.8694),
                         (4,1,'sea.jpg',1,0,1,0,0,38.5,131.5);",
            )
        })
        .unwrap();

        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(p.asked, 0, "서버에 한 번도 묻지 않는다");
        assert_eq!((p.total, p.done, p.files, p.empty), (3, 3, 3, 1), "바다 한 자리는 못 정한다");

        let named = |id: i64| -> (Option<String>, Option<String>, Option<String>) {
            db.read(|c| {
                c.query_row("SELECT geo_country, geo_admin1, geo_admin2 FROM files WHERE id=?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
            })
            .unwrap()
        };
        assert_eq!(named(1), (Some("대한민국".into()), Some("경기도".into()), Some("수원시".into())));
        assert_eq!(named(2).2, Some("수원시".into()), "같은 자리의 다른 사진에도 붙는다");
        // **독도는 한국 땅이다** — 채우기 전체를 지나온 뒤에도 그렇다
        assert_eq!(named(3), (Some("대한민국".into()), Some("경상북도".into()), None));
        assert_eq!(named(4), (None, None, None), "바다는 온라인 몫으로 남는다");

        // 못 정한 자리는 «다시 물어볼 수 있음»으로 남는다 — 못 박지 않는다
        let (status, source): (String, String) = db
            .read(|c| {
                c.query_row(
                    "SELECT status, source FROM places WHERE country IS NULL",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!((status.as_str(), source.as_str()), (UNRESOLVED, SRC_OFFLINE));

        // 판을 캐시에 적어 둔다 — 나중에 어느 자료로 붙였는지 알 수 있게
        let version: Option<String> = db
            .read(|c| c.query_row("SELECT dataset_version FROM places WHERE cell LIKE '37.29%'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version.as_deref(), Some(offline::dataset_version()));

        // 다시 돌려도 할 일이 없다 — 같은 자리를 두 번 판정하지 않는다
        let again = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(again.total, 0);
    }

    /// 오프라인이 채운 자리는 온라인 보강 대상으로 남는다 — 캐시로 오해하면 영영 근사값이다
    #[test]
    fn an_offline_result_still_waits_for_the_online_pass() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2911,127.0089);",
            )
        })
        .unwrap();
        fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();

        let s = stats(&db).unwrap();
        assert_eq!((s.named, s.approximate_files, s.precise_files), (1, 1, 0));
        assert_eq!(s.pending_files, 0, "이름은 붙었으니 «처리할 사진»은 아니다");
        assert_eq!(targets(&db, Mode::Offline, &valid_gps_sql(), None).unwrap().len(), 0);
        assert_eq!(targets(&db, Mode::Online, &valid_gps_sql(), None).unwrap().len(), 1, "정밀 보강 대상이다");
        // 화면이 세는 수와 실제로 처리할 자리 수가 같아야 한다
        assert_eq!(s.offline_cells_left, 0);
        assert_eq!(s.online_cells_left, 1);
    }

    /// 서버가 «이름 없음»이라 한 자리는 **그 서버에는** 다시 묻지 않는다.
    /// 오프라인도 건드리지 않는다 — 값이 없다고 내장 자료로 지어내지 않는다.
    #[test]
    fn a_settled_empty_cell_is_left_alone_by_the_same_server() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);
                 INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                   VALUES(1,1,'a.jpg',1,0,1,0,0,37.2911,127.0089);
                 INSERT INTO places(cell,status,source,precision,
                                    online_outcome,online_provider,online_checked_at,at)
                   VALUES('37.29,127.00','none','nominatim','remote','none','http://a.example/reverse',0,0);",
            )
        })
        .unwrap();
        assert_eq!(targets(&db, Mode::Offline, &valid_gps_sql(), None).unwrap().len(), 0);
        assert_eq!(
            targets(&db, Mode::Online, &valid_gps_sql(), Some("http://a.example/reverse")).unwrap().len(),
            0,
            "같은 서버에는 다시 묻지 않는다"
        );
        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(p.total, 0, "오프라인이 서버의 확정을 뒤집으면 안 된다");

        // 서버를 바꾸면 다시 물어볼 수 있다 — «없다»는 그 서버의 답이었을 뿐이다
        assert_eq!(
            targets(&db, Mode::Online, &valid_gps_sql(), Some("http://b.example/reverse")).unwrap().len(),
            1,
            "다른 서버는 알 수도 있다"
        );
    }

    /// **서버 A 가 «없다»고 한 자리를 서버 B 로 바꾸면 다시 조회한다.**
    ///
    /// «이름이 없다»는 그 서버의 답이지 세상의 사실이 아니다. 자체 Nominatim 의
    /// 지역 자료가 좁아 못 찾은 것을 다른 서버는 알 수도 있다 (2026-09-01).
    #[test]
    fn a_new_server_gets_to_answer_a_cell_the_old_one_gave_up_on() {
        // 내장 경계가 나라를 모르는 자리라야 서버 답이 그대로 쓰인다 —
        // 육지였다면 국가 충돌로 거부되어 이 시험의 뜻이 흐려진다
        let (_d, db) = db_with(&[(1, 0.005, -140.005)]);
        assert_eq!(boundary::country(0.005, -140.005), None);
        let mut a = TestServer::start(vec![("200 OK", r#"{"error":"Unable to geocode"}"#, None)]);
        set_endpoint(&db, &a.url);
        fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(a.served(), 1);
        let status: String = db
            .read(|c| c.query_row("SELECT status FROM places WHERE cell='0.00,-140.01'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(status, NONE, "이름이 없다고 못 박혔다");
        assert_eq!(stats(&db).unwrap().online_cells_left, 0, "그 서버에는 더 물을 것이 없다");

        // 서버를 바꾼다 — 이번엔 답을 안다
        let mut b = TestServer::start(vec![(
            "200 OK",
            r#"{"address":{"country":"어느나라","country_code":"xx","state":"어느주","city":"어느시"}}"#,
            None,
        )]);
        set_endpoint(&db, &b.url);
        assert_eq!(stats(&db).unwrap().online_cells_left, 1, "새 서버에는 물어볼 곳이 있다");

        let p = fill(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        assert_eq!(b.served(), 1, "새 서버에 물어봐야 한다");
        assert_eq!(p.files, 1);
        assert_eq!(geo_of(&db, 1).2.as_deref(), Some("어느시"));
        assert_eq!(stats(&db).unwrap().online_cells_left, 0);
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
                 INSERT INTO places(cell,country,admin1,admin2,name,status,source,precision,at)
                   VALUES('37.28,127.05','대한민국','경기도','수원시','수원시','ok','nominatim','remote',0);",
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
                 INSERT INTO places(cell,country,admin1,admin2,name,status,
                                    online_outcome,online_provider,at)
                   VALUES('10.00,20.00',NULL,NULL,NULL,NULL,'none','none','http://my.server/reverse',0);",
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
        let n = write_place(&db, "37.28,127.05", &place, OK, SRC_ONLINE, PREC_REMOTE, None, None, Some("my.server"), &gps, Overwrite::All, None).unwrap();
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
        write_place(&db, "37.28,127.05", &place, OK, SRC_ONLINE, PREC_REMOTE, None, None, Some("other.server"), &gps, Overwrite::All, None).unwrap();
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
    fn an_http_429_is_worth_asking_again() {
        let mut server = TestServer::once("429 Too Many Requests", r#"{"error":"slow down"}"#);
        match ask(&test_client(), &server.url, 37.5, 127.0, 12) {
            Answer::Retryable { message, retry_after } => {
                assert!(message.contains("429"));
                assert_eq!(retry_after, None, "서버가 Retry-After 를 주지 않았다");
            }
            _ => panic!("다시 물어볼 답이어야 한다"),
        }
        assert_eq!(server.served(), 1);
    }

    /// 4xx 는 주소나 권한이 틀린 것이라 다시 물어도 같은 답이 온다 — 곧바로 멈춘다
    #[test]
    fn a_404_is_not_worth_asking_again() {
        let mut server = TestServer::once("404 Not Found", r#"{}"#);
        match ask(&test_client(), &server.url, 37.5, 127.0, 12) {
            Answer::Fatal(msg) => assert!(msg.contains("404") && msg.contains("주소")),
            _ => panic!("멈춰야 한다"),
        }
        assert_eq!(server.served(), 1);
    }

    #[test]
    fn an_error_hidden_in_a_200_response_stops_instead_of_becoming_a_cache_miss() {
        let mut limited = TestServer::once("200 OK", r#"{"error":"rate limit exceeded"}"#);
        assert!(matches!(ask(&test_client(), &limited.url, 37.5, 127.0, 12), Answer::Fatal(_)));
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

/// 실측용 — 사용자 DB를 복사해 오프라인 채우기를 재 본다.
///
/// `ACUT_BENCH_DB` 에 DB 경로를 주고 `cargo test --lib -- --ignored bench` 로 돌린다.
/// 사용자 DB를 열지 않고 임시 사본만 건드린다.
#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore = "실제 DB가 있어야 한다 — ACUT_BENCH_DB 로 지정"]
    fn offline_fill_on_a_real_library() {
        let Ok(src) = std::env::var("ACUT_BENCH_DB") else { return };
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("bench.db");
        std::fs::copy(&src, &copy).expect("DB 사본을 만들지 못했습니다");
        let db = Db::open(&copy).unwrap();

        // 통계는 위치 사이드바를 열 때마다 돈다 — 여러 번 재서 가운데값을 본다
        let mut cold = vec![];
        let mut before = Stats::default();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            before = stats(&db).unwrap();
            cold.push(t.elapsed().as_millis());
        }
        cold.sort();
        let stats_ms = cold[2];

        let t1 = std::time::Instant::now();
        let p = fill_offline(&db, &AtomicBool::new(false), None, |_| {}).unwrap();
        let fill_ms = t1.elapsed().as_millis();
        let mut warm = vec![];
        let mut after = Stats::default();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            after = stats(&db).unwrap();
            warm.push(t.elapsed().as_millis());
        }
        warm.sort();
        println!("통계 — 채우기 전 {stats_ms}ms · 채운 뒤 {}ms (다섯 번의 가운데값)", warm[2]);

        // 첫 화면이 기다리는 질의들 — 어디가 오래 걸리는지 갈라 본다
        {
            use crate::db::query::{Filter, GroupBy};
            let f = Filter::default();
            let take = |name: &str, ms: Vec<u128>| {
                let mut ms = ms;
                ms.sort();
                println!("첫 화면 · {name} {}ms", ms[ms.len() / 2]);
            };
            let mut a = vec![];
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let _ = crate::db::query::page(&db, &f, None, 200, GroupBy::None).unwrap();
                a.push(t.elapsed().as_millis());
            }
            take("사진 첫 쪽 200장", a);
            let mut b = vec![];
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let _ = crate::db::query::summary(&db, &f).unwrap();
                b.push(t.elapsed().as_millis());
            }
            take("요약", b);
        }

        // 지도 칸 질의는 지도를 움직일 때마다 돈다 — 지명을 얹어 느려졌는지 직접 잰다.
        // 지명이 없던 시절의 질의를 나란히 돌려 같은 조건에서 견준다.
        let old_sql = "SELECT AVG(fi.gps_lat), AVG(fi.gps_lon), COUNT(*), MAX(fi.id)
                         FROM files fi WHERE fi.trashed_at IS NULL
                           AND fi.gps_lat IS NOT NULL AND fi.gps_lon IS NOT NULL
                           AND NOT (fi.gps_lat = 0.0 AND fi.gps_lon = 0.0)
                        GROUP BY CAST((fi.gps_lat + 90.0) / 0.1 AS INTEGER),
                                 CAST((fi.gps_lon + 180.0) / 0.1 AS INTEGER)
                        ORDER BY 3 DESC LIMIT 4000";
        let mut old_ms = vec![];
        let mut new_ms = vec![];
        let mut cells = vec![];
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let n: usize = db
                .read(|c| {
                    let mut st = c.prepare(old_sql)?;
                    let it = st.query_map([], |r| r.get::<_, i64>(2))?;
                    it.collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .len();
            old_ms.push(t.elapsed().as_millis());
            let t = std::time::Instant::now();
            cells = crate::db::query::map_cells(&db, &crate::db::query::Filter::default(), 0.1).unwrap();
            new_ms.push(t.elapsed().as_millis());
            assert_eq!(n, cells.len(), "칸 수가 달라지면 견줄 수 없다");
        }
        old_ms.sort();
        new_ms.sort();
        let named = cells.iter().filter(|c| c.place.is_some()).count();
        println!(
            "지도 칸 — 지명 없이 {}ms · 지명 얹어 {}ms (다섯 번의 가운데값) · 칸 {} 개 · 이름 붙은 칸 {named} 개",
            old_ms[2], new_ms[2], cells.len()
        );
        if let Some(c) = cells.first() {
            println!("가장 큰 칸: {:?} · {} 장 · 섞인 곳 {}", c.place, c.n, c.places);
        }

        println!(
            "통계 {stats_ms}ms · 오프라인 채우기 {fill_ms}ms\n\
             자리 {} 곳 · 사진 {} 장 · 못 정함 {} 곳\n\
             이름 붙은 사진 {} → {} (좌표 있는 사진 {})\n\
             남은 자리 {} → {} (서버만 가능 {})",
            p.done, p.files, p.empty,
            before.named, after.named, after.with_gps,
            before.cells_left, after.cells_left, after.network_cells_left,
        );
    }
}
