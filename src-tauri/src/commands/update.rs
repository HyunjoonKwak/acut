use std::io::Write;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

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

#[derive(Debug, Clone, Serialize)]
struct UpdateDownloadProgress {
    downloaded: u64,
    total: u64,
    percent: u8,
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

    let latest_version = release
        .tag_name
        .trim_start_matches(['v', 'V'])
        .to_string();
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

/// Downloads the release dmg into ~/Downloads and opens it. Downloading
/// in-app keeps the file free of the quarantine xattr, so the ad-hoc
/// signed app inside is not flagged as damaged by Gatekeeper.
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    asset_url: String,
    asset_name: String,
) -> Result<String, String> {
    let allowed_prefix = format!("https://github.com/{GITHUB_REPO}/releases/download/");
    if !asset_url.starts_with(&allowed_prefix) {
        return Err("Unexpected download URL".to_string());
    }
    if asset_name.contains('/') || asset_name.contains("..") {
        return Err("Unexpected asset name".to_string());
    }

    let download_dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("Downloads folder not found: {}", e))?;
    let target_path = download_dir.join(&asset_name);

    let mut response = reqwest::Client::new()
        .get(&asset_url)
        .header("User-Agent", HTTP_USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_percent: u8 = 0;

    let mut file = std::fs::File::create(&target_path)
        .map_err(|e| format!("Cannot write file: {}", e))?;

    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                let _ = std::fs::remove_file(&target_path);
                return Err(format!("Download interrupted: {}", e));
            }
        };
        if let Err(e) = file.write_all(&chunk) {
            let _ = std::fs::remove_file(&target_path);
            return Err(format!("Cannot write file: {}", e));
        }
        downloaded += chunk.len() as u64;
        if total > 0 {
            let percent = ((downloaded * 100) / total) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = app.emit(
                    "update-download-progress",
                    UpdateDownloadProgress {
                        downloaded,
                        total,
                        percent,
                    },
                );
            }
        }
    }
    file.flush().map_err(|e| format!("Cannot write file: {}", e))?;
    drop(file);

    // Mount the dmg so the user can drag the app into Applications
    std::process::Command::new("open")
        .arg(&target_path)
        .spawn()
        .map_err(|e| format!("Failed to open the downloaded file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_release_page(url: String) -> Result<(), String> {
    let allowed_prefix = format!("https://github.com/{GITHUB_REPO}/releases");
    if !url.starts_with(&allowed_prefix) {
        return Err("Unexpected release URL".to_string());
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}
