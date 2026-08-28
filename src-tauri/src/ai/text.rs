//! 글로 찾기 — 글을 사진과 같은 512차원 공간으로 보낸다.
//!
//! 모델은 sentence-transformers/clip-ViT-B-32-multilingual-v1 (Apache-2.0).
//! CLIP ViT-B/32의 이미지 공간에 맞춰 증류한 다국어 DistilBERT라 한국어로
//! 쳐도 된다 — 원래 CLIP 텍스트 타워는 영어뿐이다. ONNX에는 트랜스포머만
//! 들어 있어 평균 풀링과 Dense(768→512, 편향·활성 없음)는 여기서 한다.
//! 가중치는 safetensors 하나(1.6MB)를 직접 읽는다 — 형식이 단순해 의존성을
//! 더 들이지 않는다.

use super::clip::normalize;
use super::models::{self, ModelId};
use super::{AiError, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use std::path::Path;
use std::sync::Mutex;
use tokenizers::Tokenizer;

pub const DIM: usize = 512;
const HIDDEN: usize = 768;
/// 모델이 배운 길이. 더 길면 자른다 — 찾는 글이 그보다 길 일은 없다.
const MAX_TOKENS: usize = 128;

pub struct Text {
    session: Mutex<Session>,
    tok: Tokenizer,
    /// [512 × 768] 행 우선 — y = W·x
    dense: Vec<f32>,
    ids_name: String,
    mask_name: String,
}

impl Text {
    pub fn load(app_data: &Path) -> Result<Self> {
        if !models::text_present(app_data) {
            return Err(AiError::NoModel);
        }
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(models::path(app_data, ModelId::TextModel))?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let pick = |what: &str| {
            names
                .iter()
                .find(|n| n.contains(what))
                .cloned()
                .ok_or_else(|| AiError::Other(format!("텍스트 모델 입력에 {what}가 없습니다: {names:?}")))
        };
        let ids_name = pick("input_ids")?;
        let mask_name = pick("attention_mask")?;
        let tok = Tokenizer::from_file(models::path(app_data, ModelId::TextTokenizer))
            .map_err(|e| AiError::Other(format!("토크나이저: {e}")))?;
        let dense = load_dense(&models::path(app_data, ModelId::TextDense))?;
        Ok(Text { session: Mutex::new(session), tok, dense, ids_name, mask_name })
    }

    /// 글 한 줄 → 길이 1 벡터. 사진 벡터와 내적하면 곧 닮은 정도.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let enc = self
            .tok
            .encode(text, true)
            .map_err(|e| AiError::Other(format!("토크나이저: {e}")))?;
        let n = enc.get_ids().len().min(MAX_TOKENS);
        if n == 0 {
            return Err(AiError::Other("빈 글".into()));
        }
        let ids: Vec<i64> = enc.get_ids()[..n].iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = enc.get_attention_mask()[..n].iter().map(|&x| x as i64).collect();

        let hidden = {
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let outputs = s.run(ort::inputs![
                self.ids_name.as_str() => ort::value::Tensor::from_array(([1, n], ids))?,
                self.mask_name.as_str() => ort::value::Tensor::from_array(([1, n], mask.clone()))?,
            ])?;
            let view = outputs[0].try_extract_array::<f32>()?;
            view.iter().copied().collect::<Vec<f32>>()
        };
        if hidden.len() != n * HIDDEN {
            return Err(AiError::Other(format!("텍스트 모델 출력 크기가 다릅니다: {}", hidden.len())));
        }
        Ok(normalize(&project(&self.dense, &pool(&hidden, &mask, n))))
    }
}

/// 마스크가 1인 토큰들의 평균 — 768
fn pool(hidden: &[f32], mask: &[i64], n: usize) -> Vec<f32> {
    let mut out = vec![0f32; HIDDEN];
    let mut cnt = 0f32;
    for t in 0..n {
        if mask[t] == 0 {
            continue;
        }
        cnt += 1.0;
        for (o, h) in out.iter_mut().zip(&hidden[t * HIDDEN..(t + 1) * HIDDEN]) {
            *o += h;
        }
    }
    let cnt = cnt.max(1.0);
    out.iter().map(|x| x / cnt).collect()
}

/// Dense — 768 → 512
fn project(w: &[f32], x: &[f32]) -> Vec<f32> {
    (0..DIM)
        .map(|o| w[o * HIDDEN..(o + 1) * HIDDEN].iter().zip(x).map(|(a, b)| a * b).sum())
        .collect()
}

