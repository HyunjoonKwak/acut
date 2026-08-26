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

#[cfg(test)]
mod tests {
    use super::*;
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
