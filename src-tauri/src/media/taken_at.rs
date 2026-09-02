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

use chrono::{Datelike, Local, TimeZone, Timelike, Utc};

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
    /// 사용자가 명시적으로 고친 시각. 파일 내부에 쓸 수 없는 형식도 재스캔 뒤 유지한다.
    Manual = 4,
}

/// 2000-01-01 지역 자정을 모든 시간대에서 포함하도록 UTC 기준 하루 여유를 둔다.
pub const PLAUSIBLE_FROM: i64 = 946_598_400;

/// 그럴듯한 시각인가. 2000년 이후이고 미래가 아니어야 한다.
pub fn is_plausible(ts: i64, now: i64) -> bool {
    // 하루치 여유 — 시간대 차이로 조금 앞선 값이 들어올 수 있다.
    ts >= PLAUSIBLE_FROM && ts <= now + 86_400
}

/// 영상의 촬영일 — 단서 가운데 **가장 이른 그럴듯한 것**.
///
/// 영상은 EXIF가 없고, 컨테이너의 시각은 다시 인코딩·내보내기한 날로 바뀌기
/// 일쑤다(실측: 2017년 영상의 mvhd가 2026-07-01, 구글포토 내보내기는 파일명에
/// 2021이 있는데 컨테이너·파일 시각은 2026-08-26). 복사·변환은 시각을 뒤로만
/// 미루므로 가장 이른 값이 진짜에 가장 가깝다. 폴더 이름(«2017-11-12 반도4차…»)도 단서다.
pub fn resolve_video(
    embedded: Option<i64>,
    file_name: &str,
    folder_name: &str,
    mtime: Option<i64>,
    birthtime: Option<i64>,
    now: i64,
) -> (i64, Source) {
    let mut best: Option<(i64, Source)> = None;
    let mut offer = |t: Option<i64>, src: Source| {
        if let Some(t) = t.filter(|&t| is_plausible(t, now)) {
            if best.is_none_or(|(b, _)| t < b) {
                best = Some((t, src));
            }
        }
    };
    offer(embedded, Source::Exif);
    offer(from_filename(file_name), Source::Filename);
    offer(from_filename(folder_name), Source::Filename);
    offer(mtime, Source::FileTime);
    offer(birthtime, Source::FileTime);
    best.unwrap_or((now, Source::Unknown))
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
    // 날짜 꼴이 하나도 없을 때만 — 13자리 순수 숫자 덩어리는 유닉스 밀리초다.
    // `kakaotalk_1525225566458.mp4`·`FB_IMG_…` 처럼 메신저·SNS가 저장한 파일은
    // 이것 말고는 촬영 시각 단서가 없다 (컨테이너 시각은 0, 파일 시각은 복사한 날).
    epoch_ms_run(&digits)
}

/// 앞뒤가 숫자가 아닌 13자리 밀리초와, 그 뒤에 붙은 0~6자리 순번을 읽는다.
/// 덩어리의 일부(꼬리)는 보지 않는다 — `20261301_120000`의 꼬리를 시각으로 오인한다.
/// 열 자리 초는 믿지 않는다 — 웹 이미지 id와 구분이 안 된다.
fn epoch_ms_run(b: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let len = i - start;
        if (13..=19).contains(&len) {
            let ms = b[start..start + 13]
                .iter()
                .fold(0i64, |a, &c| a * 10 + (c - b'0') as i64);
            if let Some(t) = epoch_secs(ms / 1000) {
                return Some(t);
            }
        }
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
            return Some(civil_to_unix(y, mo, da, h, mi, s));
        }
        return None;
    }
    // 12자리 — 두 가지 꼴이 있다. 분까지(`2022_05_14 19_17`) 또는 두 자리 연도에
    // 초까지(`AH001_am_sm_210609_155304` = 2021-06-09 15:53:04, 갤럭시 편집본).
    if d.len() == 12 {
        let (y, mo, da) = (num(&d[0..4]), num(&d[4..6]), num(&d[6..8]));
        let (h, mi) = (num(&d[8..10]), num(&d[10..12]));
        if valid_date(y, mo, da) && y <= 2100 && h < 24 && mi < 60 {
            return Some(civil_to_unix(y, mo, da, h, mi, 0));
        }
        let (y2, mo2, da2) = (2000 + num(&d[0..2]), num(&d[2..4]), num(&d[4..6]));
        let (h2, mi2, s2) = (num(&d[6..8]), num(&d[8..10]), num(&d[10..12]));
        if valid_date(y2, mo2, da2) && h2 < 24 && mi2 < 60 && s2 < 60 {
            return Some(civil_to_unix(y2, mo2, da2, h2, mi2, s2));
        }
        return None;
    }
    if d.len() == 8 {
        let (y, mo, da) = (num(&d[0..4]), num(&d[4..6]), num(&d[6..8]));
        if valid_date(y, mo, da) {
            return Some(civil_to_unix(y, mo, da, 0, 0, 0));
        }
    }
    None
}

