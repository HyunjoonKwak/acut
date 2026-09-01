//! 새 판이 나왔는지 살피고, 받아서 열어 준다.
//!
//! 이 앱은 오프라인이 기본이라 바깥과 말을 섞는 자리가 적다. 여기는 그 몇 안 되는
//! 곳이므로 규칙을 좁게 둔다: 주소는 우리 저장소로 못 박고, 사용자가 누르거나
//! 하루 한 번만 살피며, 받는 파일 이름과 크기를 믿지 않고 직접 확인한다.
//!
//! 받은 dmg 를 앱 안에서 여는 이유: 브라우저로 내려받으면 격리 표시(quarantine)가
//! 붙어, 자체 서명한 앱이 «손상되었다»며 열리지 않는다.

use crate::api::{err, AppState};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager, State};

const REPO: &str = "HyunjoonKwak/photo_desk";
const UA: &str = concat!("photo-desk/", env!("CARGO_PKG_VERSION"), " (github.com/HyunjoonKwak/photo_desk)");
/// 이보다 큰 파일은 우리 dmg 가 아니다 — 받다가 디스크를 채우지 않는다
const MAX_BYTES: u64 = 500 * 1024 * 1024;
/// 자동 살피기는 하루 한 번. 오프라인 우선 앱이 바깥을 자주 두드리면 안 된다.
const AUTO_GAP_SECS: i64 = 24 * 60 * 60;
const LAST_CHECK_KEY: &str = "update.last_check";
const AUTO_KEY: &str = "update.auto";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub notes: String,
    pub page_url: String,
    pub published_at: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
    percent: u8,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// 판 번호를 숫자 세 칸으로. 자리가 모자라면 0 으로 채운다 —
/// 그래야 «0.6» 과 «0.6.0» 이 같은 것으로 읽힌다.
fn parts(version: &str) -> [u64; 3] {
    let mut out = [0u64; 3];
    for (i, part) in version.trim().trim_start_matches(['v', 'V']).split('.').take(3).enumerate() {
        out[i] = part
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0);
    }
    out
}

fn is_newer(latest: &str, current: &str) -> bool {
    parts(latest) > parts(current)
}

/// 받아도 되는 파일 이름인가 — 경로를 벗어나는 이름은 거절한다
fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name.ends_with(".dmg")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.contains('\0')
        && !name.starts_with('.')
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(UA)
        .build()
        .map_err(|e| format!("연결을 준비하지 못했습니다: {e}"))
}

async fn fetch_latest(app: &AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let res = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("새 판을 살피지 못했습니다: {}", e.without_url()))?;

    let status = res.status();
    if !status.is_success() {
        // GitHub 는 로그인 없이 시간당 60번만 답한다 — 그때는 사유를 또렷이 말한다
        return Err(match status.as_u16() {
            403 | 429 => "GitHub 가 잠시 요청을 받지 않습니다 — 조금 뒤에 다시 눌러 주세요".into(),
            404 => "아직 게시된 판이 없습니다".into(),
            _ => format!("새 판을 살피지 못했습니다 — 서버가 {status} 로 답했습니다"),
        });
    }

    let release: Release = res
        .json()
        .await
        .map_err(|e| format!("받은 내용을 읽지 못했습니다: {}", e.without_url()))?;

    Ok(to_info(release, &current))
}

/// 받아도 되는 자산 하나 — 우리가 올린 dmg 만. 이름과 주소를 둘 다 본다.
fn pick_asset(release: &Release) -> Option<&Asset> {
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    release
        .assets
        .iter()
        .find(|a| safe_asset_name(&a.name) && a.browser_download_url.starts_with(&prefix) && a.size <= MAX_BYTES)
}

/// 받은 릴리스를 화면이 쓸 모양으로. 네트워크와 떼어 두어 시험할 수 있게 한다.
fn to_info(release: Release, current: &str) -> UpdateInfo {
    let latest = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let dmg = pick_asset(&release);
    UpdateInfo {
        newer: is_newer(&latest, current),
        current: current.to_string(),
        latest,
        asset_name: dmg.map(|a| a.name.clone()),
        asset_url: dmg.map(|a| a.browser_download_url.clone()),
        asset_size: dmg.map(|a| a.size),
        notes: release.body.unwrap_or_default(),
        page_url: release.html_url,
        published_at: release.published_at,
    }
}

/// 사용자가 «확인»을 눌렀을 때. 살핀 시각을 적어 자동 살피기가 겹치지 않게 한다.
#[tauri::command]
pub async fn update_check(app: AppHandle, state: State<'_, AppState>) -> Result<UpdateInfo, String> {
    let info = fetch_latest(&app).await?;
    let now = chrono::Utc::now().timestamp();
    let _ = crate::db::settings::set(&state.db, LAST_CHECK_KEY, &now.to_string());
    Ok(info)
}

