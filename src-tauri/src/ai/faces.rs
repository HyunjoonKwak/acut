//! 얼굴 — 찾기(YuNet)와 알아보기(SFace). 둘 다 OpenCV zoo, Apache-2.0.
//!
//! YuNet은 640×640 고정 입력에 stride 8·16·32 격자마다 (있나, 얼굴인가,
//! 상자, 눈·코·입 다섯 점)을 낸다. 썸네일을 비율대로 줄여 왼쪽 위에 놓고
//! 나머지는 검게 둔 뒤, 결과를 다시 원래 자리로 돌린다.
//!
//! SFace는 다섯 점을 기준 자리에 맞춘 112×112 얼굴을 받아 128개 숫자로
//! 요약한다. 같은 사람이면 코사인이 높다 (OpenCV 문서: 0.363 위면 같은 사람).
//!
//! 기준 자리 다섯 점과 «다섯 점 → 기준» 유사변환은 OpenCV의 것을 그대로
//! 따른다 — 모델이 그렇게 배웠다.

use super::{AiError, Result};
use image::{imageops, RgbImage};
use ort::session::{builder::GraphOptimizationLevel, Session};
use std::path::Path;
use std::sync::Mutex;

pub const DET_SIDE: u32 = 640;
pub const ALIGN_SIDE: u32 = 112;
pub const EMB_DIM: usize = 128;
/// 이보다 확신이 낮은 검출은 버린다 (OpenCV 기본 0.9, 썸네일은 작아서 조금 낮춘다)
pub const SCORE_MIN: f32 = 0.8;
const NMS_IOU: f32 = 0.3;
const STRIDES: [usize; 3] = [8, 16, 32];

/// 찾은 얼굴 하나 — 좌표는 넣은 그림의 픽셀
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    /// 오른눈·왼눈·코·입 오른쪽·입 왼쪽 (x, y)
    pub kps: [[f32; 2]; 5],
}

/// SFace가 배운 기준 자리 (112×112)
const REF: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

fn session(path: &Path, threads: usize) -> Result<Session> {
    Ok(Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads)?
        .commit_from_file(path)?)
}

pub struct Detector {
    session: Mutex<Session>,
}

impl Detector {
    pub fn load(path: &Path, threads: usize) -> Result<Self> {
        Ok(Detector {
            session: Mutex::new(session(path, threads)?),
        })
    }

    pub fn detect(&self, img: &RgbImage) -> Result<Vec<Face>> {
        let (canvas, scale) = letterbox(img);
        // BGR, CHW, 0~255 그대로 — OpenCV의 blobFromImage 기본값
        let side = DET_SIDE as usize;
        let mut data = vec![0f32; 3 * side * side];
        for (i, p) in canvas.pixels().enumerate() {
            data[i] = p[2] as f32;
            data[side * side + i] = p[1] as f32;
            data[2 * side * side + i] = p[0] as f32;
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let input = ort::value::Tensor::from_array(([1, 3, side, side], data))?;
        let outputs = s.run(ort::inputs!["input" => input])?;
        let grab = |name: &str| -> Result<Vec<f32>> {
            Ok(outputs[name]
                .try_extract_array::<f32>()?
                .iter()
                .copied()
                .collect())
        };
        let mut all = Vec::new();
        for stride in STRIDES {
            let cls = grab(&format!("cls_{stride}"))?;
            let obj = grab(&format!("obj_{stride}"))?;
            let bbox = grab(&format!("bbox_{stride}"))?;
            let kps = grab(&format!("kps_{stride}"))?;
            all.extend(decode(
                stride,
                side / stride,
                &cls,
                &obj,
                &bbox,
                &kps,
                SCORE_MIN,
            ));
        }
        let mut kept = nms(all, NMS_IOU);
        for f in &mut kept {
            f.x /= scale;
            f.y /= scale;
            f.w /= scale;
            f.h /= scale;
            for p in &mut f.kps {
                p[0] /= scale;
                p[1] /= scale;
            }
        }
        Ok(kept)
    }
}

/// 비율대로 줄여 640×640 검은 판 왼쪽 위에 놓는다. 돌아오는 값은 줄인 비율.
fn letterbox(img: &RgbImage) -> (RgbImage, f32) {
    let (w, h) = img.dimensions();
    let scale = DET_SIDE as f32 / w.max(h) as f32;
    let nw = ((w as f32 * scale).round() as u32).clamp(1, DET_SIDE);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, DET_SIDE);
    let small = imageops::resize(img, nw, nh, imageops::FilterType::Triangle);
    let mut canvas = RgbImage::new(DET_SIDE, DET_SIDE);
    imageops::replace(&mut canvas, &small, 0, 0);
    (canvas, scale)
}