/// 2000-01-01 ≤ t < 2030-03-17. 그 밖은 시각이 아니라 다른 숫자로 본다.
fn epoch_secs(t: i64) -> Option<i64> {
    (946_684_800..1_900_000_000).contains(&t).then_some(t)
}

fn valid_date(y: i64, mo: i64, da: i64) -> bool {
    (1990..=2100).contains(&y)
        && chrono::NaiveDate::from_ymd_opt(y as i32, mo as u32, da as u32).is_some()
}

/// 시간대 없는 촬영 기기의 지역 시각 → 실제 유닉스 시각.
///
/// EXIF와 파일명에는 대개 offset이 없다. 앱이 실행 중인 기기의 지역 시각으로
/// 해석해 저장하면 파일시각·영상 컨테이너처럼 이미 UTC인 값과 같은 의미가 된다.
pub fn civil_to_unix(y: i64, mo: i64, da: i64, h: i64, mi: i64, s: i64) -> i64 {
    let Some(date) = chrono::NaiveDate::from_ymd_opt(y as i32, mo as u32, da as u32) else { return 0 };
    let Some(naive) = date.and_hms_opt(h as u32, mi as u32, s as u32) else { return 0 };
    Local.from_local_datetime(&naive)
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or_else(|| naive.and_utc().timestamp())
}

