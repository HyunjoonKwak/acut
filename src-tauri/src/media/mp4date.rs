//! 영상 안에 박힌 촬영 시각 — MP4/MOV 컨테이너를 직접 읽는다.
//!
//! Spotlight(kMDItemContentCreationDate)는 색인이 안 된 파일에서 **파일 시스템의
//! 생성 시각**을 돌려준다. 실측: NAS에서 받은 영상 194개가 전부 «2026-07-01»
//! (복사한 날)로 잡혔고, 그 파일의 mvhd에는 2015-04-08이 들어 있었다.
//!
//! 순서: `moov/meta`의 `com.apple.quicktimetime.creationdate`(시간대 포함, 아이폰·DJI)
//! → `moov/udta/©day` → `moov/mvhd` creation_time(1904년 기준 초, UTC).
//! MPEG-TS(.mts)처럼 상자 구조가 아니면 None — 그럼 부르는 쪽이 다른 단서를 쓴다.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 1904-01-01 → 1970-01-01 (초)
const QT_EPOCH: i64 = 2_082_844_800;
/// 2000-01-01 이전은 믿지 않는다 — 카메라 기본값(1904·1970·2000)이 흔하다
const MIN_PLAUSIBLE: i64 = 946_684_800;

struct Box {
    offset: u64,
    size: u64,
    header: u64,
    kind: [u8; 4],
}

fn read_boxes(f: &mut File, start: u64, end: u64) -> Vec<Box> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        if f.seek(SeekFrom::Start(pos)).is_err() {
            break;
        }
        let mut h = [0u8; 8];
        if f.read_exact(&mut h).is_err() {
            break;
        }
        let mut size = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind = [h[4], h[5], h[6], h[7]];
        let mut header = 8;
        if size == 1 {
            let mut l = [0u8; 8];
            if f.read_exact(&mut l).is_err() {
                break;
            }
            size = u64::from_be_bytes(l);
            header = 16;
        } else if size == 0 {
            size = end - pos;
        }
        if size < header {
            break;
        }
        out.push(Box { offset: pos, size, header, kind });
        pos += size;
        if out.len() > 512 {
            break;
        }
    }
    out
}

fn read_at(f: &mut File, at: u64, n: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut v = vec![0u8; n];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

/// mvhd — 버전 0은 32비트, 1은 64비트 시각
fn mvhd_creation(f: &mut File, b: &Box) -> Option<i64> {
    let body = read_at(f, b.offset + b.header, 20)?;
    let secs = if body[0] == 1 {
        u64::from_be_bytes(body[4..12].try_into().ok()?) as i64
    } else {
        u32::from_be_bytes(body[4..8].try_into().ok()?) as i64
    };
    plausible(secs - QT_EPOCH)
}

fn plausible(t: i64) -> Option<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(i64::MAX);
    (t >= MIN_PLAUSIBLE && t <= now + 86_400).then_some(t)
}

/// `2015-04-08T11:33:09+0900` / `…+09:00` / `…Z` → 유닉스 초
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('\0');
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> { s.get(a..a + n)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    // 초 뒤에 소수점이 올 수 있다
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let offset: i64 = match b.get(i) {
        Some(b'Z') | None => 0,
        Some(sign @ (b'+' | b'-')) => {
            let rest: String = s[i + 1..].chars().filter(|c| c.is_ascii_digit()).collect();
            if rest.len() < 4 {
                return None;
            }
            let hh: i64 = rest[0..2].parse().ok()?;
            let mm: i64 = rest[2..4].parse().ok()?;
            let o = hh * 3600 + mm * 60;
            if *sign == b'-' {
                -o
            } else {
                o
            }
        }
        _ => return None,
    };
    let date = chrono::NaiveDate::from_ymd_opt(y as i32, mo as u32, d as u32)?;
    let t = date.and_hms_opt(h as u32, mi as u32, sec as u32)?;
    plausible(t.and_utc().timestamp() - offset)
}

