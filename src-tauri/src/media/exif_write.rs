//! JPEG 촬영일 쓰기 — 압축 화소를 건드리지 않고 EXIF/TIFF 시각을 갱신한다.

use chrono::{Local, TimeZone};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("JPEG 메타데이터를 읽을 수 없습니다")]
    Metadata,
    #[error("파일 쓰기 실패: {0}")]
    Io(#[from] std::io::Error),
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.photo-desk-{}.tmp", std::process::id()))
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

fn u16_at(bytes: &[u8], offset: usize, endian: Endian) -> Option<u16> {
    let value: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u16::from_le_bytes(value),
        Endian::Big => u16::from_be_bytes(value),
    })
}

fn u32_at(bytes: &[u8], offset: usize, endian: Endian) -> Option<u32> {
    let value: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes(value),
        Endian::Big => u32::from_be_bytes(value),
    })
}

fn ifd_entries(bytes: &[u8], tiff: usize, relative: u32, endian: Endian) -> Option<Vec<usize>> {
    let start = tiff.checked_add(relative as usize)?;
    let count = u16_at(bytes, start, endian)? as usize;
    let first = start.checked_add(2)?;
    let end = first.checked_add(count.checked_mul(12)?)?;
    (end <= bytes.len()).then(|| (0..count).map(|index| first + index * 12).collect())
}

fn tag_value(
    bytes: &[u8],
    tiff: usize,
    entries: &[usize],
    wanted: u16,
    endian: Endian,
) -> Option<(usize, usize)> {
    let entry = *entries
        .iter()
        .find(|&&entry| u16_at(bytes, entry, endian) == Some(wanted))?;
    let kind = u16_at(bytes, entry + 2, endian)?;
    let count = u32_at(bytes, entry + 4, endian)? as usize;
    if kind != 2 || count < 20 {
        return None;
    }
    let offset = if count <= 4 {
        entry + 8
    } else {
        tiff.checked_add(u32_at(bytes, entry + 8, endian)? as usize)?
    };
    (offset.checked_add(20)? <= bytes.len()).then_some((offset, count))
}

/// 이미 표준 EXIF 세 필드가 있는 JPEG는 TIFF 구조 안의 고정 길이 ASCII 값만
/// 제자리에서 바꾼다. 압축 화소·MakerNote·GPS와 segment 배치를 전혀 다시 쓰지 않는다.
/// EXIF 자체가 없는 JPEG는 `insert_new_exif`가 세 필드를 새로 만든다.
fn overwrite_existing_exif(path: &Path, date: &[u8; 20]) -> Result<bool, WriteError> {
    let mut bytes = std::fs::read(path)?;
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(WriteError::Metadata);
    }
    let mut cursor = 2_usize;
    while cursor + 4 <= bytes.len() {
        if bytes[cursor] != 0xff {
            cursor += 1;
            continue;
        }
        let marker = bytes[cursor + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x00 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            cursor += 2;
            continue;
        }
        let segment_len = u16::from_be_bytes([bytes[cursor + 2], bytes[cursor + 3]]) as usize;
        if segment_len < 2 {
            return Err(WriteError::Metadata);
        }
        let payload = cursor + 4;
        let segment_end = cursor
            .checked_add(2)
            .and_then(|value| value.checked_add(segment_len))
            .ok_or(WriteError::Metadata)?;
        if segment_end > bytes.len() {
            return Err(WriteError::Metadata);
        }
        if marker != 0xe1 || bytes.get(payload..payload + 6) != Some(b"Exif\0\0") {
            cursor = segment_end;
            continue;
        }

        let tiff = payload + 6;
        let endian = match bytes.get(tiff..tiff + 2) {
            Some(b"II") => Endian::Little,
            Some(b"MM") => Endian::Big,
            _ => return Err(WriteError::Metadata),
        };
        if u16_at(&bytes, tiff + 2, endian) != Some(42) {
            return Err(WriteError::Metadata);
        }
        let ifd0 = ifd_entries(
            &bytes,
            tiff,
            u32_at(&bytes, tiff + 4, endian).ok_or(WriteError::Metadata)?,
            endian,
        )
        .ok_or(WriteError::Metadata)?;
        let exif_pointer = ifd0
            .iter()
            .find(|&&entry| u16_at(&bytes, entry, endian) == Some(0x8769))
            .and_then(|&entry| u32_at(&bytes, entry + 8, endian))
            .ok_or(WriteError::Metadata)?;
        let exif_ifd =
            ifd_entries(&bytes, tiff, exif_pointer, endian).ok_or(WriteError::Metadata)?;
        let positions = [
            tag_value(&bytes, tiff, &ifd0, 0x0132, endian),
            tag_value(&bytes, tiff, &exif_ifd, 0x9003, endian),
            tag_value(&bytes, tiff, &exif_ifd, 0x9004, endian),
        ];
        if positions.iter().any(Option::is_none) {
            return Err(WriteError::Metadata);
        }
        for (offset, _) in positions.into_iter().flatten() {
            bytes[offset..offset + 20].copy_from_slice(date);
        }

        let temp = temp_path(path);
        let _ = std::fs::remove_file(&temp);
        let mut output = std::fs::File::create(&temp)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        std::fs::set_permissions(&temp, std::fs::metadata(path)?.permissions())?;
        std::fs::rename(&temp, path)?;
        return Ok(true);
    }
    Ok(false)
}

