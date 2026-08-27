//! `thumb://` 프로토콜 — 썸네일 캐시만 서빙한다.
//!
//! asset 프로토콜의 scope glob은 한글·공백이 섞인 절대경로에서 기대대로 동작하지
//! 않았다(`**`, `/**` 둘 다 거부). 범용 프로토콜에 넓은 권한을 주는 대신,
//! **캐시 폴더만 여는 전용 프로토콜**을 등록한다. 경로 탈출도 여기서 막는다.
//!
//! 주소 형태: `thumb://localhost/<라이브러리 id>/<캐시 상대경로>`
//!
//! 라이브러리 id가 앞에 붙는 이유: 캐시는 **라이브러리 폴더마다** 따로 있다.
//! 이게 없던 때는 "지금 열린 라이브러리"의 캐시만 뒤져서, 다른 디스크 사진은
//! 썸네일이 이미 만들어져 있는데도 빈 칸으로 나왔다.

use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeResponder};

pub fn handle(app: &AppHandle, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap()
    };

    let Some(state) = app.try_state::<super::AppState>() else {
        responder.respond(not_found());
        return;
    };
    // "/2/53/5314aa.jpg" → 라이브러리 2의 캐시에서 "53/5314aa.jpg"
    let raw = req.uri().path().trim_start_matches('/');
    let Some((lib_id, rest)) = raw.split_once('/') else {
        responder.respond(not_found());
        return;
    };
    let Ok(lib_id) = lib_id.parse::<i64>() else {
        responder.respond(not_found());
        return;
    };
    let decoded = percent_decode(rest);

    // 경로 탈출 차단 — ".."이 하나라도 있으면 거부한다
    if decoded.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        responder.respond(not_found());
        return;
    }

    // 디스크가 빠져 있으면 캐시도 함께 사라진다.
    // 경로는 AppState가 기억한다 — 썸네일 한 장마다 DB를 읽으면 그리드가 멈춘다.
    let Some(dir) = state.library_dir(lib_id) else {
        responder.respond(not_found());
        return;
    };
    let cache_root = crate::media::cache::cache_root(&dir);
    let path = cache_root.join(&decoded);

    // 정규화 후에도 캐시 안에 있어야 한다 (심볼릭 링크 대비)
    let inside = path
        .canonicalize()
        .ok()
        .zip(cache_root.canonicalize().ok())
        .map(|(p, root)| p.starts_with(root))
        .unwrap_or(false);
    if !inside {
        responder.respond(not_found());
        return;
    }

    match std::fs::read(&path) {
        Ok(bytes) => {
            let res = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "image/jpeg")
                // 캐시 키에 원본의 크기·수정시각이 들어 있으므로 내용이 바뀌면
                // 파일명이 바뀐다. 오래 캐시해도 안전하다.
                .header("Cache-Control", "public, max-age=31536000, immutable")
                .body(bytes)
                .unwrap();
            responder.respond(res);
        }
        Err(_) => responder.respond(not_found()),
    }
}

/// %XX 디코딩. 한글 파일명이 인코딩되어 온다.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("53/5314aa.jpg"), "53/5314aa.jpg");
        // 한글 "가" = EA B0 80
        assert_eq!(percent_decode("%EA%B0%80.jpg"), "가.jpg");
        assert_eq!(percent_decode("a%20b.jpg"), "a b.jpg");
        // 잘못된 이스케이프는 그대로 둔다
        assert_eq!(percent_decode("100%.jpg"), "100%.jpg");
    }

    #[test]
    fn traversal_segments_are_detectable() {
        let bad = percent_decode("..%2F..%2Fetc%2Fpasswd");
        assert!(bad.split('/').any(|s| s == ".."), "탈출 시도를 잡아야 한다");
    }
}
