//! `video://` 프로토콜 — 원본 영상을 웹뷰가 재생할 수 있게 흘려준다.
//!
//! 썸네일은 정지 프레임이라 무엇이 찍혔는지 알기 어렵다. 그리드에서 마우스를
//! 올리면 그 자리에서 재생되어야 한다.
//!
//! **Range 요청을 지원해야 한다.** 안 그러면 `<video>`가 400MB짜리 파일을
//! 통째로 받으려 하고, 되감기·건너뛰기도 안 된다. 실제 영상 평균이 217MB다.
//!
//! asset 프로토콜을 쓰지 않는 이유는 썸네일과 같다 — scope glob이 한글·공백
//! 경로에서 동작하지 않는다.

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeResponder};

/// 한 번에 보낼 최대 크기. 너무 크면 앞부분 재생이 늦고, 너무 작으면
/// 요청이 잦아진다.
const CHUNK: u64 = 4 * 1024 * 1024;

pub fn handle(app: &AppHandle, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let fail = |code: StatusCode| Response::builder().status(code).body(Vec::new()).unwrap();

    let Some(state) = app.try_state::<super::AppState>() else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };
    // video://localhost/<file_id>
    let Ok(id) = req.uri().path().trim_start_matches('/').parse::<i64>() else {
        responder.respond(fail(StatusCode::BAD_REQUEST));
        return;
    };

    let row = state.db.read(|c| {
        c.query_row(
            "SELECT fo.volume_uuid,
                    fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name,
                    fi.ext
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.id = ?1 AND fi.kind = 1 AND fi.trashed_at IS NULL",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
    });
    let Ok((volume_uuid, vol_rel, ext)) = row else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };
    let Some(mount) = crate::db::volumes::find_mount(&volume_uuid) else {
        responder.respond(fail(StatusCode::SERVICE_UNAVAILABLE));
        return;
    };
    let path = mount.join(&vol_rel);

    let Ok(meta) = std::fs::metadata(&path) else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };
    let len = meta.len();
    let mime = mime_for(ext.as_deref());

    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_range(v, len));

    match range {
        Some((start, end)) => {
            let Some(bytes) = read_part(&path, start, end) else {
                responder.respond(fail(StatusCode::NOT_FOUND));
                return;
            };
            responder.respond(
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                    .header(header::CONTENT_LENGTH, (end - start + 1).to_string())
                    .body(bytes)
                    .unwrap(),
            );
        }
        None => {
            // 첫 요청 — 앞부분만 보낸다. 400MB를 통째로 올리면 재생이 시작조차 안 된다.
            let end = (CHUNK - 1).min(len.saturating_sub(1));
            let Some(bytes) = read_part(&path, 0, end) else {
                responder.respond(fail(StatusCode::NOT_FOUND));
                return;
            };
            responder.respond(
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_RANGE, format!("bytes 0-{end}/{len}"))
                    .header(header::CONTENT_LENGTH, (end + 1).to_string())
                    .body(bytes)
                    .unwrap(),
            );
        }
    }
}

fn read_part(path: &std::path::Path, start: u64, end: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(start)).ok()?;
    let n = (end - start + 1) as usize;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// `bytes=0-` / `bytes=100-200` 을 (시작, 끝)으로. 끝은 포함이다.
///
/// 열린 범위(`bytes=100-`)는 한 덩어리만 준다. 그래야 400MB짜리도 곧바로
/// 재생이 시작된다.
pub fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let spec = value.strip_prefix("bytes=")?.trim();
    // 여러 구간 요청은 첫 것만 본다
    let spec = spec.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;

    let (start, end) = if a.is_empty() {
        // bytes=-500 → 마지막 500바이트. 이것도 한 조각까지만 — «bytes=-400000000» 이
        // 통째로 메모리에 오르던 길 (리뷰 H11)
        let n: u64 = b.parse().ok()?;
        let n = n.min(len).min(CHUNK);
        (len - n, len - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        if start >= len {
            return None;
        }
        // 닫힌 구간도 CHUNK 를 넘지 않는다 — «bytes=0-419430399» 한 번에 400MB 를 메모리에
        // 올리던 길. 웹뷰가 여러 영상을 훑으면 그만큼 곱이 된다 (리뷰 H11)
        let end = if b.is_empty() {
            (start + CHUNK - 1).min(len - 1)
        } else {
            b.parse::<u64>().ok()?.min(start + CHUNK - 1).min(len - 1)
        };
        (start, end)
    };
    (start <= end).then_some((start, end))
}

fn mime_for(ext: Option<&str>) -> &'static str {
    match ext.unwrap_or("") {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "3gp" => "video/3gpp",
        _ => "video/mp4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_range_is_capped_to_one_chunk() {
        // 400MB짜리에 bytes=0- 이 오면 전부 주면 안 된다.
        let len = 400 * 1024 * 1024;
        let (s, e) = parse_range("bytes=0-", len).unwrap();
        assert_eq!(s, 0);
        assert_eq!(e, CHUNK - 1, "한 덩어리만");
    }

    #[test]
    fn closed_range_is_honoured() {
        assert_eq!(parse_range("bytes=100-200", 1000), Some((100, 200)));
        // 끝이 파일보다 크면 파일 끝까지
        assert_eq!(parse_range("bytes=900-99999", 1000), Some((900, 999)));
        // 닫힌 구간도 한 조각(CHUNK)까지만 — 나머지는 다음 요청이 가져간다
        assert_eq!(parse_range("bytes=0-99999999", 400_000_000), Some((0, CHUNK - 1)));
    }

    #[test]
    fn suffix_range_reads_the_tail() {
        // mp4의 moov atom이 끝에 있으면 플레이어가 꼬리를 먼저 요청한다
        assert_eq!(parse_range("bytes=-500", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-99999", 1000), Some((0, 999)));
        // 꼬리도 한 조각(CHUNK)까지만
        assert_eq!(parse_range("bytes=-400000000", 400_000_000), Some((400_000_000 - CHUNK, 399_999_999)));
    }

    #[test]
    fn nonsense_ranges_are_rejected() {
        assert_eq!(parse_range("bytes=2000-", 1000), None, "파일 밖");
        assert_eq!(parse_range("items=0-1", 1000), None, "bytes가 아님");
        assert_eq!(parse_range("bytes=abc", 1000), None);
        assert_eq!(parse_range("bytes=0-", 0), None, "빈 파일");
    }

    #[test]
    fn multipart_takes_the_first_range() {
        assert_eq!(parse_range("bytes=0-99,200-299", 1000), Some((0, 99)));
    }

    #[test]
    fn mime_covers_what_we_scan() {
        for e in ["mp4", "mov", "avi", "mkv", "m4v", "3gp", "webm"] {
            assert!(mime_for(Some(e)).starts_with("video/"), "{e}");
        }
        assert_eq!(mime_for(None), "video/mp4");
    }
}