/// 앱을 열 때 조용히 한 번. 하루가 안 지났거나 꺼 두었으면 아무것도 하지 않는다.
///
/// 새 판이 있을 때만 `Some` 을 돌려준다 — 화면은 그때만 무언가를 보여 준다.
/// 실패는 오류가 아니라 «모름»이다. 인터넷이 없다고 앱이 잔소리하면 안 된다.
#[tauri::command]
pub async fn update_check_auto(app: AppHandle, state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    let on = crate::db::settings::get(&state.db, AUTO_KEY)
        .map_err(err)?
        .map(|v| v != "off")
        .unwrap_or(true);
    if !on {
        return Ok(None);
    }
    let last: i64 = crate::db::settings::get(&state.db, LAST_CHECK_KEY)
        .map_err(err)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let now = chrono::Utc::now().timestamp();
    if now - last < AUTO_GAP_SECS {
        return Ok(None);
    }
    let _ = crate::db::settings::set(&state.db, LAST_CHECK_KEY, &now.to_string());

    match fetch_latest(&app).await {
        Ok(info) if info.newer => Ok(Some(info)),
        Ok(_) => Ok(None),
        // 인터넷이 없을 수도 있다 — 조용히 넘어간다
        Err(e) => {
            log::info!("새 판 자동 살피기를 건너뜁니다: {e}");
            Ok(None)
        }
    }
}

