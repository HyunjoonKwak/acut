//! 촬영일 결정 — 사진이 언제 찍혔는지 정한다.
//!
//! photo_manager(안드로이드)에서 실전 검증된 폴백 체인을 옮긴 것이다.
//! 그쪽 체인은 `EXIF → DATE_TAKEN → min-plausible(DATE_MODIFIED, DATE_ADDED) → now`
//! 인데, macOS에는 MediaStore가 없으므로 그 자리를 **파일명 파싱**이 대신한다.
//!
//! ```text
//! EXIF DateTimeOriginal
//!   → 파일명에서 추출        (20260101_123456 형식이 2만 8천 장)
//!   → min-plausible(mtime, birthtime)
//!   → now
//! ```
//!
//! `min-plausible`이 핵심이다. 두 가지를 동시에 막는다:
//!   - **그럴듯함 검사** — 2000-01-01 이후만 인정한다. 배터리가 죽은 기기의
//!     1970/1980 날짜를 걸러낸다.
//!   - **더 이른 쪽 선택** — PC로 옮기면 생성일이 "복사한 날"로 바뀐다.
//!     보존된 수정일이 더 이르면 그쪽이 진짜에 가깝다.
//!
//! 결과와 함께 **어디서 나온 값인지**([`Source`])를 남긴다. 그래야 정리할 때
//! "이 날짜는 추정입니다"를 보여줄 수 있고, 나중에 정확한 값이 생기면
//! 추정치만 골라 갱신할 수 있다.

/// 촬영일의 출처. DB의 `files.taken_at_source`에 그대로 들어간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Source {
    /// EXIF DateTimeOriginal — 가장 믿을 수 있다
    Exif = 0,
    /// 파일명에서 뽑았다 — 갤럭시·픽셀 등은 거의 정확하다
    Filename = 1,
    /// 파일 시각에서 추정했다 — 부정확할 수 있다
    FileTime = 2,
    /// 알 수 없어 현재 시각을 넣었다
    Unknown = 3,
}

/// 2000-01-01 00:00:00 UTC. 이보다 이른 값은 기기 시계 오류로 본다.
pub const PLAUSIBLE_FROM: i64 = 946_684_800;

/// 그럴듯한 시각인가. 2000년 이후이고 미래가 아니어야 한다.
pub fn is_plausible(ts: i64, now: i64) -> bool {
    // 하루치 여유 — 시간대 차이로 조금 앞선 값이 들어올 수 있다.
    ts >= PLAUSIBLE_FROM && ts <= now + 86_400
}

/// 촬영일을 정한다. 항상 값을 돌려준다 (`taken_at`은 NOT NULL).
pub fn resolve(
    exif: Option<i64>,
    file_name: &str,
    mtime: Option<i64>,
    birthtime: Option<i64>,
    now: i64,
) -> (i64, Source) {
    if let Some(t) = exif.filter(|&t| is_plausible(t, now)) {
        return (t, Source::Exif);
    }
    if let Some(t) = from_filename(file_name).filter(|&t| is_plausible(t, now)) {
        return (t, Source::Filename);
    }
    if let Some(t) = min_plausible(mtime, birthtime, now) {
        return (t, Source::FileTime);
    }
    (now, Source::Unknown)
}

