use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// Information about a mounted volume (typically an SD card or USB drive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub mount_point: String,
    pub device_node: String,
    pub label: String,
    pub is_removable: bool,
    pub total_space: u64,
    pub free_space: u64,
    pub file_system: String,
    pub uuid: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect currently-mounted volumes.
///
/// Implementation strategy:
/// 1. List entries under `/Volumes/`.
/// 2. For each entry run `diskutil info <mount_point>` and parse the
///    human-readable key/value output.
/// 3. Keep volumes that look removable (Removable Media, USB protocol,
///    or external physical location); with `include_internal` the boot
///    volume (`/`) and internal partitions are included as well.
pub fn detect_volumes(include_internal: bool) -> Vec<VolumeInfo> {
    let mut result: Vec<VolumeInfo> = Vec::new();

    // The boot volume itself is mounted at `/`, not under /Volumes
    if include_internal {
        if let Some(mut info) = query_diskutil_info("/") {
            if info.label.is_empty() {
                info.label = "Macintosh HD".to_string();
            }
            info.is_removable = false;
            result.push(info);
        }
    }

    let volumes_dir = Path::new("/Volumes");
    let Ok(entries) = std::fs::read_dir(volumes_dir) else {
        log::warn!("Could not read /Volumes directory");
        return result;
    };

    for entry in entries.flatten() {
        let mount_point = entry.path();
        let Some(mount_str) = mount_point.to_str() else {
            continue;
        };

        // Skip the boot volume symlinks (already covered by `/` above)
        if mount_str == "/Volumes/Macintosh HD" || mount_str == "/Volumes/Macintosh HD - Data" {
            continue;
        }

        if let Some(info) = query_diskutil_info(mount_str) {
            if info.is_removable || include_internal {
                result.push(info);
            }
        }
    }

    result
}

/// Returns `true` if `mount_point` corresponds to a removable volume.
pub fn is_removable_volume(mount_point: &str) -> bool {
    match query_diskutil_info(mount_point) {
        Some(info) => info.is_removable,
        None => false,
    }
}

/// Safely eject a volume.  The mount point must reside under `/Volumes/`.
pub fn eject_volume(mount_point: &str) -> Result<(), String> {
    // Safety check: only allow ejecting things under /Volumes/
    let path = Path::new(mount_point);
    if !path.starts_with("/Volumes/") {
        return Err(format!(
            "Refusing to eject '{}': not under /Volumes/",
            mount_point
        ));
    }

    if !path.exists() {
        return Err(format!("Volume '{}' does not exist", mount_point));
    }

    let output = Command::new("diskutil")
        .arg("eject")
        .arg(mount_point)
        .output()
        .map_err(|e| format!("Failed to run diskutil eject: {}", e))?;

    if output.status.success() {
        log::info!("Ejected volume: {}", mount_point);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("diskutil eject failed: {}", stderr.trim()))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Run `diskutil info <path>` and parse the key-value text output into a
/// `VolumeInfo`.  Returns `None` if the command fails or the output cannot
/// be meaningfully parsed.
fn query_diskutil_info(mount_point: &str) -> Option<VolumeInfo> {
    let output = Command::new("diskutil")
        .arg("info")
        .arg(mount_point)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let map = parse_diskutil_output(&text);

    let device_node = map.get("Device Node").cloned().unwrap_or_default();
    let label = map.get("Volume Name").cloned().unwrap_or_default();

    let is_removable = check_removable(&map);

    let total_space = parse_byte_value(map.get("Disk Size").map(|s| s.as_str()).unwrap_or(""));
    let free_space =
        parse_byte_value(map.get("Container Free Space").map(|s| s.as_str()).unwrap_or(""))
            .or_else(|| {
                parse_byte_value(
                    map.get("Volume Free Space").map(|s| s.as_str()).unwrap_or(""),
                )
            })
            .or_else(|| {
                parse_byte_value(
                    map.get("Volume Available Space")
                        .map(|s| s.as_str())
                        .unwrap_or(""),
                )
            });

    let file_system = map
        .get("File System Personality")
        .or_else(|| map.get("Type (Bundle)"))
        .cloned()
        .unwrap_or_default();

    let uuid = map
        .get("Volume UUID")
        .or_else(|| map.get("Disk / Partition UUID"))
        .cloned();

    Some(VolumeInfo {
        mount_point: mount_point.to_string(),
        device_node,
        label,
        is_removable,
        total_space: total_space.unwrap_or(0),
        free_space: free_space.unwrap_or(0),
        file_system,
        uuid,
    })
}

/// Parse the `diskutil info` text output into a simple key-value map.
///
/// Each non-blank line is expected to look like:
///
/// ```text
///    Volume Name:              My USB Drive
/// ```
///
/// Leading whitespace, the key, a colon, then optional whitespace before the
/// value.
fn parse_diskutil_output(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }

    map
}