/// 상자 안을 통째로 읽어 ISO 시각 문자열을 찾는다 (meta/ilst·udta). 크기는 제한한다.
fn find_iso_in(f: &mut File, b: &Box, keys: &[&[u8]]) -> Option<i64> {
    let n = (b.size - b.header).min(64 * 1024) as usize;
    let body = read_at(f, b.offset + b.header, n)?;
    for key in keys {
        let mut from = 0;
        while let Some(i) = body[from..].windows(key.len()).position(|w| w == *key) {
            let at = from + i + key.len();
            // 키 뒤 200바이트 안에 «YYYY-MM-DDT» 꼴이 온다
            let win = &body[at..body.len().min(at + 200)];
            if let Some(j) = win.windows(11).position(|w| {
                w[4] == b'-' && w[7] == b'-' && w[10] == b'T' && w[..4].iter().all(u8::is_ascii_digit)
            }) {
                let s: String = win[j..].iter().take(40).map(|&c| c as char).collect();
                if let Some(t) = parse_iso(&s) {
                    return Some(t);
                }
            }
            from = at;
        }
    }
    None
}

/// 컨테이너에 박힌 촬영 시각(유닉스 초, UTC). 없거나 상자 구조가 아니면 None.
pub fn creation_time(path: &Path) -> Option<i64> {
    let mut f = File::open(path).ok()?;
    let end = f.metadata().ok()?.len();
    // 앞 8바이트가 상자 꼴인지 — ftyp/moov/mdat/wide/free 아니면 MP4가 아니다
    let head = read_at(&mut f, 4, 4)?;
    if !matches!(&head[..], b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip") {
        return None;
    }
    let top = read_boxes(&mut f, 0, end);
    let moov = top.iter().find(|b| &b.kind == b"moov")?;
    let inner = read_boxes(&mut f, moov.offset + moov.header, moov.offset + moov.size);
    // 1) 시간대가 든 문자열 — 아이폰·DJI·고프로
    for b in inner.iter().filter(|b| &b.kind == b"meta" || &b.kind == b"udta") {
        if let Some(t) = find_iso_in(&mut f, b, &[b"com.apple.quicktime.creationdate", b"\xa9day"]) {
            return Some(t);
        }
    }
    // 2) mvhd — 표준. UTC로 본다 (일부 기기는 현지 시각을 그대로 넣는다)
    inner.iter().find(|b| &b.kind == b"mvhd").and_then(|b| mvhd_creation(&mut f, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    fn mvhd_v0(creation_unix: i64) -> Vec<u8> {
        let mut body = vec![0u8; 100];
        let qt = (creation_unix + QT_EPOCH) as u32;
        body[4..8].copy_from_slice(&qt.to_be_bytes());
        boxed(b"mvhd", &body)
    }

    fn write(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("v.mp4");
        std::fs::write(&p, bytes).unwrap();
        (d, p)
    }

    #[test]
    fn reads_mvhd_creation_time() {
        let t = 1_428_460_389; // 2015-04-08 02:33:09 UTC
        let file = [boxed(b"ftyp", b"isom"), boxed(b"moov", &mvhd_v0(t))].concat();
        let (_d, p) = write(&file);
        assert_eq!(creation_time(&p), Some(t));
    }

    #[test]
    fn prefers_the_timezone_aware_string_over_mvhd() {
        let iso = b"com.apple.quicktime.creationdate\x00\x00\x00\x00data\x00\x00\x00\x012015-04-08T11:33:09+0900";
        let moov = [boxed(b"meta", iso), mvhd_v0(1_000_000_000)].concat();
        let file = [boxed(b"ftyp", b"isom"), boxed(b"moov", &moov)].concat();
        let (_d, p) = write(&file);
        assert_eq!(creation_time(&p), Some(1_428_460_389));
    }

    #[test]
    fn implausible_or_missing_dates_are_none() {
        let file = [boxed(b"ftyp", b"isom"), boxed(b"moov", &mvhd_v0(0))].concat();
        let (_d, p) = write(&file);
        assert_eq!(creation_time(&p), None);
        let (_d2, p2) = write(b"not a video at all, just bytes here");
        assert_eq!(creation_time(&p2), None);
    }

    #[test]
    fn iso_strings_with_offsets() {
        assert_eq!(parse_iso("2015-04-08T11:33:09+0900"), Some(1_428_460_389));
        assert_eq!(parse_iso("2015-04-08T02:33:09Z"), Some(1_428_460_389));
        assert_eq!(parse_iso("2015-04-08T02:33:09.123+00:00"), Some(1_428_460_389));
        assert_eq!(parse_iso("1999-01-01T00:00:00Z"), None);
    }
}