/// 격자 한 단계의 출력을 얼굴 목록으로. `cols`는 격자 한 변의 칸 수.
fn decode(
    stride: usize,
    cols: usize,
    cls: &[f32],
    obj: &[f32],
    bbox: &[f32],
    kps: &[f32],
    min_score: f32,
) -> Vec<Face> {
    let n = cls.len().min(obj.len());
    let mut out = Vec::new();
    for i in 0..n {
        let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
        if score < min_score {
            continue;
        }
        let (row, col) = ((i / cols) as f32, (i % cols) as f32);
        let s = stride as f32;
        let b = &bbox[i * 4..i * 4 + 4];
        let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
        let (w, h) = (b[2].exp() * s, b[3].exp() * s);
        let k = &kps[i * 10..i * 10 + 10];
        let mut pts = [[0f32; 2]; 5];
        for (j, p) in pts.iter_mut().enumerate() {
            *p = [(col + k[j * 2]) * s, (row + k[j * 2 + 1]) * s];
        }
        out.push(Face {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
            score,
            kps: pts,
        });
    }
    out
}

fn iou(a: &Face, b: &Face) -> f32 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.w).min(b.x + b.w);
    let y2 = (a.y + a.h).min(b.y + b.h);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = a.w * a.h + b.w * b.h - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// 겹치는 상자 가운데 확신이 높은 것만 남긴다
fn nms(mut faces: Vec<Face>, max_iou: f32) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for f in faces {
        if kept.iter().all(|k| iou(k, &f) <= max_iou) {
            kept.push(f);
        }
    }
    kept
}

/// 다섯 점을 기준 자리로 보내는 유사변환 [a, -b, tx; b, a, ty] — 최소제곱.
/// x' = a·x − b·y + tx, y' = b·x + a·y + ty
pub fn similarity(from: &[[f32; 2]; 5], to: &[[f32; 2]; 5]) -> [f32; 4] {
    // 정규방정식 4×4: 미지수 (a, b, tx, ty)
    let mut m = [[0f64; 4]; 4];
    let mut v = [0f64; 4];
    for (p, q) in from.iter().zip(to) {
        let (x, y) = (p[0] as f64, p[1] as f64);
        let (u, w) = (q[0] as f64, q[1] as f64);
        // 행 1: a·x − b·y + tx = u
        let r1 = [x, -y, 1.0, 0.0];
        // 행 2: b·x + a·y + ty = w
        let r2 = [y, x, 0.0, 1.0];
        for (r, t) in [(r1, u), (r2, w)] {
            for i in 0..4 {
                for j in 0..4 {
                    m[i][j] += r[i] * r[j];
                }
                v[i] += r[i] * t;
            }
        }
    }
    // 가우스 소거
    for c in 0..4 {
        let pivot = (c..4)
            .max_by(|&i, &j| m[i][c].abs().total_cmp(&m[j][c].abs()))
            .unwrap();
        m.swap(c, pivot);
        v.swap(c, pivot);
        let d = m[c][c];
        if d.abs() < 1e-12 {
            return [1.0, 0.0, 0.0, 0.0];
        }
        let pivot_row = m[c];
        for r in 0..4 {
            if r == c {
                continue;
            }
            let f = m[r][c] / d;
            for (value, pivot_value) in m[r].iter_mut().zip(pivot_row) {
                *value -= f * pivot_value;
            }
            v[r] -= f * v[c];
        }
    }
    [
        (v[0] / m[0][0]) as f32,
        (v[1] / m[1][1]) as f32,
        (v[2] / m[2][2]) as f32,
        (v[3] / m[3][3]) as f32,
    ]
}

/// 얼굴을 기준 자리에 맞춘 112×112. 바깥은 검다.
pub fn align(img: &RgbImage, kps: &[[f32; 2]; 5]) -> RgbImage {
    let [a, b, tx, ty] = similarity(kps, &REF);
    let det = a * a + b * b;
    let mut out = RgbImage::new(ALIGN_SIDE, ALIGN_SIDE);
    if det < 1e-9 {
        return out;
    }
    let (w, h) = (img.width() as f32, img.height() as f32);
    for (u, v, px) in out.enumerate_pixels_mut() {
        // 거꾸로: 결과 자리 (u, v) → 원본 자리
        let (du, dv) = (u as f32 - tx, v as f32 - ty);
        let sx = (a * du + b * dv) / det;
        let sy = (-b * du + a * dv) / det;
        if sx < 0.0 || sy < 0.0 || sx >= w - 1.0 || sy >= h - 1.0 {
            continue;
        }
        *px = bilinear(img, sx, sy);
    }
    out
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> image::Rgb<u8> {
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |dx: u32, dy: u32| img.get_pixel(x0 + dx, y0 + dy).0;
    let (p00, p10, p01, p11) = (p(0, 0), p(1, 0), p(0, 1), p(1, 1));
    let mut o = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
        let bot = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
        o[c] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgb(o)
}

/// 모델에 픽셀을 어떻게 넣나 — 실측으로 고른다 (아래 real 테스트)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Norm {
    Rgb,
    Bgr,
    RgbCentered,
    BgrCentered,
}