/// dmg 를 내려받아 연다. 진행은 `update-progress`.
#[tauri::command]
pub async fn update_download(app: AppHandle, asset_url: String, asset_name: String) -> Result<String, String> {
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    if !asset_url.starts_with(&prefix) {
        return Err("우리 저장소의 파일이 아닙니다".into());
    }
    if !safe_asset_name(&asset_name) {
        return Err("받을 수 없는 파일 이름입니다".into());
    }

    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("다운로드 폴더를 찾지 못했습니다: {e}"))?;
    let target = dir.join(&asset_name);

    let mut res = client()?
        .get(&asset_url)
        .send()
        .await
        .map_err(|e| format!("내려받지 못했습니다: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(format!("내려받지 못했습니다 — 서버가 {} 로 답했습니다", res.status()));
    }
    let total = res.content_length().unwrap_or(0);
    if total > MAX_BYTES {
        return Err("파일이 너무 큽니다".into());
    }

    // 받다 만 파일을 그대로 두면 다음에 «이미 있다»고 오해하기 쉽다 —
    // 임시 이름으로 받고 다 받았을 때만 제 이름을 준다.
    let temp = dir.join(format!("{asset_name}.part"));
    let mut file = std::fs::File::create(&temp).map_err(|e| format!("파일을 만들지 못했습니다: {e}"))?;
    let mut done: u64 = 0;
    let mut last_percent = u8::MAX;

    loop {
        let chunk = match res.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(format!("받는 중에 끊겼습니다: {}", e.without_url()));
            }
        };
        done += chunk.len() as u64;
        if done > MAX_BYTES {
            let _ = std::fs::remove_file(&temp);
            return Err("파일이 너무 큽니다".into());
        }
        if let Err(e) = file.write_all(&chunk) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("파일을 쓰지 못했습니다: {e}"));
        }
        if total > 0 {
            let percent = ((done * 100) / total) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = app.emit("update-progress", Progress { done, total, percent });
            }
        }
    }
    file.flush().map_err(|e| format!("파일을 쓰지 못했습니다: {e}"))?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&temp);
        return Err("받은 크기가 알려진 크기와 다릅니다 — 다시 시도해 주세요".into());
    }
    std::fs::rename(&temp, &target).map_err(|e| format!("파일 이름을 바꾸지 못했습니다: {e}"))?;

    // 열어 두면 사용자가 앱을 응용 프로그램 폴더로 끌어다 놓을 수 있다
    std::process::Command::new("/usr/bin/open")
        .arg(&target)
        .spawn()
        .map_err(|e| format!("받은 파일을 열지 못했습니다: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

/// 릴리스 쪽지를 브라우저로 연다
#[tauri::command]
pub async fn update_open_page(url: String) -> Result<(), String> {
    if !url.starts_with(&format!("https://github.com/{REPO}/releases")) {
        return Err("우리 저장소의 주소가 아닙니다".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("브라우저를 열지 못했습니다: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub 이 실제로 준 답 (2026-09-01 v0.6.0). 우리 파싱이 그 모양과 맞는지 본다 —
    /// 필드 이름을 하나만 잘못 적어도 «새 판 없음»으로 조용히 잘못 답하게 된다.
    const REAL: &str = r###"{
      "tag_name": "v0.6.0",
      "html_url": "https://github.com/HyunjoonKwak/photo_desk/releases/tag/v0.6.0",
      "body": "## 사진이 어디서 찍혔는지, 서버 없이 알려 줍니다",
      "published_at": "2026-09-01T05:08:04Z",
      "assets": [{
        "name": "_0.6.0_aarch64.dmg",
        "browser_download_url": "https://github.com/HyunjoonKwak/photo_desk/releases/download/v0.6.0/_0.6.0_aarch64.dmg",
        "size": 19922944
      }]
    }"###;

    fn real() -> Release {
        serde_json::from_str(REAL).expect("실제 답을 읽지 못했습니다")
    }

    #[test]
    fn it_reads_what_github_actually_sends() {
        let info = to_info(real(), "0.6.0");
        assert_eq!(info.latest, "0.6.0", "v 는 떼어 낸다");
        assert!(!info.newer, "같은 판이면 새 판이 아니다");
        assert_eq!(info.asset_name.as_deref(), Some("_0.6.0_aarch64.dmg"));
        assert_eq!(info.asset_size, Some(19922944));
        assert!(info.page_url.ends_with("/v0.6.0"));
        assert!(info.notes.starts_with("## 사진이"));
        assert_eq!(info.published_at.as_deref(), Some("2026-09-01T05:08:04Z"));

        // 옛 판을 쓰고 있으면 새 판이라고 알려 준다
        let info = to_info(real(), "0.5.4");
        assert!(info.newer);
        assert!(info.asset_url.is_some());
    }

    /// 자산이 없거나 우리 것이 아니면 «받기»를 내밀지 않는다 — 주소를 그대로 믿지 않는다
    #[test]
    fn it_never_offers_a_download_it_should_not() {
        let none: Release = serde_json::from_str(r#"{"tag_name":"v9.0.0","html_url":"x","assets":[]}"#).unwrap();
        assert!(pick_asset(&none).is_none(), "자산이 없다");

        let elsewhere: Release = serde_json::from_str(
            r#"{"tag_name":"v9.0.0","html_url":"x","assets":[{"name":"a.dmg","browser_download_url":"https://evil.example/a.dmg","size":10}]}"#,
        )
        .unwrap();
        assert!(pick_asset(&elsewhere).is_none(), "우리 저장소가 아닌 주소");

        let huge: Release = serde_json::from_str(
            r#"{"tag_name":"v9.0.0","html_url":"x","assets":[{"name":"a.dmg","browser_download_url":"https://github.com/HyunjoonKwak/photo_desk/releases/download/v9/a.dmg","size":999999999999}]}"#,
        )
        .unwrap();
        assert!(pick_asset(&huge).is_none(), "너무 큰 파일");

        let script: Release = serde_json::from_str(
            r#"{"tag_name":"v9.0.0","html_url":"x","assets":[{"name":"run.sh","browser_download_url":"https://github.com/HyunjoonKwak/photo_desk/releases/download/v9/run.sh","size":10}]}"#,
        )
        .unwrap();
        assert!(pick_asset(&script).is_none(), "dmg 가 아닌 것");
    }

    /// 답에 없는 칸이 있어도 살아남아야 한다 — 없는 것과 못 읽는 것은 다르다
    #[test]
    fn a_release_without_notes_or_assets_still_reads() {
        let bare: Release = serde_json::from_str(r#"{"tag_name":"v0.7.0","html_url":"u"}"#).unwrap();
        let info = to_info(bare, "0.6.0");
        assert!(info.newer);
        assert_eq!(info.notes, "");
        assert_eq!(info.asset_url, None, "받을 것이 없으면 내밀지 않는다");
    }

    #[test]
    fn a_bigger_number_is_a_newer_version() {
        assert!(is_newer("0.6.1", "0.6.0"));
        assert!(is_newer("0.10.0", "0.9.9"), "10 은 9 보다 크다 — 글자 순서로 견주면 진다");
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.6.0", "0.6.0"));
        assert!(!is_newer("0.5.9", "0.6.0"), "옛 판을 새 판이라 하면 안 된다");
    }

    #[test]
    fn the_v_prefix_and_missing_places_do_not_confuse_it() {
        assert!(!is_newer("v0.6.0", "0.6.0"), "v 는 붙었을 뿐 같은 판이다");
        assert!(!is_newer("0.6", "0.6.0"), "자리가 모자라도 같은 판이다");
        assert!(is_newer("0.7", "0.6.9"));
        assert_eq!(parts("v1.2.3"), [1, 2, 3]);
        assert_eq!(parts("1.2"), [1, 2, 0]);
        assert_eq!(parts("1.2.3-beta.4"), [1, 2, 3], "꼬리표는 무시한다");
        assert_eq!(parts("그냥글자"), [0, 0, 0]);
        assert_eq!(parts("1.2.3.4"), [1, 2, 3], "네 번째 자리는 보지 않는다");
    }

    /// 받는 파일 이름은 우리가 정하지 않는다 — 저장소에서 온 값이므로 믿지 않는다
    #[test]
    fn a_file_name_can_never_escape_the_downloads_folder() {
        assert!(safe_asset_name("에이컷_0.6.0_aarch64.dmg"), "한글 이름은 정상이다");
        assert!(safe_asset_name("A-Cut_0.4.1_aarch64.dmg"));
        assert!(!safe_asset_name("../../etc/passwd.dmg"));
        assert!(!safe_asset_name("a/b.dmg"));
        assert!(!safe_asset_name("a\\b.dmg"));
        assert!(!safe_asset_name(".hidden.dmg"));
        assert!(!safe_asset_name("no-extension"));
        assert!(!safe_asset_name("script.sh"));
        assert!(!safe_asset_name(""));
        assert!(!safe_asset_name(&format!("{}.dmg", "a".repeat(300))));
    }
}