/// Determine whether the volume described by `diskutil info` output should be
/// considered removable.  We use several heuristics:
///
/// - "Removable Media" is "Removable" or "Yes"
/// - "Protocol" contains "USB" or "Secure Digital"
/// - "Location" is "External"
/// - "Virtual" is "No" (avoid APFS snapshots / disk images that are virtual)
fn check_removable(map: &std::collections::HashMap<String, String>) -> bool {
    // Removable Media field
    if let Some(v) = map.get("Removable Media") {
        let v_lower = v.to_lowercase();
        if v_lower.contains("removable") || v_lower == "yes" {
            return true;
        }
    }

    // Protocol field
    if let Some(v) = map.get("Protocol") {
        let v_lower = v.to_lowercase();
        if v_lower.contains("usb") || v_lower.contains("secure digital") {
            return true;
        }
    }

    // Location field
    if let Some(v) = map.get("Location") {
        if v.to_lowercase().contains("external") {
            // Also make sure it's not a virtual disk.
            let is_virtual = map
                .get("Virtual")
                .map(|x| x.to_lowercase() == "yes")
                .unwrap_or(false);
            if !is_virtual {
                return true;
            }
        }
    }

    false
}

/// Parse a size string from `diskutil info` into bytes.
///
/// The value typically looks like one of:
///   "31.9 GB (31914983424 Bytes)"
///   "500107862016 B"
///   "31914983424"
///
/// We try to extract the parenthesized byte count first, then fall back to
/// simple numeric parsing.
fn parse_byte_value(s: &str) -> Option<u64> {
    // Try "(NNN Bytes)" pattern first.
    if let Some(start) = s.find('(') {
        if let Some(end) = s.find("Bytes)").or_else(|| s.find("bytes)")) {
            let num_str = s[start + 1..end].trim();
            if let Ok(n) = num_str.parse::<u64>() {
                return Some(n);
            }
        }
    }

    // Try plain number (possibly with trailing " B")
    let cleaned = s.trim().trim_end_matches(" B").trim_end_matches(" b");
    cleaned.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_byte_value() {
        assert_eq!(
            parse_byte_value("31.9 GB (31914983424 Bytes)"),
            Some(31914983424)
        );
        assert_eq!(parse_byte_value("500107862016"), Some(500107862016));
        assert_eq!(parse_byte_value(""), None);
    }

    #[test]
    fn test_parse_diskutil_output() {
        let sample = r#"
   Device Identifier:         disk4s1
   Device Node:               /dev/disk4s1
   Whole:                     No
   Volume Name:               SD_CARD
   Mounted:                   Yes
   Mount Point:               /Volumes/SD_CARD
   Removable Media:           Removable
   Protocol:                  USB
        "#;
        let map = parse_diskutil_output(sample);
        assert_eq!(map.get("Device Node").unwrap(), "/dev/disk4s1");
        assert_eq!(map.get("Volume Name").unwrap(), "SD_CARD");
        assert_eq!(map.get("Removable Media").unwrap(), "Removable");
        assert_eq!(map.get("Protocol").unwrap(), "USB");
    }

    #[test]
    fn test_check_removable_usb() {
        let mut map = std::collections::HashMap::new();
        map.insert("Protocol".to_string(), "USB".to_string());
        assert!(check_removable(&map));
    }

    #[test]
    fn test_check_removable_external_non_virtual() {
        let mut map = std::collections::HashMap::new();
        map.insert("Location".to_string(), "External".to_string());
        map.insert("Virtual".to_string(), "No".to_string());
        assert!(check_removable(&map));
    }

    #[test]
    fn test_check_removable_internal() {
        let mut map = std::collections::HashMap::new();
        map.insert("Location".to_string(), "Internal".to_string());
        map.insert("Removable Media".to_string(), "Fixed".to_string());
        assert!(!check_removable(&map));
    }
}
