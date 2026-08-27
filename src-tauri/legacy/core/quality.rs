use std::path::Path;

#[derive(Debug, Clone)]
pub struct QualityScore {
    pub sharpness: f64,
    pub exposure: f64,
    pub total: f64,
}

const SHARPNESS_WEIGHT: f64 = 0.45;
const EXPOSURE_WEIGHT: f64 = 0.30;
const RESOLUTION_WEIGHT: f64 = 0.15;
const FILESIZE_WEIGHT: f64 = 0.10;

// Compute sharpness via Laplacian variance on downscaled grayscale image
pub fn compute_sharpness(path: &Path) -> Option<f64> {
    let img = image::open(path).ok()?;
    let resized = img.resize(800, 800, image::imageops::FilterType::Triangle);
    let gray = resized.to_luma8();
    let (w, h) = gray.dimensions();

    if w < 3 || h < 3 {
        return None;
    }

    // Laplacian kernel: [[0,1,0],[1,-4,1],[0,1,0]]
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0u64;

    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let center = gray.get_pixel(x, y).0[0] as f64;
            let top = gray.get_pixel(x, y - 1).0[0] as f64;
            let bottom = gray.get_pixel(x, y + 1).0[0] as f64;
            let left = gray.get_pixel(x - 1, y).0[0] as f64;
            let right = gray.get_pixel(x + 1, y).0[0] as f64;

            let laplacian = top + bottom + left + right - 4.0 * center;
            sum += laplacian;
            sum_sq += laplacian * laplacian;
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    Some(variance)
}

// Compute exposure quality from brightness histogram
pub fn compute_exposure(path: &Path) -> Option<f64> {
    let img = image::open(path).ok()?;
    let resized = img.resize(400, 400, image::imageops::FilterType::Triangle);
    let gray = resized.to_luma8();

    let mut histogram = [0u64; 256];
    let total_pixels = gray.pixels().count() as f64;

    for pixel in gray.pixels() {
        histogram[pixel.0[0] as usize] += 1;
    }

    // Mean brightness
    let mean: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, &count)| i as f64 * count as f64)
        .sum::<f64>()
        / total_pixels;

    // Clipping ratio (over/underexposed pixels)
    let clip_low = histogram[0..5].iter().sum::<u64>() as f64 / total_pixels;
    let clip_high = histogram[251..256].iter().sum::<u64>() as f64 / total_pixels;
    let clip_ratio = clip_low + clip_high;

    // Score: ideal mean is ~128, penalize extremes and clipping
    let mean_score = 1.0 - ((mean - 128.0) / 128.0).abs();
    let clip_penalty = (1.0 - clip_ratio * 5.0).max(0.0);

    Some((mean_score * 0.6 + clip_penalty * 0.4) * 100.0)
}

// Combined quality score for ranking within a group
pub fn compute_quality(
    path: &Path,
    width: Option<i32>,
    height: Option<i32>,
    file_size: i64,
    group_max_resolution: f64,
    group_max_filesize: i64,
) -> QualityScore {
    let sharpness_raw = compute_sharpness(path).unwrap_or(0.0);
    // Normalize sharpness to 0-100 range (typical variance range: 0-2000+)
    let sharpness = (sharpness_raw / 20.0).min(100.0);

    let exposure = compute_exposure(path).unwrap_or(50.0);

    let resolution = match (width, height) {
        (Some(w), Some(h)) if group_max_resolution > 0.0 => {
            ((w as f64 * h as f64) / group_max_resolution * 100.0).min(100.0)
        }
        _ => 50.0,
    };

    let size_score = if group_max_filesize > 0 {
        (file_size as f64 / group_max_filesize as f64 * 100.0).min(100.0)
    } else {
        50.0
    };

    let total = sharpness * SHARPNESS_WEIGHT
        + exposure * EXPOSURE_WEIGHT
        + resolution * RESOLUTION_WEIGHT
        + size_score * FILESIZE_WEIGHT;

    QualityScore {
        sharpness,
        exposure,
        total,
    }
}
