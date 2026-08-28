//! 모델 파일 — 어디에 있고, 있는지 없는지, 없으면 받아 오기.
//!
//! 앱에 350MB를 넣지 않는다. 처음 쓸 때 앱 데이터 폴더로 한 번 받는다.
//!
//! fp32판(352MB)을 쓴다. 양자화(uint8, 89MB)·fp16(176MB)도 재 봤는데 이
//! 맥의 CPU에서는 fp32가 제일 빨랐다 — 초당 96장 대 61·69장. CoreML(신경
//! 엔진)은 이 그래프에서 이득이 없었다. 한 번 받는 것이라 크기보다 속도.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelId {
    /// CLIP ViT-B/32 그림 쪽 — 비슷한 사진
    ClipVision,
}

pub struct Spec {
    pub id: ModelId,
    /// 앱 데이터 아래 `models/<dir>/<file>`
    pub dir: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// 받을 크기 (진행 표시용). 실제와 조금 달라도 된다.
    pub bytes: u64,
}

pub const SPECS: &[Spec] = &[Spec {
    id: ModelId::ClipVision,
    dir: "clip-vit-b32",
    file: "vision_model.onnx",
    url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/vision_model.onnx",
    bytes: 351_700_000,
}];

pub fn spec(id: ModelId) -> &'static Spec {
    SPECS.iter().find(|s| s.id == id).expect("모델 사양")
}

pub fn path(app_data: &Path, id: ModelId) -> PathBuf {
    let s = spec(id);
    app_data.join("models").join(s.dir).join(s.file)
}

/// 다 받아졌나. 받다 만 파일(.part)은 없는 것으로 친다.
pub fn present(app_data: &Path, id: ModelId) -> bool {
    let p = path(app_data, id);
    p.is_file() && std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub id: ModelId,
    pub got: u64,
    pub total: u64,
}

/// 받는다. `.part`에 쓰다가 끝나면 제 이름으로 — 중간에 끊겨도 반쪽이 모델로 보이지 않는다.
pub fn download(
    app_data: &Path,
    id: ModelId,
    on_progress: impl Fn(&DownloadProgress),
) -> super::Result<PathBuf> {
    let s = spec(id);
    let dest = path(app_data, id);
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let part = dest.with_extension("onnx.part");

    let rt = tokio::runtime::Runtime::new().map_err(|e| super::AiError::Other(e.to_string()))?;
    rt.block_on(async {
        use tokio::io::AsyncWriteExt;
        let resp = reqwest::get(s.url)
            .await
            .map_err(|e| super::AiError::Other(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(super::AiError::Other(format!("받기 실패: HTTP {}", resp.status())));
        }
        let total = resp.content_length().unwrap_or(s.bytes);
        let mut file = tokio::fs::File::create(&part).await?;
        let mut got = 0u64;
        let mut last = std::time::Instant::now();
        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| super::AiError::Other(e.to_string()))?;
            file.write_all(&chunk).await?;
            got += chunk.len() as u64;
            if last.elapsed().as_millis() >= 100 {
                last = std::time::Instant::now();
                on_progress(&DownloadProgress { id, got, total });
            }
        }
        file.flush().await?;
        on_progress(&DownloadProgress { id, got, total });
        Ok::<(), super::AiError>(())
    })?;
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_partial_file_is_not_present() {
        let d = tempfile::tempdir().unwrap();
        assert!(!present(d.path(), ModelId::ClipVision));
        let p = path(d.path(), ModelId::ClipVision);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"stub").unwrap();
        assert!(!present(d.path(), ModelId::ClipVision), "너무 작으면 받다 만 것");
    }
}