pub struct Recognizer {
    session: Mutex<Session>,
    norm: Norm,
}

impl Recognizer {
    pub fn load(path: &Path, threads: usize) -> Result<Self> {
        Self::load_with(path, threads, Norm::Rgb)
    }

    pub fn load_with(path: &Path, threads: usize, norm: Norm) -> Result<Self> {
        Ok(Recognizer {
            session: Mutex::new(session(path, threads)?),
            norm,
        })
    }

    /// 정렬된 112×112 얼굴 → 길이 1 벡터 (128)
    pub fn embed(&self, aligned: &RgbImage) -> Result<Vec<f32>> {
        if aligned.dimensions() != (ALIGN_SIDE, ALIGN_SIDE) {
            return Err(AiError::Other("정렬된 얼굴이 112×112가 아닙니다".into()));
        }
        let n = (ALIGN_SIDE * ALIGN_SIDE) as usize;
        let mut data = vec![0f32; 3 * n];
        let (swap, center) = match self.norm {
            Norm::Rgb => (false, false),
            Norm::Bgr => (true, false),
            Norm::RgbCentered => (false, true),
            Norm::BgrCentered => (true, true),
        };
        let f = |v: u8| {
            if center {
                (v as f32 - 127.5) / 128.0
            } else {
                v as f32
            }
        };
        for (i, p) in aligned.pixels().enumerate() {
            let (r, g, b) = if swap {
                (p[2], p[1], p[0])
            } else {
                (p[0], p[1], p[2])
            };
            data[i] = f(r);
            data[n + i] = f(g);
            data[2 * n + i] = f(b);
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let input = ort::value::Tensor::from_array((
            [1, 3, ALIGN_SIDE as usize, ALIGN_SIDE as usize],
            data,
        ))?;
        let outputs = s.run(ort::inputs!["data" => input])?;
        let v: Vec<f32> = outputs[0]
            .try_extract_array::<f32>()?
            .iter()
            .copied()
            .collect();
        if v.len() != EMB_DIM {
            return Err(AiError::Other(format!("얼굴 벡터 길이가 {}", v.len())));
        }
        Ok(super::clip::normalize(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_puts_the_box_on_the_grid_cell() {
        // stride 8, 격자 4×4, 칸 5 (row 1, col 1)에 얼굴 하나
        let n = 16;
        let mut cls = vec![0f32; n];
        let mut obj = vec![0f32; n];
        let mut bbox = vec![0f32; n * 4];
        let kps = vec![0f32; n * 10];
        cls[5] = 0.81;
        obj[5] = 1.0;
        bbox[5 * 4..5 * 4 + 4].copy_from_slice(&[0.5, 0.5, 0.0, 0.0]); // 중심 칸 가운데, 크기 e^0 = 1 × stride
        let f = decode(8, 4, &cls, &obj, &bbox, &kps, 0.5);
        assert_eq!(f.len(), 1);
        let f = &f[0];
        assert!((f.score - 0.9).abs() < 1e-4);
        assert_eq!((f.w, f.h), (8.0, 8.0));
        assert_eq!((f.x, f.y), (12.0 - 4.0, 12.0 - 4.0));
        assert_eq!(f.kps[0], [8.0, 8.0]);
    }

    #[test]
    fn decode_drops_low_scores() {
        let f = decode(8, 2, &[0.3, 0.9], &[0.3, 0.9], &[0.0; 8], &[0.0; 20], 0.8);
        assert_eq!(f.len(), 1);
    }

    fn face(x: f32, y: f32, w: f32, h: f32, score: f32) -> Face {
        Face {
            x,
            y,
            w,
            h,
            score,
            kps: [[0.0; 2]; 5],
        }
    }

    #[test]
    fn nms_keeps_the_stronger_of_overlapping_boxes() {
        let kept = nms(
            vec![
                face(0.0, 0.0, 10.0, 10.0, 0.9),
                face(1.0, 1.0, 10.0, 10.0, 0.95),
                face(50.0, 50.0, 10.0, 10.0, 0.85),
            ],
            0.3,
        );
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.95);
        assert_eq!(kept[1].score, 0.85);
    }

    #[test]
    fn letterbox_scales_the_long_side_to_640() {
        let img = RgbImage::new(320, 240);
        let (c, s) = letterbox(&img);
        assert_eq!(c.dimensions(), (640, 640));
        assert_eq!(s, 2.0);
    }

    #[test]
    fn similarity_recovers_a_known_transform() {
        // 2배 키우고 90° 돌리고 (10, 20) 옮긴다: a=0, b=2
        let from = [[1.0, 0.0], [0.0, 1.0], [2.0, 3.0], [-1.0, 4.0], [5.0, -2.0]];
        let mut to = [[0f32; 2]; 5];
        for (p, q) in from.iter().zip(to.iter_mut()) {
            *q = [-2.0 * p[1] + 10.0, 2.0 * p[0] + 20.0];
        }
        let [a, b, tx, ty] = similarity(&from, &to);
        assert!((a - 0.0).abs() < 1e-4 && (b - 2.0).abs() < 1e-4, "{a} {b}");
        assert!((tx - 10.0).abs() < 1e-3 && (ty - 20.0).abs() < 1e-3);
    }

    #[test]
    fn align_of_reference_points_is_identity() {
        // 기준 자리 그대로면 그림이 그대로 온다
        let mut img = RgbImage::new(112, 112);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 2) as u8, (y * 2) as u8, 7]);
        }
        let out = align(&img, &REF);
        let (x, y) = (60u32, 40u32);
        let (a, b) = (img.get_pixel(x, y).0, out.get_pixel(x, y).0);
        assert!(
            (a[0] as i32 - b[0] as i32).abs() <= 1 && (a[1] as i32 - b[1] as i32).abs() <= 1,
            "{a:?} {b:?}"
        );
    }

    /// 실제 썸네일 몇 장에서 얼굴을 찾아 정렬된 얼굴을 파일로 남긴다 (눈으로 확인용).
    /// `ACUT_FACE_DIR=<yunet.onnx·sface.onnx 있는 곳> ACUT_THUMBS=a.jpg,b.jpg ACUT_OUT=/tmp/x cargo test --release --lib ai::faces::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "모델과 썸네일 필요"]
    fn real_faces_from_thumbnails() {
        let (Ok(dir), Ok(thumbs), Ok(out)) = (
            std::env::var("ACUT_FACE_DIR"),
            std::env::var("ACUT_THUMBS"),
            std::env::var("ACUT_OUT"),
        ) else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let det = Detector::load(&dir.join("yunet.onnx"), 4).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let mut crops: Vec<(String, RgbImage)> = Vec::new();
        for (n, p) in thumbs.split(',').enumerate() {
            let img = image::open(p).unwrap().to_rgb8();
            let t = std::time::Instant::now();
            let faces = det.detect(&img).unwrap();
            eprintln!(
                "\n{p}: {}×{} 얼굴 {}개 {:.0}ms",
                img.width(),
                img.height(),
                faces.len(),
                t.elapsed().as_secs_f64() * 1000.0
            );
            for (i, f) in faces.iter().enumerate() {
                if f.w < 24.0 {
                    continue;
                }
                let aligned = align(&img, &f.kps);
                let path = format!("{out}/{n}_{i}.png");
                aligned.save(&path).unwrap();
                eprintln!(
                    "  #{i} score {:.2} {}×{} at ({:.0},{:.0}) → {path}",
                    f.score, f.w as i32, f.h as i32, f.x, f.y
                );
                crops.push((format!("{n}_{i}"), aligned));
            }
        }
        for norm in [Norm::Rgb, Norm::Bgr, Norm::RgbCentered, Norm::BgrCentered] {
            let rec = Recognizer::load_with(&dir.join("sface.onnx"), 4, norm).unwrap();
            let embs: Vec<Vec<f32>> = crops.iter().map(|(_, c)| rec.embed(c).unwrap()).collect();
            eprint!(
                "\n[{norm:?}] 코사인 행렬 (행/열 = {})\n      ",
                crops
                    .iter()
                    .map(|c| c.0.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            for i in 0..embs.len() {
                eprint!("\n  {:>4} ", crops[i].0);
                for j in 0..embs.len() {
                    let d: f32 = embs[i].iter().zip(&embs[j]).map(|(a, b)| a * b).sum();
                    eprint!("{d:5.2} ");
                }
            }
            eprintln!();
        }
    }
}
