//! `photo://` 프로토콜 — 뷰어용 큰 미리보기를 서빙한다.
//!
//! 요청이 오면 캐시를 보고, 없으면 그 자리에서 만든다. 미리 전부 만들지 않는
//! 이유는 6만 장의 2560px 미리보기가 수십 GB가 되기 때문이다. 실제로 크게 보는
//! 사진은 극히 일부다.
//!
//! `thumb://`와 나눈 이유: 썸네일은 스캔 때 일괄 생성되고 미리보기는 요청 시
//! 만들어진다. 수명과 크기가 달라 캐시 폴더도 따로 둔다.

use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeResponder};

pub fn handle(app: &AppHandle, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let fail = |code: StatusCode| Response::builder().status(code).body(Vec::new()).unwrap();

    let Some(state) = app.try_state::<super::AppState>() else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };
    let Ok(lib) = state.current() else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };

    // photo://localhost/<file_id>
    let id: i64 = match req.uri().path().trim_start_matches('/').parse() {
        Ok(v) => v,
        Err(_) => {
            responder.respond(fail(StatusCode::BAD_REQUEST));
            return;
        }
    };

    // 원본 경로와 무효화 키를 함께 읽는다
    let row = state.db.read(|c| {
        c.query_row(
            "SELECT fo.rel_path, fi.name, fi.size, COALESCE(fi.modified_at,0)
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
    });
    let Ok((dir, name, size, mtime)) = row else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };

    let rel = crate::media::cache::rel_path(&dir, &name);
    let src = lib.volume_mount.join(&rel);
    let key = crate::media::cache::key_for(&rel, size as u64, mtime);
    let out = crate::media::cache::thumb_path(
        &crate::media::cache::preview_root(&lib.root),
        &key,
    );

    // 캐시에 있으면 그대로, 없으면 만든다
    if !out.is_file() {
        let r = crate::media::thumbnail::make(
            &src,
            &out,
            crate::media::cache::PREVIEW_PX,
            crate::media::cache::PREVIEW_QUALITY,
        );
        if r.is_err() {
            responder.respond(fail(StatusCode::UNSUPPORTED_MEDIA_TYPE));
            return;
        }
    }

    match std::fs::read(&out) {
        Ok(bytes) => responder.respond(
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "image/jpeg")
                // 키에 원본의 크기·수정시각이 들어 있어 내용이 바뀌면 이름이 바뀐다
                .header("Cache-Control", "public, max-age=31536000, immutable")
                .body(bytes)
                .unwrap(),
        ),
        Err(_) => responder.respond(fail(StatusCode::NOT_FOUND)),
    }
}
