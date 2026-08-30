//! 파일 동일성 판정 — 3단계로 좁힌다.
//!
//! 6만 장을 전부 SHA-256으로 읽으면 373GB를 읽어야 한다. 대신 단계를 나눈다:
//!
//! 1. **크기** — 다르면 다른 파일이다. 대부분 여기서 걸러진다
//! 2. **빠른 해시** — 앞 64KB + 뒤 64KB + 크기. 파일당 128KB만 읽는다
//! 3. **전체 해시** — 위 둘이 같은 것만. 여기까지 오는 건 극소수다
//!
//! 3단계까지 가야 "같은 파일"이라고 단정한다. 2단계까지만 같은 것은
//! 후보일 뿐이다 — 앞뒤가 같고 가운데가 다른 파일이 실제로 존재한다.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use xxhash_rust::xxh64::Xxh64;

/// 빠른 해시가 읽는 양 (앞뒤 각각).
const PROBE: usize = 64 * 1024;

/// 앞 64KB + 뒤 64KB + 크기로 만든 지문.
///
/// 크기를 섞는 이유: 앞뒤가 같고 길이만 다른 파일(잘린 업로드 등)을 구분하기 위해서다.
pub fn quick(path: impl AsRef<Path>) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = Xxh64::new(len); // 크기를 시드로

    let mut buf = vec![0u8; PROBE.min(len as usize)];
    let n = f.read(&mut buf)?;
    h.update(&buf[..n]);

    // 파일이 충분히 크면 뒤쪽도 본다. 작으면 앞에서 이미 다 읽었다.
    if len > PROBE as u64 * 2 {
        f.seek(SeekFrom::End(-(PROBE as i64)))?;
        let n = f.read(&mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(format!("{:016x}", h.digest()))
}

/// 전체 SHA-256. 이것이 같으면 같은 파일로 본다.
pub fn full(path: impl AsRef<Path>) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    // 1MB씩 — 너무 작으면 syscall이 잦고, 너무 크면 메모리를 쓴다
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// JPEG 의 **그림 데이터만** 해시 — APPn(EXIF·XMP·ICC·미리보기)과 COM 을 뺀다.
///
/// 촬영일시를 나중에 써 넣은 사본은 바이트가 106바이트 달라 [`full`]로는 다른 파일이다
/// (실측 2026-08-30: 하와이 1,081장). 이것이 같으면 «메타데이터만 다른 사본»이다.
/// JPEG 이 아니거나 구조가 깨졌으면 `None` — 그런 파일은 이 잣대로 재지 않는다.
pub fn image(path: impl AsRef<Path>) -> std::io::Result<Option<String>> {
    let data = std::fs::read(path)?;
    Ok(jpeg_codestream_hash(&data))
}

/// 세그먼트를 걸으며 그림에 속한 것만 해시한다. 첫 SOS 부터는 EOI 까지 통째로 —
/// 스캔 데이터 안에서 FF D9 는 EOI 뿐이다 (FF 뒤엔 00 이나 RSTn 만 온다).
/// EOI 뒤에 붙은 것(삼성 SEF 꼬리 등)은 그림이 아니므로 뺀다.
fn jpeg_codestream_hash(d: &[u8]) -> Option<String> {
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        return None;
    }
    let mut h = Sha256::new();
    h.update([0xFF, 0xD8]);
    let mut i = 2;
    loop {
        // 마커 앞의 채움 바이트(FF…)는 건너뛴다
        while i < d.len() && d[i] == 0xFF {
            i += 1;
        }
        if i >= d.len() {
            return None;
        }
        let m = d[i];
        i += 1;
        match m {
            0x00 => return None, // 스캔 밖의 채움 0 — 깨진 파일
            0xD9 => {
                h.update([0xFF, 0xD9]);
                break;
            }
            0xDA => {
                let start = i - 2;
                let mut j = i;
                let end = loop {
                    if j + 1 >= d.len() {
                        break d.len();
                    }
                    if d[j] == 0xFF && d[j + 1] == 0xD9 {
                        break j + 2;
                    }
                    j += 1;
                };
                h.update(&d[start..end]);
                break;
            }
            0x01 | 0xD0..=0xD8 => h.update([0xFF, m]), // 길이 없는 마커
            _ => {
                if i + 2 > d.len() {
                    return None;
                }
                let len = u16::from_be_bytes([d[i], d[i + 1]]) as usize;
                if len < 2 || i + len > d.len() {
                    return None;
                }
                let is_meta = (0xE0..=0xEF).contains(&m) || m == 0xFE;
                if !is_meta {
                    h.update([0xFF, m]);
                    h.update(&d[i..i + len]);
                }
                i += len;
            }
        }
    }
    Some(format!("{:x}", h.finalize()))
}

