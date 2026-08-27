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

    // photo://localhost/<file_id>
    let id: i64 = match req.uri().path().trim_start_matches('/').parse() {
        Ok(v) => v,
        Err(_) => {
            responder.respond(fail(StatusCode::BAD_REQUEST));
            return;
        }
    };

    // 파일이 **자기 라이브러리**를 통해 경로를 푼다. 열려 있는 라이브러리를
    // 기준으로 하면 다른 디스크 사진에서 엉뚱한 경로를 찾게 된다.
    let row = state.db.read(|c| {
        c.query_row(
            "SELECT fo.library_id, fo.volume_uuid,
                    fo.rel_path || CASE WHEN fo.rel_path = '' THEN '' ELSE '/' END || fi.name,
                    fi.size, COALESCE(fi.modified_at, 0), fi.kind
             FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fi.id = ?1 AND fo.library_id IS NOT NULL",
            [id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i32>(5)?,
                ))
            },
        )
    });
    let Ok((lib_id, volume_uuid, vol_rel, size, mtime, kind)) = row else {
        responder.respond(fail(StatusCode::NOT_FOUND));
        return;
    };

    // 원본을 읽어야 하므로 디스크는 꽂혀 있어야 한다
    let Some(mount) = crate::db::volumes::find_mount(&volume_uuid) else {
        responder.respond(fail(StatusCode::SERVICE_UNAVAILABLE));
        return;
    };

    // 무효화 키는 볼륨 기준 상대경로로 만든다 — 썸네일 생성 쪽과 같아야 한다
    let src = mount.join(&vol_rel);
    let key = crate::media::cache::key_for(&vol_rel, size as u64, mtime);
    let out =
        crate::media::cache::thumb_path(
            &crate::media::cache::preview_root(&state.cache_base, lib_id),
            &key,
        );

    // 캐시에 있으면 그대로, 없으면 만든다.
    // 영상은 ImageIO가 못 여니 QuickLook이 대표 프레임을 준다.
    if !out.is_file() {
        let r = if kind == 1 {
            crate::media::video::thumbnail(
                &src,
                &out,
                crate::media::cache::PREVIEW_PX,
                crate::media::cache::PREVIEW_QUALITY,
            )
        } else {
            crate::media::thumbnail::make(
                &src,
                &out,
                crate::media::cache::PREVIEW_PX,
                crate::media::cache::PREVIEW_QUALITY,
            )
        };
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
