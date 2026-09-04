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

fn ifd_entries(
    bytes: &[u8],
    tiff: usize,
    relative: u32,
    endian: Endian,
    segment_end: usize,
) -> Option<Vec<usize>> {
    let start = tiff.checked_add(relative as usize)?;
    if start < tiff || start.checked_add(2)? > segment_end {
        return None;
    }
    let count = u16_at(bytes, start, endian)? as usize;
    let first = start.checked_add(2)?;
    let end = first.checked_add(count.checked_mul(12)?)?;
    (end <= segment_end && end <= bytes.len())
        .then(|| (0..count).map(|index| first + index * 12).collect())
}

fn tag_value(
    bytes: &[u8],
    tiff: usize,
    entries: &[usize],
    wanted: u16,
    endian: Endian,
    segment_end: usize,
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
    let value_end = offset.checked_add(count)?;
    (offset >= tiff && value_end <= segment_end && value_end <= bytes.len())
        .then_some((offset, count))
}

fn encoded_u16(value: u16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encoded_u32(value: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn ifd_snapshot(
    bytes: &[u8],
    tiff: usize,
    relative: u32,
    endian: Endian,
    segment_end: usize,
) -> Option<(Vec<[u8; 12]>, u32)> {
    let positions = ifd_entries(bytes, tiff, relative, endian, segment_end)?;
    let start = tiff.checked_add(relative as usize)?;
    let next_at = start.checked_add(2 + positions.len().checked_mul(12)?)?;
    // 마지막 IFD가 segment 끝에서 정확히 끝나고 next 포인터를 생략한 파일이 있다.
    // 그때 segment 밖(다음 JPEG 마커)을 읽어 IFD1 포인터로 옮겨 적으면 안 된다.
    let next = if next_at.checked_add(4)? <= segment_end {
        u32_at(bytes, next_at, endian)?
    } else {
        0
    };
    let mut entries = Vec::with_capacity(positions.len());
    for position in positions {
        entries.push(bytes.get(position..position + 12)?.try_into().ok()?);
    }
    Some((entries, next))
}

fn ascii_entry(tag: u16, value_offset: u32, endian: Endian) -> [u8; 12] {
    let mut entry = [0_u8; 12];
    entry[0..2].copy_from_slice(&encoded_u16(tag, endian));
    entry[2..4].copy_from_slice(&encoded_u16(2, endian));
    entry[4..8].copy_from_slice(&encoded_u32(20, endian));
    entry[8..12].copy_from_slice(&encoded_u32(value_offset, endian));
    entry
}

fn long_entry(tag: u16, value: u32, endian: Endian) -> [u8; 12] {
    let mut entry = [0_u8; 12];
    entry[0..2].copy_from_slice(&encoded_u16(tag, endian));
    entry[2..4].copy_from_slice(&encoded_u16(4, endian));
    entry[4..8].copy_from_slice(&encoded_u32(1, endian));
    entry[8..12].copy_from_slice(&encoded_u32(value, endian));
    entry
}

fn push_ifd(out: &mut Vec<u8>, entries: &mut Vec<[u8; 12]>, next: u32, endian: Endian) {
    entries.sort_by_key(|entry| u16_at(entry, 0, endian).unwrap_or_default());
    out.extend_from_slice(&encoded_u16(entries.len() as u16, endian));
    for entry in entries {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(&encoded_u32(next, endian));
}

struct ExistingExif {
    segment_cursor: usize,
    segment_len: usize,
    segment_end: usize,
    tiff: usize,
    ifd0_relative: u32,
    endian: Endian,
}

/// 기존 EXIF에 날짜 태그 일부가 없거나 형식이 잘못된 경우, 기존 TIFF 데이터와
/// MakerNote/GPS/썸네일 offset은 그대로 두고 새 IFD 두 개와 날짜 문자열만 segment
/// 끝에 덧붙인다. TIFF 머리의 IFD0 포인터 하나만 새 IFD로 바꾸므로 화소와 알 수 없는
/// 제조사 메타데이터를 다시 인코딩하지 않는다.
fn append_complete_date_ifds(
    bytes: &mut Vec<u8>,
    context: ExistingExif,
    date: &[u8; 20],
) -> Result<(), WriteError> {
    let ExistingExif {
        segment_cursor,
        segment_len,
        segment_end,
        tiff,
        ifd0_relative,
        endian,
    } = context;
    let (mut ifd0, next_ifd) = ifd_snapshot(bytes, tiff, ifd0_relative, endian, segment_end)
        .ok_or(WriteError::Metadata)?;
    let exif_pointer = ifd0
        .iter()
        .find(|entry| u16_at(*entry, 0, endian) == Some(0x8769))
        .and_then(|entry| u32_at(entry, 8, endian));
    let (mut exif, next_exif) = match exif_pointer {
        Some(relative) => {
            ifd_snapshot(bytes, tiff, relative, endian, segment_end).ok_or(WriteError::Metadata)?
        }
        None => (Vec::new(), 0),
    };

    ifd0.retain(|entry| !matches!(u16_at(entry, 0, endian), Some(0x0132) | Some(0x8769)));
    exif.retain(|entry| !matches!(u16_at(entry, 0, endian), Some(0x9003) | Some(0x9004)));
    if ifd0.len() + 2 > u16::MAX as usize || exif.len() + 2 > u16::MAX as usize {
        return Err(WriteError::Metadata);
    }

    let base = u32::try_from(segment_end.checked_sub(tiff).ok_or(WriteError::Metadata)?)
        .map_err(|_| WriteError::Metadata)?;
    let ifd0_len = 2_usize
        .checked_add(
            (ifd0.len() + 2)
                .checked_mul(12)
                .ok_or(WriteError::Metadata)?,
        )
        .and_then(|value| value.checked_add(4))
        .ok_or(WriteError::Metadata)?;
    let exif_relative = base
        .checked_add(u32::try_from(ifd0_len).map_err(|_| WriteError::Metadata)?)
        .ok_or(WriteError::Metadata)?;
    let exif_len = 2_usize
        .checked_add(
            (exif.len() + 2)
                .checked_mul(12)
                .ok_or(WriteError::Metadata)?,
        )
        .and_then(|value| value.checked_add(4))
        .ok_or(WriteError::Metadata)?;
    let date0 = exif_relative
        .checked_add(u32::try_from(exif_len).map_err(|_| WriteError::Metadata)?)
        .ok_or(WriteError::Metadata)?;
    let date_original = date0.checked_add(20).ok_or(WriteError::Metadata)?;
    let date_digitized = date_original.checked_add(20).ok_or(WriteError::Metadata)?;

    ifd0.push(ascii_entry(0x0132, date0, endian));
    ifd0.push(long_entry(0x8769, exif_relative, endian));
    exif.push(ascii_entry(0x9003, date_original, endian));
    exif.push(ascii_entry(0x9004, date_digitized, endian));

    let mut appended = Vec::with_capacity(ifd0_len + exif_len + 60);
    push_ifd(&mut appended, &mut ifd0, next_ifd, endian);
    push_ifd(&mut appended, &mut exif, next_exif, endian);
    appended.extend_from_slice(date);
    appended.extend_from_slice(date);
    appended.extend_from_slice(date);

    let new_segment_len = segment_len
        .checked_add(appended.len())
        .filter(|length| *length <= u16::MAX as usize)
        .ok_or(WriteError::Metadata)?;
    bytes[tiff + 4..tiff + 8].copy_from_slice(&encoded_u32(base, endian));
    bytes[segment_cursor + 2..segment_cursor + 4]
        .copy_from_slice(&(new_segment_len as u16).to_be_bytes());
    bytes.splice(segment_end..segment_end, appended);
    Ok(())
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
        let ifd0_relative = u32_at(&bytes, tiff + 4, endian).ok_or(WriteError::Metadata)?;
        let ifd0 = ifd_entries(&bytes, tiff, ifd0_relative, endian, segment_end)
            .ok_or(WriteError::Metadata)?;
        let exif_pointer = ifd0
            .iter()
            .find(|&&entry| u16_at(&bytes, entry, endian) == Some(0x8769))
            .and_then(|&entry| u32_at(&bytes, entry + 8, endian));
        let exif_ifd = exif_pointer
            .and_then(|relative| ifd_entries(&bytes, tiff, relative, endian, segment_end))
            .unwrap_or_default();
        let positions = [
            tag_value(&bytes, tiff, &ifd0, 0x0132, endian, segment_end),
            tag_value(&bytes, tiff, &exif_ifd, 0x9003, endian, segment_end),
            tag_value(&bytes, tiff, &exif_ifd, 0x9004, endian, segment_end),
        ];
        if positions.iter().all(Option::is_some) {
            for (offset, _) in positions.into_iter().flatten() {
                bytes[offset..offset + 20].copy_from_slice(date);
            }
        } else {
            append_complete_date_ifds(
                &mut bytes,
                ExistingExif {
                    segment_cursor: cursor,
                    segment_len,
                    segment_end,
                    tiff,
                    ifd0_relative,
                    endian,
                },
                date,
            )?;
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

    #[test]
    fn repairs_a_value_offset_that_escapes_exif_without_touching_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-offset.jpg");
        image::RgbImage::from_pixel(32, 32, image::Rgb([10, 20, 30]))
            .save(&path)
            .unwrap();
        let first = crate::media::taken_at::civil_to_unix(2024, 1, 2, 3, 4, 5);
        write_capture_time(&path, first).unwrap();

        let mut bytes = std::fs::read(&path).unwrap();
        let exif = bytes
            .windows(6)
            .position(|window| window == b"Exif\0\0")
            .expect("inserted EXIF");
        let tiff = exif + 6;
        // IFD0 첫 항목(DateTime)의 값 offset을 TIFF 끝 바깥, 그러나 JPEG 파일
        // 안쪽으로 돌린다. 예전 검사는 전체 파일 길이만 봐 화소 구간을 썼다.
        let entry = tiff + 10;
        bytes[entry + 8..entry + 12].copy_from_slice(&140_u32.to_be_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let pixels = image::open(&path).unwrap().to_rgb8();

        let second = first + 60;
        write_capture_time(&path, second).unwrap();
        assert_eq!(image::open(&path).unwrap().to_rgb8(), pixels);
        assert_eq!(
            crate::media::exif::read(&path).and_then(|meta| meta.taken_at),
            Some(second)
        );
    }

    #[test]
    fn completes_a_partial_exif_without_reencoding_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.jpg");
        image::RgbImage::from_fn(32, 24, |x, y| {
            image::Rgb([(x * 5) as u8, (y * 7) as u8, 91])
        })
        .save(&path)
        .unwrap();
        let pixels = image::open(&path).unwrap().to_rgb8();
        let first = crate::media::taken_at::civil_to_unix(2024, 1, 2, 3, 4, 5);
        write_capture_time(&path, first).unwrap();

        // DateTimeDigitized 하나만 없는, 그러나 나머지 EXIF는 유효한 JPEG를 만든다.
        let mut bytes = std::fs::read(&path).unwrap();
        let exif = bytes
            .windows(6)
            .position(|window| window == b"Exif\0\0")
            .unwrap();
        let tiff = exif + 6;
        let ifd0 = ifd_entries(&bytes, tiff, 8, Endian::Big, bytes.len()).unwrap();
        let exif_pointer = ifd0
            .iter()
            .find(|&&entry| u16_at(&bytes, entry, Endian::Big) == Some(0x8769))
            .and_then(|&entry| u32_at(&bytes, entry + 8, Endian::Big))
            .unwrap();
        let exif_ifd = ifd_entries(&bytes, tiff, exif_pointer, Endian::Big, bytes.len()).unwrap();
        let digitized = *exif_ifd
            .iter()
            .find(|&&entry| u16_at(&bytes, entry, Endian::Big) == Some(0x9004))
            .unwrap();
        bytes[digitized..digitized + 2].copy_from_slice(&0xa000_u16.to_be_bytes());
        std::fs::write(&path, bytes).unwrap();

        let second = first + 3_600;
        write_capture_time(&path, second).unwrap();
        assert_eq!(image::open(&path).unwrap().to_rgb8(), pixels);
        assert_eq!(
            crate::media::exif::read(&path).and_then(|meta| meta.taken_at),
            Some(second)
        );
        let wanted = Local
            .timestamp_opt(second, 0)
            .single()
            .unwrap()
            .format("%Y:%m:%d %H:%M:%S\0")
            .to_string();
        let completed = std::fs::read(&path).unwrap();
        assert_eq!(
            completed
                .windows(20)
                .filter(|value| *value == wanted.as_bytes())
                .count(),
            3,
            "보완한 IFD가 세 촬영일 필드를 모두 가리켜야 한다"
        );
        assert_eq!(
            completed
                .windows(6)
                .filter(|window| *window == b"Exif\0\0")
                .count(),
            1,
            "중복 EXIF APP1을 만들면 읽는 앱마다 결과가 달라진다"
        );
    }

    /// next 포인터 없이 segment 끝에서 끝나는 Exif IFD — 보완하면서 segment 밖의
    /// 다음 마커 바이트를 IFD 포인터로 옮겨 적으면 안 된다.
    #[test]
    fn completing_an_ifd_that_ends_flush_with_the_segment_does_not_read_past_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flush.jpg");
        image::RgbImage::from_fn(16, 16, |x, y| image::Rgb([(x * 9) as u8, (y * 3) as u8, 45]))
            .save(&path)
            .unwrap();
        let pixels = image::open(&path).unwrap().to_rgb8();

        // IFD0(DateTime, ExifIFD→78) · 문자열 둘 · Exif IFD(DateTimeOriginal 만, next 없음)
        let date = b"2024:01:02 03:04:05\0";
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"MM\x00\x2a\x00\x00\x00\x08");
        tiff.extend_from_slice(&2_u16.to_be_bytes());
        tiff.extend_from_slice(&0x0132_u16.to_be_bytes());
        tiff.extend_from_slice(&2_u16.to_be_bytes());
        tiff.extend_from_slice(&20_u32.to_be_bytes());
        tiff.extend_from_slice(&38_u32.to_be_bytes());
        tiff.extend_from_slice(&0x8769_u16.to_be_bytes());
        tiff.extend_from_slice(&4_u16.to_be_bytes());
        tiff.extend_from_slice(&1_u32.to_be_bytes());
        tiff.extend_from_slice(&78_u32.to_be_bytes());
        tiff.extend_from_slice(&0_u32.to_be_bytes());
        tiff.extend_from_slice(date);
        tiff.extend_from_slice(date);
        tiff.extend_from_slice(&1_u16.to_be_bytes());
        tiff.extend_from_slice(&0x9003_u16.to_be_bytes());
        tiff.extend_from_slice(&2_u16.to_be_bytes());
        tiff.extend_from_slice(&20_u32.to_be_bytes());
        tiff.extend_from_slice(&58_u32.to_be_bytes());
        assert_eq!(tiff.len(), 92);
        let mut segment = vec![0xff, 0xe1];
        segment.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        segment.extend_from_slice(b"Exif\0\0");
        segment.extend_from_slice(&tiff);
        let source = std::fs::read(&path).unwrap();
        let mut bytes = source[..2].to_vec();
        bytes.extend_from_slice(&segment);
        bytes.extend_from_slice(&source[2..]);
        std::fs::write(&path, &bytes).unwrap();

        let wanted = crate::media::taken_at::civil_to_unix(2025, 6, 7, 8, 9, 10);
        write_capture_time(&path, wanted).unwrap();
        assert_eq!(image::open(&path).unwrap().to_rgb8(), pixels);
        assert_eq!(
            crate::media::exif::read(&path).and_then(|meta| meta.taken_at),
            Some(wanted)
        );

        let written = std::fs::read(&path).unwrap();
        let exif = written
            .windows(6)
            .position(|window| window == b"Exif\0\0")
            .unwrap();
        let tiff_at = exif + 6;
        let ifd0_relative = u32_at(&written, tiff_at + 4, Endian::Big).unwrap();
        let ifd0 = ifd_entries(&written, tiff_at, ifd0_relative, Endian::Big, written.len()).unwrap();
        let exif_relative = ifd0
            .iter()
            .find(|&&entry| u16_at(&written, entry, Endian::Big) == Some(0x8769))
            .and_then(|&entry| u32_at(&written, entry + 8, Endian::Big))
            .unwrap();
        for relative in [ifd0_relative, exif_relative] {
            let start = tiff_at + relative as usize;
            let count = u16_at(&written, start, Endian::Big).unwrap() as usize;
            let next = u32_at(&written, start + 2 + count * 12, Endian::Big).unwrap();
            assert_eq!(next, 0, "보완한 IFD의 next 포인터는 segment 밖 바이트가 아니라 0 이어야 한다");
        }
    }
}
