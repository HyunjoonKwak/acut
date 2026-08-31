pub mod ssh;
// Synology DSM Web API client for NAS upload integration.
// Follows the same conventions as the companion nas_photo project:
// - API paths are probed at runtime via SYNO.API.Info (never hardcoded)
// - Credentials are never persisted; only the session id (sid) is kept in memory
// - CreateFolder params are JSON-encoded arrays (bare strings break on Korean
//   or digit-leading folder names)

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const CORE_APIS: &[&str] = &[
    "SYNO.API.Auth",
    "SYNO.FileStation.List",
    "SYNO.FileStation.CreateFolder",
    "SYNO.FileStation.Upload",
];

// Applied to metadata calls (probe/login/list/create) so an unresponsive NAS
// fails fast; uploads stay untimed because large videos can take minutes.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
pub struct ApiInfo {
    pub path: String,
    #[serde(rename = "maxVersion")]
    pub max_version: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NasEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct DsmClient {
    http: reqwest::Client,
    base_url: String,
    api_info: HashMap<String, ApiInfo>,
    sid: String,
    pub account: String,
}

pub fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

fn build_http(verify_tls: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(!verify_tls)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP 클라이언트 생성 실패: {}", e))
}

fn dsm_error_message(api: &str, code: i64) -> String {
    let detail = if api == "SYNO.API.Auth" {
        match code {
            400 => "계정 또는 비밀번호가 올바르지 않습니다",
            401 => "비활성화된 계정입니다",
            402 => "권한이 없는 계정입니다",
            403 => "2단계 인증 코드가 필요합니다. OTP 코드를 입력해주세요",
            404 => "2단계 인증 코드가 올바르지 않습니다",
            407 => "이 IP가 DSM 자동 차단 목록에 있습니다. DSM 보안 설정을 확인해주세요",
            408..=410 => "비밀번호가 만료되었습니다. DSM에서 비밀번호를 변경해주세요",
            _ => "인증에 실패했습니다",
        }
    } else {
        match code {
            105 => "권한이 부족합니다 (세션이 만료되었을 수 있습니다)",
            119 => "세션이 만료되었습니다. NAS에 다시 연결해주세요",
            407 => "대상 폴더에 쓰기 권한이 없습니다",
            414 => "같은 이름의 파일 또는 폴더가 이미 존재합니다",
            1805 => "같은 이름의 파일이 이미 존재합니다 (덮어쓰기 꺼짐)",
            _ => "DSM 요청이 실패했습니다",
        }
    };
    format!("[{}] {} ({})", code, detail, api)
}

/// Extract the numeric DSM error code from a message built by dsm_error_message.
pub fn error_code(message: &str) -> Option<i64> {
    let rest = message.strip_prefix('[')?;
    let end = rest.find(']')?;
    rest[..end].parse().ok()
}

async fn parse_dsm_response(resp: reqwest::Response, api: &str) -> Result<Value, String> {
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("{} HTTP 오류: {}", api, status));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("{} 응답 파싱 실패: {}", api, e))?;
    if body.get("success").and_then(|v| v.as_bool()) == Some(true) {
        Ok(body.get("data").cloned().unwrap_or(Value::Null))
    } else {
        let code = body
            .pointer("/error/code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        Err(dsm_error_message(api, code))
    }
}