/// safetensors에서 `*.weight` [512, 768] F32를 읽는다.
/// 형식: u64 머리 길이 + JSON 머리 + 텐서 바이트.
fn load_dense(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let bad = |m: &str| AiError::Other(format!("Dense 가중치({}): {m}", path.display()));
    if bytes.len() < 8 {
        return Err(bad("너무 짧다"));
    }
    let hlen = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let header: serde_json::Value =
        serde_json::from_slice(bytes.get(8..8 + hlen).ok_or_else(|| bad("머리가 잘렸다"))?)
            .map_err(|e| bad(&e.to_string()))?;
    let (_, meta) = header
        .as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.ends_with("weight")))
        .ok_or_else(|| bad("weight 텐서가 없다"))?;
    let shape: Vec<u64> = meta["shape"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    if shape != [DIM as u64, HIDDEN as u64] || meta["dtype"] != "F32" {
        return Err(bad(&format!("모양 {shape:?} / {}", meta["dtype"])));
    }
    let off: Vec<usize> = meta["data_offsets"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(|x| x as usize)).collect())
        .unwrap_or_default();
    let data = bytes
        .get(8 + hlen + off[0]..8 + hlen + off[1])
        .ok_or_else(|| bad("데이터가 잘렸다"))?;
    if data.len() != DIM * HIDDEN * 4 {
        return Err(bad("데이터 길이가 다르다"));
    }
    Ok(super::clip::from_blob(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooling_averages_only_unmasked_tokens() {
        let mut hidden = vec![0f32; 3 * HIDDEN];
        hidden[0] = 2.0; // 토큰 0
        hidden[HIDDEN] = 4.0; // 토큰 1
        hidden[2 * HIDDEN] = 100.0; // 토큰 2 — 마스크 0
        let p = pool(&hidden, &[1, 1, 0], 3);
        assert_eq!(p[0], 3.0);
    }

    #[test]
    fn projection_is_a_plain_matrix_product() {
        let mut w = vec![0f32; DIM * HIDDEN];
        w[0] = 1.0; // 출력 0 = 입력 0
        w[HIDDEN + 1] = 2.0; // 출력 1 = 2 × 입력 1
        let mut x = vec![0f32; HIDDEN];
        x[0] = 5.0;
        x[1] = 7.0;
        let y = project(&w, &x);
        assert_eq!((y[0], y[1], y[2]), (5.0, 14.0, 0.0));
    }

    #[test]
    fn reads_a_tiny_safetensors_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("dense.safetensors");
        let n = DIM * HIDDEN * 4;
        let header = format!(
            r#"{{"linear.weight":{{"dtype":"F32","shape":[{DIM},{HIDDEN}],"data_offsets":[0,{n}]}}}}"#
        );
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        let mut data = vec![0u8; n];
        data[0..4].copy_from_slice(&1.5f32.to_le_bytes());
        bytes.extend_from_slice(&data);
        std::fs::write(&p, bytes).unwrap();
        let w = load_dense(&p).unwrap();
        assert_eq!(w.len(), DIM * HIDDEN);
        assert_eq!(w[0], 1.5);
    }

    /// 실제 DB 사본에서 글로 찾아 본다 — 상위 다섯의 썸네일 경로를 찍는다 (눈으로 확인용).
    /// `ACUT_DB_COPY=… ACUT_QUERY="강아지" cargo test --release --lib ai::text::tests::real_search -- --ignored --nocapture`
    #[test]
    #[ignore = "받아 둔 텍스트 모델과 DB 사본 필요"]
    fn real_search_prints_top_hits() {
        let home = std::env::var("HOME").unwrap();
        let base = std::path::PathBuf::from(&home).join("Library/Application Support/com.acut.media");
        let (Ok(copy), Ok(t)) = (std::env::var("ACUT_DB_COPY"), Text::load(&base)) else {
            eprintln!("모델이나 사본 없음 — 건너뜀");
            return;
        };
        let db = crate::db::conn::Db::open(copy).unwrap();
        let index = super::super::similar::Index::load(&db).unwrap();
        let q = std::env::var("ACUT_QUERY").unwrap_or_else(|_| "강아지".into());
        let t0 = std::time::Instant::now();
        let v = t.embed(&q).unwrap();
        let hits = index.similar_to(&v, 5, None);
        eprintln!("\n«{q}» — {}장 중 {:.0}ms", index.len(), t0.elapsed().as_secs_f64() * 1000.0);
        for (id, score) in hits {
            let (name, thumb): (String, Option<String>) = db
                .read(|c| {
                    c.query_row(
                        "SELECT f.name, fo.library_id || '/' || t.rel_path FROM files f
                         JOIN folders fo ON fo.id = f.folder_id
                         LEFT JOIN thumbs t ON t.file_id = f.id AND t.state = 1 WHERE f.id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                })
                .unwrap();
            eprintln!("  {score:.3}  {name}  thumbs/{}", thumb.unwrap_or_default());
        }
    }

    /// 실제 모델 — 영어와 한국어가 같은 뜻이면 가깝고, 다른 뜻이면 멀다.
    /// `cargo test --release --lib ai::text::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "받아 둔 텍스트 모델 필요"]
    fn real_model_puts_korean_and_english_together() {
        let home = std::env::var("HOME").unwrap();
        let base = std::path::PathBuf::from(home).join("Library/Application Support/com.acut.media");
        let Ok(t) = Text::load(&base) else {
            eprintln!("모델 없음 — 건너뜀");
            return;
        };
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let dog_en = t.embed("a photo of a dog").unwrap();
        let dog_ko = t.embed("강아지 사진").unwrap();
        let car_ko = t.embed("자동차").unwrap();
        let same = dot(&dog_en, &dog_ko);
        let diff = dot(&dog_en, &car_ko);
        eprintln!("\n개(en)·개(ko) {same:.3}  개(en)·차(ko) {diff:.3}");
        // 글끼리의 코사인은 다 높게 나온다(0.85~0.98) — 차이가 있으면 된다.
        // 실측: 개(en)·개(ko) 0.979, 개(en)·차(ko) 0.890.
        assert!(same > diff + 0.05, "{same} vs {diff}");
    }
}