/// 시험용 최소 JPEG 만들기 — dedup 시험도 쓴다
#[cfg(test)]
pub(crate) mod fixtures {
    /// 세그먼트 하나 — 마커 + 길이(자기 포함) + 내용
    pub fn seg(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    /// 최소 JPEG — SOI · (메타) · DQT · SOF · SOS+스캔 · EOI
    pub fn jpeg(meta: &[Vec<u8>], scan: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        for m in meta {
            v.extend_from_slice(m);
        }
        v.extend(seg(0xDB, &[0u8; 65]));
        v.extend(seg(0xC0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0]));
        v.extend(seg(0xDA, &[1, 1, 0, 0, 63, 0]));
        v.extend_from_slice(scan);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{jpeg, seg};
    use super::*;

    #[test]
    fn exif_and_xmp_headers_do_not_change_the_image_hash() {
        let scan = [0x12, 0x34, 0xFF, 0x00, 0x56];
        let bare = jpeg(&[seg(0xE0, b"JFIF\0")], &scan);
        let with_exif = jpeg(
            &[seg(0xE0, b"JFIF\0"), seg(0xE1, b"Exif\0\0II*\0date"), seg(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>")],
            &scan,
        );
        let with_comment = jpeg(&[seg(0xFE, b"hello")], &scan);
        assert_ne!(bare, with_exif);
        assert_eq!(jpeg_codestream_hash(&bare), jpeg_codestream_hash(&with_exif));
        assert_eq!(jpeg_codestream_hash(&bare), jpeg_codestream_hash(&with_comment));
    }

    #[test]
    fn different_pixels_or_tables_differ() {
        let a = jpeg(&[], &[1, 2, 3]);
        let b = jpeg(&[], &[1, 2, 4]);
        assert_ne!(jpeg_codestream_hash(&a), jpeg_codestream_hash(&b));
        // 양자화표가 다르면 다른 그림이다
        let mut c = a.clone();
        c[2 + 4 + 10] ^= 1;
        assert_ne!(jpeg_codestream_hash(&a), jpeg_codestream_hash(&c));
    }

    #[test]
    fn trailing_data_after_eoi_is_ignored_and_non_jpeg_is_none() {
        let a = jpeg(&[], &[9, 9]);
        let mut with_tail = a.clone();
        with_tail.extend_from_slice(b"SEFT trailer");
        assert_eq!(jpeg_codestream_hash(&a), jpeg_codestream_hash(&with_tail));
        assert_eq!(jpeg_codestream_hash(b"\x89PNG\r\n"), None);
        assert_eq!(jpeg_codestream_hash(&[0xFF, 0xD8, 0xFF]), None);
        // 길이가 파일 밖을 가리키면 깨진 파일
        let mut cut = a.clone();
        cut.truncate(10);
        assert_eq!(jpeg_codestream_hash(&cut), None);
    }

    #[test]
    fn image_reads_a_file_and_skips_non_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        std::fs::write(&p, jpeg(&[seg(0xE1, b"Exif")], &[1])).unwrap();
        let q = dir.path().join("b.png");
        std::fs::write(&q, b"\x89PNG").unwrap();
        assert!(image(&p).unwrap().is_some());
        assert_eq!(image(&q).unwrap(), None);
    }
    use std::io::Write;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn identical_files_have_identical_hashes() {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), "a", b"hello world");
        let b = write(d.path(), "b", b"hello world");
        assert_eq!(quick(&a).unwrap(), quick(&b).unwrap());
        assert_eq!(full(&a).unwrap(), full(&b).unwrap());
    }

    #[test]
    fn different_content_differs() {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), "a", b"hello world");
        let b = write(d.path(), "b", b"hello worlt");
        assert_ne!(full(&a).unwrap(), full(&b).unwrap());
    }

    #[test]
    fn same_prefix_and_suffix_but_different_middle() {
        // 빠른 해시로는 구분되지 않을 수 있는 경우.
        // 전체 해시까지 가야 하는 이유를 고정한다.
        let d = tempfile::tempdir().unwrap();
        let mut a = vec![7u8; PROBE * 3];
        let mut b = a.clone();
        a[PROBE + 100] = 1;
        b[PROBE + 100] = 2;
        let pa = write(d.path(), "a", &a);
        let pb = write(d.path(), "b", &b);

        assert_eq!(
            quick(&pa).unwrap(),
            quick(&pb).unwrap(),
            "앞뒤가 같으면 빠른 해시는 같다 — 그래서 후보일 뿐이다"
        );
        assert_ne!(full(&pa).unwrap(), full(&pb).unwrap(), "전체 해시는 구분한다");
    }

    #[test]
    fn size_is_part_of_the_quick_hash() {
        // 앞부분이 같고 길이만 다른 파일 (잘린 업로드 등)
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), "a", &vec![9u8; 1000]);
        let b = write(d.path(), "b", &vec![9u8; 2000]);
        assert_ne!(quick(&a).unwrap(), quick(&b).unwrap());
    }

    #[test]
    fn handles_tiny_and_empty_files() {
        let d = tempfile::tempdir().unwrap();
        let empty = write(d.path(), "e", b"");
        let tiny = write(d.path(), "t", b"x");
        assert!(quick(&empty).is_ok());
        assert!(full(&empty).is_ok());
        assert_ne!(quick(&empty).unwrap(), quick(&tiny).unwrap());
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(quick("/no/such/file").is_err());
        assert!(full("/no/such/file").is_err());
    }
}