impl DsmClient {
    pub async fn connect(
        base_url: &str,
        account: &str,
        password: &str,
        otp_code: Option<&str>,
        verify_tls: bool,
    ) -> Result<Self, String> {
        let base = normalize_base_url(base_url);
        let http = build_http(verify_tls)?;

        // Probe available APIs and their entry paths
        let url = format!("{}/webapi/query.cgi", base);
        let resp = http
            .get(&url)
            .timeout(REQUEST_TIMEOUT)
            .query(&[
                ("api", "SYNO.API.Info"),
                ("version", "1"),
                ("method", "query"),
                ("query", &CORE_APIS.join(",")),
            ])
            .send()
            .await
            .map_err(|e| format!("NAS에 연결할 수 없습니다: {}", e))?;
        let data = parse_dsm_response(resp, "SYNO.API.Info").await?;
        let api_info: HashMap<String, ApiInfo> =
            serde_json::from_value(data).map_err(|e| format!("API 정보 파싱 실패: {}", e))?;
        let auth = api_info
            .get("SYNO.API.Auth")
            .ok_or("이 서버에서 DSM 인증 API를 찾을 수 없습니다")?
            .clone();

        // Login with the user's own DSM account; password is used once and dropped
        let version = auth.max_version.clamp(3, 6);
        let mut form: Vec<(&str, String)> = vec![
            ("api", "SYNO.API.Auth".to_string()),
            ("version", version.to_string()),
            ("method", "login".to_string()),
            ("account", account.to_string()),
            ("passwd", password.to_string()),
            ("session", "FileStation".to_string()),
            ("format", "sid".to_string()),
        ];
        if let Some(otp) = otp_code {
            if !otp.trim().is_empty() {
                form.push(("otp_code", otp.trim().to_string()));
            }
        }
        let url = format!("{}/webapi/{}", base, auth.path);
        let resp = http
            .post(&url)
            .timeout(REQUEST_TIMEOUT)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("로그인 요청 실패: {}", e))?;
        let data = parse_dsm_response(resp, "SYNO.API.Auth").await?;
        let sid = data
            .get("sid")
            .and_then(|v| v.as_str())
            .ok_or("로그인 응답에 세션 정보가 없습니다")?
            .to_string();

