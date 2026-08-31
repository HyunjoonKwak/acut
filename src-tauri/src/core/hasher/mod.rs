use rayon::prelude::*;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use xxhash_rust::xxh64::xxh64;

const QUICK_HASH_SIZE: usize = 4096;

pub fn quick_hash(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0u8; QUICK_HASH_SIZE];
    let bytes_read = reader.read(&mut buffer).ok()?;
    buffer.truncate(bytes_read);

    let hash = xxh64(&buffer, 0);
    Some(format!("{:016x}", hash))
}

/// Hash an entire file with xxHash64 (for sync checksum verification)
pub fn xxhash_file(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 65536];
    let mut seed_hash: u64 = 0;

    loop {
        let bytes_read = reader.read(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }
        seed_hash = xxh64(&buffer[..bytes_read], seed_hash);
    }

    Some(format!("{:016x}", seed_hash))
}

pub fn full_hash(path: &Path) -> Option<String> {
    xxhash_file(path)
}

// Simple perceptual hash implementation using average hash algorithm
// Resizes image to 8x8 grayscale, compares each pixel to the mean
pub fn compute_phash(path: &Path) -> Option<Vec<u8>> {
    let img = image::open(path).ok()?;
    let gray = img.resize_exact(8, 8, image::imageops::FilterType::Lanczos3).to_luma8();

    let pixels: Vec<u8> = gray.pixels().map(|p| p.0[0]).collect();
    let mean: f64 = pixels.iter().map(|&p| p as f64).sum::<f64>() / pixels.len() as f64;

    // Build 64-bit hash (8 bytes)
    let mut hash_bytes = vec![0u8; 8];
    for (i, &pixel) in pixels.iter().enumerate() {
        if pixel as f64 > mean {
            hash_bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }

    Some(hash_bytes)
}

pub fn hamming_distance(hash_a: &[u8], hash_b: &[u8]) -> u32 {
    hash_a
        .iter()
        .zip(hash_b.iter())
        .map(|(a, b)| (a ^ b).count_ones())
        .sum()
}

#[allow(dead_code)]
pub struct HashResult {
    pub path: String,
    pub quick_hash: Option<String>,
    pub full_hash: Option<String>,
    pub phash: Option<Vec<u8>>,
}

#[allow(dead_code)]
pub fn batch_hash(paths: &[String]) -> Vec<HashResult> {
    paths
        .par_iter()
        .map(|p| {
            let path = Path::new(p);
            HashResult {
                path: p.clone(),
                quick_hash: quick_hash(path),
                full_hash: full_hash(path),
                phash: compute_phash(path),
            }
        })
        .collect()
}

pub fn generate_thumbnail(path: &Path, max_size: u32) -> Option<String> {
    let img = image::open(path).ok()?;
    let thumbnail = img.thumbnail(max_size, max_size);

    let mut buffer = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buffer);
    thumbnail
        .write_to(&mut cursor, image::ImageFormat::Jpeg)
        .ok()?;

    use base64::Engine;
    Some(base64::engine::general_purpose::STANDARD.encode(&buffer))
}

#[cfg(test)]
mod tests {
    use super::hamming_distance;

    #[test]
    fn identical_hashes_have_zero_distance() {
        let h = vec![0xABu8; 8];
        assert_eq!(hamming_distance(&h, &h), 0);
    }

    #[test]
    fn counts_differing_bits() {
        let a = vec![0b0000_0000u8; 8];
        let mut b = a.clone();
        b[0] = 0b0000_0111; // 3 bits
        b[7] = 0b1000_0000; // 1 bit
        assert_eq!(hamming_distance(&a, &b), 4);
    }

    #[test]
    fn fully_inverted_hash_is_64() {
        let a = vec![0x00u8; 8];
        let b = vec![0xFFu8; 8];
        assert_eq!(hamming_distance(&a, &b), 64);
    }
}
