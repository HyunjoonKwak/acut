use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: String,
    pub language: String,
    pub watch: WatchConfig,
    pub sync_presets: Vec<SyncPreset>,
    pub schedules: Vec<ScheduleConfig>,
    pub mcp_enabled: bool,
    #[serde(default)]
    pub gallery: GalleryConfig,
    #[serde(default)]
    pub scan: ScanConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryConfig {
    /// Thumbnail cell size in px (80–320)
    pub thumb_size: u32,
}

impl Default for GalleryConfig {
    fn default() -> Self {
        Self { thumb_size: 160 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    /// Directory names pruned everywhere during scans (system/junk folders)
    pub exclude_dir_names: Vec<String>,
    /// Bundle-style directory suffixes to prune (e.g. ".app")
    pub exclude_suffixes: Vec<String>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            exclude_dir_names: [
                "Library",
                "System",
                "Applications",
                "Volumes",
                "node_modules",
                "__MACOSX",
                "Backups.backupdb",
                "private",
                "usr",
                "bin",
                "sbin",
                "etc",
                "var",
                "dev",
                "opt",
                "cores",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            exclude_suffixes: [
                ".app",
                ".photoslibrary",
                ".framework",
                ".bundle",
                ".xcodeproj",
                ".xcassets",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    pub enabled: bool,
    pub debounce_ms: u64,
    pub auto_scan_on_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPreset {
    pub id: String,
    pub name: String,
    pub source_dir: String,
    pub target_dir: String,
    pub exclusion_patterns: Vec<String>,
    pub verify_checksum: bool,
    pub detect_orphans: bool,
    pub auto_eject: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub id: String,
    pub name: String,
    pub cron_expression: String,
    pub task_type: String,
    pub task_params_json: Option<String>,
    pub enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "ko".into(),
            watch: WatchConfig {
                enabled: false,
                debounce_ms: 500,
                auto_scan_on_change: true,
            },
            sync_presets: Vec::new(),
            schedules: Vec::new(),
            mcp_enabled: false,
            gallery: GalleryConfig::default(),
            scan: ScanConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(content) => serde_yaml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content =
            serde_yaml::to_string(self).map_err(|e| format!("YAML serialize error: {}", e))?;

        // Atomic write: write to temp file, then rename
        let temp_path = path.with_extension("yaml.tmp");
        fs::write(&temp_path, &content).map_err(|e| format!("Write error: {}", e))?;
        fs::rename(&temp_path, path).map_err(|e| format!("Rename error: {}", e))?;

        Ok(())
    }

    pub fn merge_update(&mut self, partial: PartialConfig) {
        if let Some(theme) = partial.theme {
            self.theme = theme;
        }
        if let Some(language) = partial.language {
            self.language = language;
        }
        if let Some(watch) = partial.watch {
            self.watch = watch;
        }
        if let Some(mcp_enabled) = partial.mcp_enabled {
            self.mcp_enabled = mcp_enabled;
        }
        if let Some(gallery) = partial.gallery {
            self.gallery = gallery;
        }
        if let Some(scan) = partial.scan {
            self.scan = scan;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialConfig {
    pub theme: Option<String>,
    pub language: Option<String>,
    pub watch: Option<WatchConfig>,
    pub mcp_enabled: Option<bool>,
    #[serde(default)]
    pub gallery: Option<GalleryConfig>,
    #[serde(default)]
    pub scan: Option<ScanConfig>,
}