/// EXIF APP1이 전혀 없는 JPEG에 표준 TIFF/EXIF 세 날짜 필드를 추가한다.
/// JPEG segment만 삽입하므로 entropy-coded pixel stream은 그대로 보존된다.
fn insert_new_exif(path: &Path, date: &[u8; 20]) -> Result<(), WriteError> {
    let source = std::fs::read(path)?;
    if !source.starts_with(&[0xff, 0xd8]) {
        return Err(WriteError::Metadata);
    }

    // Big-endian TIFF: IFD0(DateTime + ExifIFD pointer), then ExifIFD의
    // DateTimeOriginal/DateTimeDigitized. 모든 문자열은 EXIF 규격의 NUL 포함 20바이트다.
    let mut tiff = Vec::with_capacity(128);
    tiff.extend_from_slice(b"MM");
    tiff.extend_from_slice(&42_u16.to_be_bytes());
    tiff.extend_from_slice(&8_u32.to_be_bytes());
    tiff.extend_from_slice(&2_u16.to_be_bytes());
    tiff.extend_from_slice(&0x0132_u16.to_be_bytes());
    tiff.extend_from_slice(&2_u16.to_be_bytes());
    tiff.extend_from_slice(&20_u32.to_be_bytes());
    tiff.extend_from_slice(&38_u32.to_be_bytes());
    tiff.extend_from_slice(&0x8769_u16.to_be_bytes());
    tiff.extend_from_slice(&4_u16.to_be_bytes());
    tiff.extend_from_slice(&1_u32.to_be_bytes());
    tiff.extend_from_slice(&58_u32.to_be_bytes());
    tiff.extend_from_slice(&0_u32.to_be_bytes());
    tiff.extend_from_slice(date);
    tiff.extend_from_slice(&2_u16.to_be_bytes());
    tiff.extend_from_slice(&0x9003_u16.to_be_bytes());
    tiff.extend_from_slice(&2_u16.to_be_bytes());
    tiff.extend_from_slice(&20_u32.to_be_bytes());
    tiff.extend_from_slice(&88_u32.to_be_bytes());
    tiff.extend_from_slice(&0x9004_u16.to_be_bytes());
    tiff.extend_from_slice(&2_u16.to_be_bytes());
    tiff.extend_from_slice(&20_u32.to_be_bytes());
    tiff.extend_from_slice(&108_u32.to_be_bytes());
    tiff.extend_from_slice(&0_u32.to_be_bytes());
    tiff.extend_from_slice(date);
    tiff.extend_from_slice(date);
    if tiff.len() != 128 {
        return Err(WriteError::Metadata);
    }

    let mut segment = Vec::with_capacity(138);
    segment.extend_from_slice(&[0xff, 0xe1]);
    segment.extend_from_slice(&136_u16.to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);

    // JFIF APP0가 있으면 그 뒤에 두고, 없으면 SOI 바로 뒤에 둔다.
    let insert_at = if source.len() >= 6 && source[2..4] == [0xff, 0xe0] {
        let len = u16::from_be_bytes([source[4], source[5]]) as usize;
        if len < 2 {
            return Err(WriteError::Metadata);
        }
        4_usize.checked_add(len).ok_or(WriteError::Metadata)?
    } else {
        2
    };
    if insert_at > source.len() {
        return Err(WriteError::Metadata);
    }

    let temp = temp_path(path);
    let _ = std::fs::remove_file(&temp);
    let mut output = std::fs::File::create(&temp)?;
    output.write_all(&source[..insert_at])?;
    output.write_all(&segment)?;
    output.write_all(&source[insert_at..])?;
    output.sync_all()?;
    std::fs::set_permissions(&temp, std::fs::metadata(path)?.permissions())?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// 세 필드(DateTimeOriginal, DateTimeDigitized, TIFF DateTime)를 같은 지역 wall-clock으로 쓴다.
pub fn write_capture_time(path: &Path, timestamp: i64) -> Result<(), WriteError> {
    let local = Local
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or(WriteError::Metadata)?;
    let formatted = local.format("%Y:%m:%d %H:%M:%S").to_string();
    if formatted.len() != 19 {
        return Err(WriteError::Metadata);
    }
    let mut date = [0_u8; 20];
    date[..19].copy_from_slice(formatted.as_bytes());
    if overwrite_existing_exif(path, &date)? {
        return Ok(());
    }
    insert_new_exif(path, &date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_all_capture_fields_without_changing_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.jpg");
        let image = image::RgbImage::from_fn(8, 8, |x, y| {
            image::Rgb([(x * 11) as u8, (y * 13) as u8, 77])
        });
        image.save(&path).unwrap();
        let before = image::open(&path).unwrap().to_rgb8();
        let wanted = crate::media::taken_at::civil_to_unix(2024, 1, 2, 23, 59, 58);

        write_capture_time(&path, wanted).unwrap();

        let after = image::open(&path).unwrap().to_rgb8();
        assert_eq!(before, after, "메타데이터 쓰기가 화소를 바꾸면 안 된다");
        assert_eq!(
            crate::media::exif::read(&path).and_then(|m| m.taken_at),
            Some(wanted)
        );
        let wanted_text = Local
            .timestamp_opt(wanted, 0)
            .single()
            .unwrap()
            .format("%Y:%m:%d %H:%M:%S\0")
            .to_string();
        assert_eq!(
            std::fs::read(&path)
                .unwrap()
                .windows(20)
                .filter(|bytes| *bytes == wanted_text.as_bytes())
                .count(),
            3,
            "최초 기록에 EXIF/TIFF 세 필드가 모두 있어야 한다"
        );

        // EXIF가 이미 있는 JPEG도 기존 값에 가려지지 않고 다시 고쳐져야 한다.
        let second = wanted + 3_600;
        write_capture_time(&path, second).unwrap();
        assert_eq!(
            crate::media::exif::read(&path).and_then(|m| m.taken_at),
            Some(second)
        );
        let second_text = Local
            .timestamp_opt(second, 0)
            .single()
            .unwrap()
            .format("%Y:%m:%d %H:%M:%S\0")
            .to_string();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            bytes
                .windows(20)
                .filter(|value| *value == second_text.as_bytes())
                .count(),
            3,
            "재교정도 EXIF/TIFF 세 필드를 모두 바꿔야 한다"
        );
        assert_eq!(
            bytes
                .windows(20)
                .filter(|value| *value == wanted_text.as_bytes())
                .count(),
            0,
            "재교정 뒤 이전 시각이 남으면 안 된다"
        );
        assert_eq!(image::open(&path).unwrap().to_rgb8(), before);
    }
}