/// 그럴듯한 것들 중 더 이른 쪽.
///
/// 복사·이동으로 생성일이 오늘로 바뀌어도, 보존된 수정일이 더 이르면
/// 그것을 택한다. 둘 다 그럴듯하지 않으면 None.
pub fn min_plausible(a: Option<i64>, b: Option<i64>, now: i64) -> Option<i64> {
    let a = a.filter(|&t| is_plausible(t, now));
    let b = b.filter(|&t| is_plausible(t, now));
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

/// 파일명에서 촬영 시각을 뽑는다.
///
/// 다루는 형태 (실제 라이브러리에서 관측된 것들):
/// ```text
/// 20260101_123456.jpg            갤럭시  ← NAS에 2만 8천 장
/// IMG_20260101_123456.jpg        안드로이드 일반
/// PXL_20260101_123456789.jpg     픽셀
/// VID_20260101_123456.mp4
/// Screenshot_20260101-123456.png
/// 2026-01-01 12.34.56.jpg        아이폰 내보내기
/// 2026-01-01_123456.jpg
/// 20260101.jpg                   날짜만 (자정으로)
/// ```
pub fn from_filename(name: &str) -> Option<i64> {
    let digits: Vec<u8> = name.bytes().collect();
    let mut i = 0;
    while i < digits.len() {
        if !digits[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // 숫자 덩어리의 끝을 찾되, 사이의 구분자(- . _ 공백)는 건너뛰며 모은다.
        let mut buf = Vec::with_capacity(14);
        let mut j = i;
        while j < digits.len() && buf.len() < 14 {
            let c = digits[j];
            if c.is_ascii_digit() {
                buf.push(c - b'0');
                j += 1;
            } else if matches!(c, b'-' | b'.' | b'_' | b' ' | b':') && !buf.is_empty() {
                // 구분자 다음이 숫자일 때만 이어 붙인다.
                if j + 1 < digits.len() && digits[j + 1].is_ascii_digit() {
                    j += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if let Some(ts) = parse_digits(&buf) {
            return Some(ts);
        }
        // 이 덩어리는 실패. 한 칸만 전진해 다시 시도한다.
        // "3_20260815_093012"처럼 앞에 짧은 숫자가 붙은 경우, 덩어리 전체를
        // 건너뛰면 뒤의 진짜 날짜를 놓친다. 파일명은 짧으므로 비용은 무시할 만하다.
        i += 1;
    }
    None
}

/// 모아 온 숫자열을 날짜로 해석한다. 14자리(초까지) 또는 8자리(날짜만).
fn parse_digits(d: &[u8]) -> Option<i64> {
    let num = |r: &[u8]| r.iter().fold(0i64, |a, &x| a * 10 + x as i64);
    if d.len() >= 14 {
        let (y, mo, da) = (num(&d[0..4]), num(&d[4..6]), num(&d[6..8]));
        let (h, mi, s) = (num(&d[8..10]), num(&d[10..12]), num(&d[12..14]));
        if valid_date(y, mo, da) && h < 24 && mi < 60 && s < 60 {
            return to_unix(y, mo, da, h, mi, s);
        }
        return None;
    }
    if d.len() == 8 {
        let (y, mo, da) = (num(&d[0..4]), num(&d[4..6]), num(&d[6..8]));
        if valid_date(y, mo, da) {
            return to_unix(y, mo, da, 0, 0, 0);
        }
    }
    None
}

fn valid_date(y: i64, mo: i64, da: i64) -> bool {
    (1990..=2100).contains(&y) && (1..=12).contains(&mo) && (1..=31).contains(&da)
}

/// 그레고리력 → 유닉스 시각 (UTC 기준).
///
/// 시간대를 적용하지 않는 이유: 파일명의 시각은 촬영 기기의 지역 시각이고,
/// 우리는 그 값을 날짜 폴더로 쓸 뿐이다. 여기서 UTC 변환을 하면 자정 근처
/// 사진이 하루 밀린다. 지역 시각을 그대로 두는 편이 폴더 분류에 맞다.
fn to_unix(y: i64, mo: i64, da: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    // days_from_civil (Howard Hinnant 알고리즘)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + mi * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000; // 2027년 어딘가
    fn t(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
        to_unix(y, mo, d, h, mi, s).unwrap()
    }

    // ── 파일명 파싱 ────────────────────────────────────────────────────
    #[test]
    fn galaxy_format() {
        assert_eq!(from_filename("20260101_123456.jpg"), Some(t(2026, 1, 1, 12, 34, 56)));
    }
    #[test]
    fn android_img_prefix() {
        assert_eq!(from_filename("IMG_20260815_093012.jpg"), Some(t(2026, 8, 15, 9, 30, 12)));
    }
    #[test]
    fn pixel_format_with_extra_digits() {
        // PXL은 밀리초까지 붙는다. 앞 14자리만 쓴다.
        assert_eq!(from_filename("PXL_20260815_093012345.jpg"), Some(t(2026, 8, 15, 9, 30, 12)));
    }
    #[test]
    fn iphone_export_with_dots() {
        assert_eq!(from_filename("2026-08-15 09.30.12.jpg"), Some(t(2026, 8, 15, 9, 30, 12)));
    }
    #[test]
    fn screenshot_with_hyphen() {
        assert_eq!(from_filename("Screenshot_20260815-093012.png"), Some(t(2026, 8, 15, 9, 30, 12)));
    }
    #[test]
    fn date_only_becomes_midnight() {
        assert_eq!(from_filename("20260815.jpg"), Some(t(2026, 8, 15, 0, 0, 0)));
    }
    #[test]
    fn video_prefix() {
        assert_eq!(from_filename("VID_20260815_093012.mp4"), Some(t(2026, 8, 15, 9, 30, 12)));
    }

    #[test]
    fn rejects_things_that_are_not_dates() {
        // 카메라 일련번호 — 8자리지만 날짜가 아니다
        assert_eq!(from_filename("DSC_0031.JPG"), None);
        assert_eq!(from_filename("C0086.MP4"), None);
        assert_eq!(from_filename("IMG_1898.JPG"), None);
        // 월/일이 범위를 벗어남
        assert_eq!(from_filename("20261301_120000.jpg"), None);
        assert_eq!(from_filename("20260132_120000.jpg"), None);
        // 시각이 범위를 벗어남
        assert_eq!(from_filename("20260101_250000.jpg"), None);
        assert_eq!(from_filename("20260101_126000.jpg"), None);
        // 연도가 비현실적
        assert_eq!(from_filename("18000101.jpg"), None);
        assert_eq!(from_filename(""), None);
        assert_eq!(from_filename("photo.jpg"), None);
    }

    #[test]
    fn finds_the_date_even_with_a_prefix_number() {
        // 앞에 다른 숫자가 붙어 있어도 뒤의 날짜를 찾아낸다
        assert_eq!(from_filename("3_20260815_093012.jpg"), Some(t(2026, 8, 15, 9, 30, 12)));
    }

    // ── min-plausible ─────────────────────────────────────────────────
    #[test]
    fn picks_the_earlier_of_two_plausible_times() {
        let older = t(2020, 5, 5, 10, 0, 0);
        let newer = t(2026, 1, 1, 10, 0, 0);
        assert_eq!(min_plausible(Some(older), Some(newer), NOW), Some(older));
        assert_eq!(min_plausible(Some(newer), Some(older), NOW), Some(older));
    }

    #[test]
    fn ignores_dead_clock_timestamps() {
        // 1980년 — RTC가 죽은 기기가 흔히 남기는 값
        let dead = 315_532_800;
        let good = t(2020, 5, 5, 10, 0, 0);
        assert_eq!(min_plausible(Some(dead), Some(good), NOW), Some(good));
        assert_eq!(min_plausible(Some(dead), None, NOW), None);
        assert_eq!(min_plausible(Some(0), Some(dead), NOW), None);
    }

    #[test]
    fn ignores_future_timestamps() {
        let future = NOW + 86_400 * 30;
        let good = t(2020, 5, 5, 10, 0, 0);
        assert_eq!(min_plausible(Some(future), Some(good), NOW), Some(good));
    }

    // ── 전체 체인 ──────────────────────────────────────────────────────
    #[test]
    fn exif_wins_over_everything() {
        let exif = t(2018, 7, 25, 14, 31, 0);
        let (ts, src) = resolve(Some(exif), "20260101_123456.jpg", Some(NOW), Some(NOW), NOW);
        assert_eq!((ts, src), (exif, Source::Exif));
    }

    #[test]
    fn filename_is_used_when_exif_is_missing() {
        // 카톡 저장본처럼 EXIF가 날아간 경우
        let (ts, src) = resolve(None, "20200505_101112.jpg", Some(NOW), Some(NOW), NOW);
        assert_eq!((ts, src), (t(2020, 5, 5, 10, 11, 12), Source::Filename));
    }

    #[test]
    fn implausible_exif_falls_through_to_filename() {
        // EXIF가 1970년이면 믿지 않는다
        let (ts, src) = resolve(Some(0), "20200505_101112.jpg", None, None, NOW);
        assert_eq!((ts, src), (t(2020, 5, 5, 10, 11, 12), Source::Filename));
    }

    #[test]
    fn file_time_is_the_third_choice() {
        let mtime = t(2019, 3, 3, 8, 0, 0);
        let birth = t(2026, 1, 1, 8, 0, 0); // PC로 옮기며 오늘로 바뀐 값
        let (ts, src) = resolve(None, "DSC_0031.JPG", Some(mtime), Some(birth), NOW);
        assert_eq!(
            (ts, src),
            (mtime, Source::FileTime),
            "복사로 늦춰진 생성일 대신 보존된 수정일을 써야 한다"
        );
    }

    #[test]
    fn falls_back_to_now_when_nothing_is_known() {
        let (ts, src) = resolve(None, "photo.jpg", None, None, NOW);
        assert_eq!((ts, src), (NOW, Source::Unknown));
    }

    #[test]
    fn source_values_match_the_schema() {
        // DB에 정수로 들어가므로 값이 바뀌면 안 된다
        assert_eq!(Source::Exif as i32, 0);
        assert_eq!(Source::Filename as i32, 1);
        assert_eq!(Source::FileTime as i32, 2);
        assert_eq!(Source::Unknown as i32, 3);
    }

    // ── 날짜 변환 ──────────────────────────────────────────────────────
    #[test]
    fn unix_epoch_conversion_is_correct() {
        assert_eq!(t(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(t(2000, 1, 1, 0, 0, 0), PLAUSIBLE_FROM);
        assert_eq!(t(2024, 2, 29, 12, 0, 0), 1_709_208_000); // 윤년
    }
}