/// 구버전 DB가 UTC처럼 저장했던 floating civil 초를 실제 Unix 시각으로 바꾼다.
pub fn floating_civil_to_unix(ts: i64) -> i64 {
    let Some(old) = chrono::DateTime::<Utc>::from_timestamp(ts, 0) else { return ts };
    civil_to_unix(
        old.year() as i64,
        old.month() as i64,
        old.day() as i64,
        old.hour() as i64,
        old.minute() as i64,
        old.second() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000; // 2027년 어딘가
    fn t(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
        civil_to_unix(y, mo, d, h, mi, s)
    }

    // ── 파일명 파싱 ────────────────────────────────────────────────────
    #[test]
    fn unix_epoch_ms_in_names() {
        // 카카오톡·페이스북 저장본 — 13자리 밀리초
        assert_eq!(from_filename("kakaotalk_1525225566458.mp4"), Some(1_525_225_566));
        assert_eq!(from_filename("FB_IMG_1525225566458.jpg"), Some(1_525_225_566));
        // 카카오 내보내기는 13자리 epoch 뒤에 짧은 순번을 붙이기도 한다.
        assert_eq!(from_filename("1502088228879113.jpg"), Some(1_502_088_228));
        assert_eq!(from_filename("Kakao_1502088228879000000.jpg"), Some(1_502_088_228));
        // 순번은 최대 6자리만. 더 긴 숫자 id의 앞을 epoch로 오인하지 않는다.
        assert_eq!(from_filename("15020882288790000000.jpg"), None);
        // 열 자리 초는 믿지 않는다 — 웹 이미지 id와 구분이 안 된다
        assert_eq!(from_filename("1525225566.jpg"), None);
        // 13자리라도 2000~2030년 밖이면 시각이 아니다
        assert_eq!(from_filename("IMG_0900000000000.jpg"), None);
        // 날짜 꼴이 같이 있으면 날짜가 이긴다
        assert_eq!(
            from_filename("20180305_171923_1525225566458.mp4"),
            Some(civil_to_unix(2018, 3, 5, 17, 19, 23))
        );
    }

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
    fn two_digit_year_with_seconds() {
        assert_eq!(from_filename("AH001_am_sm_210609_155304.mp4"), Some(civil_to_unix(2021, 6, 9, 15, 53, 4)));
    }

    #[test]
    fn video_takes_the_earliest_plausible_clue() {
        let now = civil_to_unix(2026, 8, 29, 0, 0, 0);
        let re_encoded = civil_to_unix(2026, 7, 1, 0, 0, 0);
        let copied = civil_to_unix(2017, 11, 17, 14, 8, 32);
        // 컨테이너는 재인코딩 날, 파일 시각은 복사한 날, 폴더 이름이 행사 날
        let (t, s) = resolve_video(Some(re_encoded), "2동 옥상뷰(1).mp4", "2017-11-12 반도4차 현장 방문", Some(copied), Some(copied), now);
        assert_eq!((t, s), (civil_to_unix(2017, 11, 12, 0, 0, 0), Source::Filename));
        // 파일명에 두 자리 연도 — 컨테이너·파일 시각이 다 늦어도 파일명이 이긴다
        let late = civil_to_unix(2026, 8, 26, 13, 26, 7);
        let (t, s) = resolve_video(Some(late), "AH001_am_sm_210609_155304.mp4", "2021년의 사진", Some(late), Some(late), now);
        assert_eq!((t, s), (civil_to_unix(2021, 6, 9, 15, 53, 4), Source::Filename));
        // 단서가 없으면 지금
        assert_eq!(resolve_video(None, "a.mp4", "b", None, None, now).1, Source::Unknown);
    }

    #[test]
    fn filename_with_minutes_only() {
        assert_eq!(from_filename("2022_05_14 19_17 (1).mp4"), Some(civil_to_unix(2022, 5, 14, 19, 17, 0)));
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
        assert_eq!(Source::Manual as i32, 4);
    }

    // ── 날짜 변환 ──────────────────────────────────────────────────────
    #[test]
    fn unix_epoch_conversion_is_correct() {
        for (y, mo, d, h, mi, s) in [(1970, 1, 1, 0, 0, 0), (2000, 1, 1, 0, 0, 0), (2024, 2, 29, 12, 0, 0)] {
            let actual = Local.timestamp_opt(t(y, mo, d, h, mi, s), 0).single().unwrap();
            assert_eq!(
                (actual.year(), actual.month(), actual.day(), actual.hour(), actual.minute(), actual.second()),
                (y as i32, mo as u32, d as u32, h as u32, mi as u32, s as u32)
            );
        }
    }

    #[test]
    fn local_midnight_does_not_cross_a_date_boundary() {
        for (y, mo, d, h, mi, s) in [(2024, 1, 1, 0, 0, 1), (2024, 12, 31, 23, 59, 59)] {
            let actual = Local.timestamp_opt(t(y, mo, d, h, mi, s), 0).single().unwrap();
            assert_eq!((actual.year(), actual.month(), actual.day(), actual.hour()), (y as i32, mo as u32, d as u32, h as u32));
        }
    }

    #[test]
    fn rejects_impossible_calendar_dates_including_non_leap_february() {
        assert_eq!(from_filename("20230229_120000.jpg"), None);
        assert_eq!(from_filename("20240431_120000.jpg"), None);
        assert!(from_filename("20240229_120000.jpg").is_some());
    }

    #[test]
    fn old_floating_timestamp_is_migrated_to_the_same_wall_clock() {
        let old = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            .and_hms_opt(18, 0, 0).unwrap().and_utc().timestamp();
        assert_eq!(floating_civil_to_unix(old), t(2024, 1, 1, 18, 0, 0));
    }
}