        Ok(Self {
            http,
            base_url: base,
            api_info,
            sid,
            account: account.to_string(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn api(&self, name: &str) -> Result<&ApiInfo, String> {
        self.api_info
            .get(name)
            .ok_or_else(|| format!("{} API를 사용할 수 없습니다", name))
    }

    fn endpoint(&self, api: &ApiInfo) -> String {
        format!("{}/webapi/{}", self.base_url, api.path)
    }

    pub async fn list_shares(&self) -> Result<Vec<NasEntry>, String> {
        let api = self.api("SYNO.FileStation.List")?;
        let version = api.max_version.min(2);
        let resp = self
            .http
            .get(self.endpoint(api))
            .timeout(REQUEST_TIMEOUT)
            .query(&[
                ("api", "SYNO.FileStation.List"),
                ("version", &version.to_string()),
                ("method", "list_share"),
                ("_sid", &self.sid),
            ])
            .send()
            .await
            .map_err(|e| format!("공유 폴더 조회 실패: {}", e))?;
        let data = parse_dsm_response(resp, "SYNO.FileStation.List").await?;
        Ok(parse_entries(data.get("shares")))
    }

    pub async fn list_folders(&self, folder_path: &str) -> Result<Vec<NasEntry>, String> {
        let api = self.api("SYNO.FileStation.List")?;
        let version = api.max_version.min(2);
        let resp = self
            .http
            .get(self.endpoint(api))
            .timeout(REQUEST_TIMEOUT)
            .query(&[
                ("api", "SYNO.FileStation.List"),
                ("version", &version.to_string()),
                ("method", "list"),
                ("folder_path", folder_path),
                ("filetype", "dir"),
                ("sort_by", "name"),
                ("_sid", &self.sid),
            ])
            .send()
            .await
            .map_err(|e| format!("폴더 조회 실패: {}", e))?;
        let data = parse_dsm_response(resp, "SYNO.FileStation.List").await?;
        Ok(parse_entries(data.get("files")))
    }

    pub async fn create_folder(&self, parent: &str, name: &str) -> Result<String, String> {
        let api = self.api("SYNO.FileStation.CreateFolder")?;
        let version = api.max_version.min(2);
        // JSON-encoded arrays are required for Korean / digit-leading names
        let folder_path = serde_json::to_string(&[parent]).map_err(|e| e.to_string())?;
        let folder_name = serde_json::to_string(&[name]).map_err(|e| e.to_string())?;
        let form: Vec<(&str, String)> = vec![
            ("api", "SYNO.FileStation.CreateFolder".to_string()),
            ("version", version.to_string()),
            ("method", "create".to_string()),
            ("folder_path", folder_path),
            ("name", folder_name),
            ("force_parent", "true".to_string()),
            ("_sid", self.sid.clone()),
        ];
        let resp = self
            .http
            .post(self.endpoint(api))
            .timeout(REQUEST_TIMEOUT)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("폴더 생성 실패: {}", e))?;
        let data = parse_dsm_response(resp, "SYNO.FileStation.CreateFolder").await?;
        data.pointer("/folders/0/path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or("폴더 생성 응답이 올바르지 않습니다".to_string())
    }

    pub async fn upload_file(
        &self,
        dest_dir: &str,
        local_path: &Path,
        file_name: &str,
        mtime_ms: Option<i64>,
        overwrite: bool,
    ) -> Result<(), String> {
        let api = self.api("SYNO.FileStation.Upload")?;
        let version = api.max_version.min(2);

        let file = tokio::fs::File::open(local_path)
            .await
            .map_err(|e| format!("파일 열기 실패: {}", e))?;
        let len = file
            .metadata()
            .await
            .map_err(|e| format!("파일 정보 읽기 실패: {}", e))?
            .len();
        let stream = tokio_util::io::ReaderStream::new(file);
        let part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(stream),
            len,
        )
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;

        let mut form = reqwest::multipart::Form::new()
            .text("path", dest_dir.to_string())
            .text("create_parents", "true")
            .text("overwrite", if overwrite { "true" } else { "false" });
        if let Some(mtime) = mtime_ms {
            form = form.text("mtime", mtime.to_string());
        }
        // The file part must come after all text fields
        let form = form.part("file", part);

        let resp = self
            .http
            .post(self.endpoint(api))
            .query(&[
                ("api", "SYNO.FileStation.Upload"),
                ("version", &version.to_string()),
                ("method", "upload"),
                ("_sid", &self.sid),
            ])
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("업로드 요청 실패: {}", e))?;
        parse_dsm_response(resp, "SYNO.FileStation.Upload")
            .await
            .map(|_| ())
    }

    pub async fn logout(&self) {
        if let Ok(auth) = self.api("SYNO.API.Auth") {
            let _ = self
                .http
                .get(self.endpoint(auth))
                .query(&[
                    ("api", "SYNO.API.Auth"),
                    ("version", "1"),
                    ("method", "logout"),
                    ("session", "FileStation"),
                    ("_sid", &self.sid),
                ])
                .send()
                .await;
        }
    }
}

fn parse_entries(value: Option<&Value>) -> Vec<NasEntry> {
    value
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    let name = e.get("name")?.as_str()?.to_string();
                    let path = e.get("path")?.as_str()?.to_string();
                    let is_dir = e.get("isdir").and_then(|v| v.as_bool()).unwrap_or(true);
                    Some(NasEntry { name, path, is_dir })
                })
                .filter(|e| e.is_dir)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{error_code, normalize_base_url};

    #[test]
    fn normalizes_bare_host() {
        assert_eq!(normalize_base_url("192.168.0.10:5000"), "http://192.168.0.10:5000");
        assert_eq!(normalize_base_url(" https://nas.local:5001/ "), "https://nas.local:5001");
    }

    #[test]
    fn extracts_dsm_error_code() {
        assert_eq!(error_code("[1805] 같은 이름의 파일 (SYNO.FileStation.Upload)"), Some(1805));
        assert_eq!(error_code("[403] OTP 필요"), Some(403));
        assert_eq!(error_code("일반 오류"), None);
    }
}
