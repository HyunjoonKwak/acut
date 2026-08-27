use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ExifData {
    pub date_taken: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub focal_length: Option<f64>,
    pub aperture: Option<f64>,
    pub iso: Option<i32>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub orientation: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

pub fn extract_exif(path: &Path) -> ExifData {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return ExifData::default(),
    };

    let mut reader = BufReader::new(file);
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(&mut reader) {
        Ok(e) => e,
        Err(_) => return ExifData::default(),
    };

    let get_string = |tag: exif::Tag| -> Option<String> {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
    };

    let get_rational = |tag: exif::Tag| -> Option<f64> {
        exif.get_field(tag, exif::In::PRIMARY).and_then(|f| {
            if let exif::Value::Rational(ref v) = f.value {
                v.first().map(|r| r.num as f64 / r.denom as f64)
            } else {
                None
            }
        })
    };

    let get_uint = |tag: exif::Tag| -> Option<i32> {
        exif.get_field(tag, exif::In::PRIMARY).and_then(|f| match &f.value {
            exif::Value::Short(v) => v.first().map(|&x| x as i32),
            exif::Value::Long(v) => v.first().map(|&x| x as i32),
            _ => None,
        })
    };

    let gps_latitude = extract_gps_coord(&exif, exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef);
    let gps_longitude =
        extract_gps_coord(&exif, exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef);

    ExifData {
        date_taken: get_string(exif::Tag::DateTimeOriginal)
            .or_else(|| get_string(exif::Tag::DateTime)),
        camera_make: get_string(exif::Tag::Make),
        camera_model: get_string(exif::Tag::Model),
        lens_model: get_string(exif::Tag::LensModel),
        focal_length: get_rational(exif::Tag::FocalLength),
        aperture: get_rational(exif::Tag::FNumber),
        iso: get_uint(exif::Tag::PhotographicSensitivity),
        gps_latitude,
        gps_longitude,
        gps_altitude: get_rational(exif::Tag::GPSAltitude),
        orientation: get_uint(exif::Tag::Orientation),
        width: get_uint(exif::Tag::PixelXDimension)
            .or_else(|| get_uint(exif::Tag::ImageWidth)),
        height: get_uint(exif::Tag::PixelYDimension)
            .or_else(|| get_uint(exif::Tag::ImageLength)),
    }
}

fn extract_gps_coord(
    exif: &exif::Exif,
    coord_tag: exif::Tag,
    ref_tag: exif::Tag,
) -> Option<f64> {
    let field = exif.get_field(coord_tag, exif::In::PRIMARY)?;
    let rationals = if let exif::Value::Rational(ref v) = field.value {
        if v.len() >= 3 {
            Some(v)
        } else {
            None
        }
    } else {
        None
    }?;

    let degrees = rationals[0].num as f64 / rationals[0].denom as f64;
    let minutes = rationals[1].num as f64 / rationals[1].denom as f64;
    let seconds = rationals[2].num as f64 / rationals[2].denom as f64;
    let mut coord = degrees + minutes / 60.0 + seconds / 3600.0;

    let ref_field = exif.get_field(ref_tag, exif::In::PRIMARY)?;
    let ref_str = ref_field.display_value().to_string();
    if ref_str == "S" || ref_str == "W" {
        coord = -coord;
    }

    Some(coord)
}

pub fn get_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    image::image_dimensions(path).ok()
}
