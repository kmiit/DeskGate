use crate::fence::Fence;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Defaults applied to every new fence the user creates from the tray
/// menu. Mirrors the per-fence Customize submenu options so changing
/// these in one place propagates to every future fence without
/// rewriting the per-fence config. Existing fences are untouched —
/// they keep whatever they had at creation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FenceDefaults {
    #[serde(default = "default_def_width")]
    pub width: f64,
    #[serde(default = "default_def_height")]
    pub height: f64,
    #[serde(default = "default_def_bg_opacity")]
    pub bg_opacity: f64,
    #[serde(default = "default_def_true")]
    pub blur_enabled: String,
    #[serde(default = "default_def_blur_radius")]
    pub blur_radius: f64,
    #[serde(default)]
    pub custom_color: Option<String>,
    #[serde(default)]
    pub fence_border_color: Option<String>,
    #[serde(default = "default_def_border_thickness")]
    pub fence_border_thickness: i32,
    #[serde(default)]
    pub title_text_color: Option<String>,
    #[serde(default)]
    pub text_color: Option<String>,
    #[serde(default = "default_def_title_size")]
    pub title_text_size: String,
    #[serde(default = "default_def_false")]
    pub bold_title_text: String,
    #[serde(default = "default_def_icon_size")]
    pub icon_size: String,
    #[serde(default = "default_def_icon_spacing")]
    pub icon_spacing: i32,
    #[serde(default = "default_def_true")]
    pub show_item_labels: String,
    #[serde(default = "default_def_false")]
    pub text_outline_enabled: String,
    #[serde(default = "default_def_title_align")]
    pub title_text_align: String,
}

fn default_def_title_align() -> String {
    "Center".into()
}

fn default_def_width() -> f64 {
    360.0
}
fn default_def_height() -> f64 {
    180.0
}
fn default_def_bg_opacity() -> f64 {
    0.45
}
fn default_def_blur_radius() -> f64 {
    20.0
}
fn default_def_border_thickness() -> i32 {
    2
}
fn default_def_title_size() -> String {
    "Medium".into()
}
fn default_def_icon_size() -> String {
    "Medium".into()
}
fn default_def_icon_spacing() -> i32 {
    5
}
fn default_def_true() -> String {
    "true".into()
}
fn default_def_false() -> String {
    "false".into()
}

impl Default for FenceDefaults {
    fn default() -> Self {
        Self {
            width: default_def_width(),
            height: default_def_height(),
            bg_opacity: default_def_bg_opacity(),
            blur_enabled: default_def_true(),
            blur_radius: default_def_blur_radius(),
            custom_color: None,
            fence_border_color: None,
            fence_border_thickness: default_def_border_thickness(),
            title_text_color: None,
            text_color: None,
            title_text_size: default_def_title_size(),
            bold_title_text: default_def_false(),
            icon_size: default_def_icon_size(),
            icon_spacing: default_def_icon_spacing(),
            show_item_labels: default_def_true(),
            text_outline_enabled: default_def_false(),
            title_text_align: default_def_title_align(),
        }
    }
}

/// App-wide preferences that aren't part of the C#-compatible fences.json
/// schema. Lives in a sibling `settings.json` so the legacy format stays
/// pristine. Add new fields with `#[serde(default = ...)]` so existing
/// profiles keep loading after the schema grows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Animation tick rate in frames per second. Controls how often the
    /// drag-reorder displacement and roll-up animations repaint while
    /// they're in flight. Higher = smoother but more GPU/CPU per drag.
    /// Range clamped to 10..=240 on use; 0 is reserved for "no animation".
    #[serde(default = "default_anim_fps")]
    pub anim_fps: i32,

    /// Template applied to fences created from the tray menu.
    #[serde(default)]
    pub fence_defaults: FenceDefaults,

    /// UI language override (`"en"`, `"zh"`). `None` = auto-detect from
    /// the Windows display language.
    #[serde(default)]
    pub language: Option<String>,
}

fn default_anim_fps() -> i32 {
    60
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            anim_fps: default_anim_fps(),
            fence_defaults: FenceDefaults::default(),
            language: None,
        }
    }
}

pub struct AppConfig {
    pub fences: Vec<Fence>,
    pub config_dir: PathBuf,
    pub settings: AppSettings,
}

impl AppConfig {
    pub fn load(profile_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let fences_path = profile_dir.join("fences.json");
        let fences: Vec<Fence> = if fences_path.exists() {
            let data = std::fs::read_to_string(&fences_path)?;
            serde_json::from_str(&data)?
        } else {
            Vec::new()
        };

        // Missing or malformed settings file → fall back to defaults
        // rather than failing the whole load; settings are non-critical.
        let settings_path = profile_dir.join("settings.json");
        let settings = if settings_path.exists() {
            std::fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            AppSettings::default()
        };

        Ok(Self {
            fences,
            config_dir: profile_dir.to_path_buf(),
            settings,
        })
    }

    pub fn save_fences(&self) -> Result<(), Box<dyn std::error::Error>> {
        let fences_path = self.config_dir.join("fences.json");
        let data = serde_json::to_string_pretty(&self.fences)?;
        std::fs::write(&fences_path, data)?;
        Ok(())
    }

    pub fn save_settings(&self) -> Result<(), Box<dyn std::error::Error>> {
        let settings_path = self.config_dir.join("settings.json");
        let data = serde_json::to_string_pretty(&self.settings)?;
        std::fs::write(&settings_path, data)?;
        Ok(())
    }

    pub fn default_profile_dir() -> PathBuf {
        if let Ok(p) = std::env::var("DESKGATE_PROFILE") {
            return PathBuf::from(p);
        }
        std::env::var("APPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join("DeskGate"))
            .unwrap_or_else(|| PathBuf::from("Profiles"))
    }
}
