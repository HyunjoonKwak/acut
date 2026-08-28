//! CLIP ViT-B/32 그림 쪽 — 224×224를 넣으면 512차원 벡터가 나온다.
//!
//! 입력은 **썸네일 캐시**에서 만든다. 원본을 다시 여는 것보다 열 배 빠르고,
//! 224로 줄일 건데 640짜리 썸네일이면 충분하다.

use super::Result;
use image::imageops::FilterType;
use ort::ep::CoreML;
use ort::session::{builder::GraphOptimizationLevel, Session};
use std::path::Path;
use std::sync::Mutex;

pub const SIDE: usize = 224;
pub const DIM: usize = 512;
/// CLIP이 학습 때 쓴 평균·표준편차 (preprocessor_config.json)
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

pub struct Clip {
    session: Mutex<Session>,
    input: String,
    output: String,
}

impl Clip {
    pub fn load(model: &Path) -> Result<Self> {
        // 기본은 CPU다. CoreML(신경엔진·GPU)을 재 봤지만 이 그래프에서는 CPU
        // 8스레드가 더 빨랐다(초당 96장 대 84장). 다시 재 볼 때는
        // ACUT_CLIP_EP=coreml|strict, ACUT_CLIP_UNITS=all|ane|cpu, ACUT_CLIP_STATIC=1.
        let ep_mode = std::env::var("ACUT_CLIP_EP").unwrap_or_else(|_| "none".into());
        let units = match std::env::var("ACUT_CLIP_UNITS").as_deref() {
            Ok("ane") => ort::ep::coreml::ComputeUnits::CPUAndNeuralEngine,
            Ok("cpu") => ort::ep::coreml::ComputeUnits::CPUOnly,
            _ => ort::ep::coreml::ComputeUnits::All,
        };
        // 코어 수만큼, 여덟까지. 효율 코어까지 다 쓰면 오히려 느려진다.
        let threads: usize = std::env::var("ACUT_CLIP_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8));
        let static_shapes = std::env::var("ACUT_CLIP_STATIC").is_ok();
        let builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(threads)?;
        let mut builder = match ep_mode.as_str() {
            "none" => builder,
            "strict" => builder.with_execution_providers([CoreML::default()
                .with_compute_units(units)
                .with_static_input_shapes(static_shapes)
                .build()
                .error_on_failure()])?,
            _ => builder.with_execution_providers([CoreML::default()
                .with_compute_units(units)
                .with_static_input_shapes(static_shapes)
                .build()])?,
        };
        let session = builder.commit_from_file(model)?;
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .unwrap_or_else(|| "pixel_values".into());
        // 출력이 둘이다(image_embeds, last_hidden_state). 512짜리가 우리 것.
        let output = session
            .outputs()
            .iter()
            .find(|o| o.name() == "image_embeds")
            .or_else(|| session.outputs().first())
            .map(|o| o.name().to_string())
            .unwrap_or_else(|| "image_embeds".into());
        Ok(Self { session: Mutex::new(session), input, output })
    }

    /// 한 장을 모델 입력으로. 짧은 변을 224로 맞추고 가운데를 자른다.
    pub fn preprocess(path: &Path) -> Result<Vec<f32>> {
        let img = image::open(path)?;
        let (w, h) = (img.width(), img.height());
        let scale = SIDE as f32 / w.min(h) as f32;
        let nw = ((w as f32 * scale).round() as u32).max(SIDE as u32);
        let nh = ((h as f32 * scale).round() as u32).max(SIDE as u32);
        let resized = img.resize_exact(nw, nh, FilterType::CatmullRom);
        let x0 = (nw - SIDE as u32) / 2;
        let y0 = (nh - SIDE as u32) / 2;
        let rgb = resized.crop_imm(x0, y0, SIDE as u32, SIDE as u32).to_rgb8();
        // CHW, 채널마다 정규화
        let mut out = vec![0f32; 3 * SIDE * SIDE];
        for (i, p) in rgb.pixels().enumerate() {
            for c in 0..3 {
                out[c * SIDE * SIDE + i] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
        Ok(out)
    }

    /// 여러 장을 한 번에. 돌아오는 벡터는 길이 1로 정규화돼 있다 — 코사인이 곧 내적.
    pub fn embed(&self, batch: &[Vec<f32>]) -> Result<Vec<Vec<f32>>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let n = batch.len();
        let mut data = Vec::with_capacity(n * 3 * SIDE * SIDE);
        for b in batch {
            data.extend_from_slice(b);
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        // (모양, 데이터) 짝으로 넘긴다 — NCHW
        let input = ort::value::Tensor::from_array(([n, 3, SIDE, SIDE], data))?;
        let outputs = s.run(ort::inputs![self.input.as_str() => input])?;
        let view = outputs[self.output.as_str()].try_extract_array::<f32>()?;
        let flat: Vec<f32> = view.iter().copied().collect();
        let dim = flat.len() / n;
        Ok((0..n)
            .map(|i| normalize(&flat[i * dim..(i + 1) * dim]))
            .collect())
    }
}

/// 길이 1로. 0벡터면 그대로.
pub fn normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// f32 벡터 ↔ BLOB (리틀엔디언). DB에 이렇게 든다.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_round_trips() {
        let v = vec![0.5f32, -1.0, 3.25];
        assert_eq!(from_blob(&to_blob(&v)), v);
    }

    #[test]
    fn normalize_makes_unit_length() {
        let v = normalize(&[3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    /// 전처리 — 가로로 긴 그림도 정사각형 224가 되고 값이 정규화 범위에 든다
    #[test]
    fn preprocess_gives_224_square_chw() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.png");
        let img = image::RgbImage::from_fn(400, 200, |x, _| image::Rgb([x as u8, 128, 200]));
        img.save(&p).unwrap();
        let v = Clip::preprocess(&p).unwrap();
        assert_eq!(v.len(), 3 * SIDE * SIDE);
        assert!(v.iter().all(|x| x.is_finite() && *x > -3.0 && *x < 3.0));
    }

    /// 실제 모델이 있으면 — 벡터가 512이고 길이가 1이며, 같은 그림은 같은 벡터
    #[test]
    #[ignore = "모델 파일이 필요하다 (설정 › AI에서 받은 것)"]
    fn real_model_embeds_512() {
        let home = std::env::var("HOME").unwrap();
        let model = std::path::PathBuf::from(home)
            .join("Library/Application Support/com.acut.media/models/clip-vit-b32/vision_model.onnx");
        if !model.is_file() {
            eprintln!("모델 없음 — 건너뜀");
            return;
        }
        let clip = Clip::load(&model).unwrap();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.png");
        image::RgbImage::from_fn(300, 300, |x, y| image::Rgb([x as u8, y as u8, 90]))
            .save(&p)
            .unwrap();
        let x = Clip::preprocess(&p).unwrap();
        let t0 = std::time::Instant::now();
        let out = clip.embed(&[x.clone(), x]).unwrap();
        eprintln!("두 장 {:?}", t0.elapsed());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), DIM);
        let n: f32 = out[0].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-3, "길이 {n}");
        let dot: f32 = out[0].iter().zip(&out[1]).map(|(a, b)| a * b).sum();
        assert!(dot > 0.999, "같은 그림인데 {dot}");
    }
}
