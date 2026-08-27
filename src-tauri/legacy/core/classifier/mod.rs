use std::path::Path;

use crate::core::metadata::ExifData;

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub category: String,
    pub sub_category: Option<String>,
    pub suggested_folder: String,
    pub confidence: f64,
    pub source: ClassificationSource,
}

#[derive(Debug, Clone)]
pub enum ClassificationSource {
    Exif,
    FileAnalysis,
    Rule,
    Ai,
}

pub fn classify_by_exif(exif: &ExifData, _file_name: &str) -> Option<ClassificationResult> {
    if let Some(ref date) = exif.date_taken {
        let date_part = date.split_whitespace().next().unwrap_or(date);
        let parts: Vec<&str> = date_part.split(|c| c == ':' || c == '-').collect();

        if parts.len() >= 2 {
            let year = parts[0];
            let month = parts[1];
            let month_name = match month {
                "01" => "01-Jan",
                "02" => "02-Feb",
                "03" => "03-Mar",
                "04" => "04-Apr",
                "05" => "05-May",
                "06" => "06-Jun",
                "07" => "07-Jul",
                "08" => "08-Aug",
                "09" => "09-Sep",
                "10" => "10-Oct",
                "11" => "11-Nov",
                "12" => "12-Dec",
                _ => month,
            };

            return Some(ClassificationResult {
                category: "date".to_string(),
                sub_category: Some(format!("{}/{}", year, month_name)),
                suggested_folder: format!("{}/{}", year, month_name),
                confidence: 0.9,
                source: ClassificationSource::Exif,
            });
        }
    }

    None
}

pub fn classify_by_file_analysis(
    _path: &Path,
    file_name: &str,
    file_size: u64,
    width: Option<i32>,
    height: Option<i32>,
) -> Option<ClassificationResult> {
    let name_lower = file_name.to_lowercase();

    // Screenshot detection
    if name_lower.contains("screenshot")
        || name_lower.contains("screen shot")
        || name_lower.starts_with("scr_")
    {
        return Some(ClassificationResult {
            category: "screenshot".to_string(),
            sub_category: None,
            suggested_folder: "Screenshots".to_string(),
            confidence: 0.95,
            source: ClassificationSource::FileAnalysis,
        });
    }

    // Common screen resolutions for screenshot detection
    if let (Some(w), Some(h)) = (width, height) {
        let is_screen_res = matches!(
            (w, h),
            (1170, 2532)
                | (1284, 2778)
                | (1179, 2556)
                | (1290, 2796)
                | (2560, 1440)
                | (1920, 1080)
                | (2880, 1800)
                | (3024, 1964)
        );
        if is_screen_res && name_lower.contains("img_") {
            // Likely a screenshot based on resolution
        }
    }

    // Download image detection (no EXIF, small size, web-like names)
    if file_size < 100_000 && (name_lower.contains("download") || name_lower.contains("image")) {
        return Some(ClassificationResult {
            category: "download".to_string(),
            sub_category: None,
            suggested_folder: "Downloads".to_string(),
            confidence: 0.7,
            source: ClassificationSource::FileAnalysis,
        });
    }

    None
}

pub fn classify_by_rule(
    file_name: &str,
    exif: &ExifData,
    conditions_json: &str,
    action_type: &str,
    action_value: &str,
) -> Option<ClassificationResult> {
    // Simple rule matching: parse conditions and check
    let conditions: serde_json::Value = serde_json::from_str(conditions_json).ok()?;

    let matches = if let Some(arr) = conditions.as_array() {
        arr.iter().all(|cond| {
            let field = cond.get("field").and_then(|f| f.as_str()).unwrap_or("");
            let op = cond.get("operator").and_then(|o| o.as_str()).unwrap_or("");
            let value = cond.get("value").and_then(|v| v.as_str()).unwrap_or("");

            match (field, op) {
                ("file_name", "contains") => {
                    file_name.to_lowercase().contains(&value.to_lowercase())
                }
                ("file_name", "starts_with") => {
                    file_name.to_lowercase().starts_with(&value.to_lowercase())
                }
                ("camera_model", "equals") => exif
                    .camera_model
                    .as_ref()
                    .map(|m| m.to_lowercase() == value.to_lowercase())
                    .unwrap_or(false),
                _ => false,
            }
        })
    } else {
        false
    };

    if matches {
        Some(ClassificationResult {
            category: action_type.to_string(),
            sub_category: None,
            suggested_folder: action_value.to_string(),
            confidence: 1.0,
            source: ClassificationSource::Rule,
        })
    } else {
        None
    }
}
