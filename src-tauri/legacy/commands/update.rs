use serde::{Deserialize, Serialize};
use tauri::AppHandle;

const GITHUB_REPO: &str = "HyunjoonKwak/acut";
const HTTP_USER_AGENT: &str = "acut-updater";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_notes: String,
    pub release_url: String,
    pub published_at: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
    assets: Vec<GithubAsset>,
}

/// Numeric-aware version comparison: "0.3.10" beats "0.3.9".
/// Non-numeric suffixes within a segment are ignored ("1-beta" -> 1).
fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    version_parts(latest) > version_parts(current)
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();

    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let response = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", HTTP_USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Update check failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Update check failed: HTTP {}", response.status()));
    }

    let release: GithubRelease = response
        .json()
        .await
        .map_err(|e| format!("Invalid release data: {}", e))?;

    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let dmg = release.assets.iter().find(|a| a.name.ends_with(".dmg"));

    Ok(UpdateInfo {
        update_available: is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        release_notes: release.body.unwrap_or_default(),
        release_url: release.html_url,
        published_at: release.published_at,
        asset_name: dmg.map(|a| a.name.clone()),
        asset_url: dmg.map(|a| a.browser_download_url.clone()),
        asset_size: dmg.map(|a| a.size),
    })
}

/// v1 호환용 명령 이름만 남긴다. 설치 파일을 앱이 직접 받으면 macOS의 다운로드
/// 검증 경로를 우회할 수 있으므로 의도적으로 거부한다.
#[tauri::command]
pub async fn download_update(
    _app: AppHandle,
    _asset_url: String,
    _asset_name: String,
) -> Result<String, String> {
    Err(
        "앱 안에서 설치 파일을 직접 받지 않습니다. 공식 릴리스 페이지에서 내려받아 주세요."
            .to_string(),
    )
}

fn is_release_page(url: &str) -> bool {
    let base = format!("https://github.com/{GITHUB_REPO}/releases");
    url == base || url.starts_with(&format!("{base}/"))
}

#[tauri::command]
pub async fn open_release_page(url: String) -> Result<(), String> {
    if !is_release_page(&url) {
        return Err("Unexpected release URL".to_string());
    }
    std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}
