use serde::{Deserialize, Serialize};

fn default_false() -> String {
    "false".into()
}
fn default_true() -> String {
    "true".into()
}
const fn default_zero() -> f64 {
    0.0
}
const fn default_bg_opacity() -> f64 {
    0.45
}
const fn default_blur_radius() -> f64 {
    20.0
}
fn default_icon_size() -> String {
    "Medium".into()
}
const fn default_icon_spacing() -> i32 {
    5
}
const fn default_border_thickness() -> i32 {
    2
}
fn default_empty_items() -> Vec<FenceItem> {
    Vec::new()
}
fn default_empty_tabs() -> Vec<Tab> {
    Vec::new()
}
const fn default_current_tab() -> i32 {
    0
}
fn default_empty_note_items() -> Vec<NoteItem> {
    Vec::new()
}
fn default_note_mode() -> String {
    "text".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fence {
    #[serde(rename = "Id")]
    pub id: String,

    #[serde(rename = "Title")]
    pub title: String,

    #[serde(rename = "X")]
    pub x: f64,

    #[serde(rename = "Y")]
    pub y: f64,

    #[serde(rename = "Width")]
    pub width: f64,

    #[serde(rename = "Height")]
    pub height: f64,

    #[serde(rename = "ItemsType")]
    pub items_type: String,

    #[serde(default = "default_empty_items", rename = "Items")]
    pub items: Vec<FenceItem>,

    #[serde(default = "default_false", rename = "IsLocked")]
    pub is_locked: String,

    #[serde(default = "default_false", rename = "IsHidden")]
    pub is_hidden: String,

    #[serde(default = "default_false", rename = "IsRolled")]
    pub is_rolled: String,

    #[serde(default = "default_zero", rename = "UnrolledHeight")]
    pub unrolled_height: f64,

    #[serde(default = "default_false", rename = "TabsEnabled")]
    pub tabs_enabled: String,

    #[serde(default = "default_current_tab", rename = "CurrentTab")]
    pub current_tab: i32,

    #[serde(default = "default_empty_tabs", rename = "Tabs")]
    pub tabs: Vec<Tab>,

    #[serde(default, rename = "CustomColor")]
    pub custom_color: Option<String>,

    #[serde(default = "default_border_thickness", rename = "FenceBorderThickness")]
    pub fence_border_thickness: i32,

    #[serde(default = "default_icon_size", rename = "IconSize")]
    pub icon_size: String,

    #[serde(default = "default_icon_spacing", rename = "IconSpacing")]
    pub icon_spacing: i32,

    #[serde(default, rename = "CustomLaunchEffect")]
    pub custom_launch_effect: Option<String>,

    #[serde(default, rename = "TextColor")]
    pub text_color: Option<String>,

    #[serde(default, rename = "TitleTextColor")]
    pub title_text_color: Option<String>,

    #[serde(default = "default_icon_size", rename = "TitleTextSize")]
    pub title_text_size: String,

    #[serde(default = "default_false", rename = "BoldTitleText")]
    pub bold_title_text: String,

    #[serde(default = "default_false", rename = "DisableTextShadow")]
    pub disable_text_shadow: String,

    #[serde(default = "default_false", rename = "GrayscaleIcons")]
    pub grayscale_icons: String,

    #[serde(default, rename = "FenceBorderColor")]
    pub fence_border_color: Option<String>,

    // Note type fence fields.
    #[serde(default, rename = "NoteContent")]
    pub note_content: Option<String>,

    #[serde(default = "default_icon_size", rename = "NoteFontSize")]
    pub note_font_size: String,

    #[serde(default, rename = "NoteFontFamily")]
    pub note_font_family: Option<String>,

    #[serde(default = "default_false", rename = "WordWrap")]
    pub word_wrap: String,

    // Note mode: "text" = plain text (uses note_content), "todo" =
    // checkable TODO list (uses note_items). Defaults to "text" so older
    // configs that only set NoteContent keep working.
    #[serde(default = "default_note_mode", rename = "NoteMode")]
    pub note_mode: String,

    // Items for the TODO-list variant of a Note fence. Each entry is a
    // single row with its own checked state. Used when note_mode == "todo".
    #[serde(default = "default_empty_note_items", rename = "NoteItems")]
    pub note_items: Vec<NoteItem>,

    // Blur / opacity (Rust extension fields).
    #[serde(default = "default_true", rename = "BlurEnabled")]
    pub blur_enabled: String,

    #[serde(default = "default_blur_radius", rename = "BlurRadius")]
    pub blur_radius: f64,

    #[serde(default = "default_bg_opacity", rename = "BackgroundOpacity")]
    pub bg_opacity: f64,

    #[serde(default = "default_true", rename = "ShowItemLabels")]
    pub show_item_labels: String,

    #[serde(default = "default_title_align", rename = "TitleTextAlign")]
    pub title_text_align: String,
}

fn default_title_align() -> String {
    "Center".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FenceItem {
    #[serde(rename = "Filename")]
    pub filename: String,

    #[serde(default, rename = "DisplayName")]
    pub display_name: String,

    #[serde(default, rename = "IsFolder")]
    pub is_folder: bool,

    #[serde(default, rename = "IsLink")]
    pub is_link: bool,

    #[serde(default, rename = "DisplayOrder")]
    pub display_order: i32,

    #[serde(default, rename = "Arguments")]
    pub arguments: Option<String>,

    // Original location of the file when it was dropped. `Some(p)` means
    // DeskGate moved the file from `p` into its private items storage so
    // the desktop entry disappears entirely (matches Stardock Fences).
    // On remove/delete we owe the user a move back to `p`. `None` = the
    // file was dropped from somewhere we don't manage (a non-desktop
    // location); we left it in place and won't touch it on cleanup.
    #[serde(default, rename = "OriginalPath")]
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    #[serde(rename = "TabName")]
    pub tab_name: String,

    #[serde(rename = "Items")]
    pub items: Vec<FenceItem>,
}

/// One row of a TODO-list-style Note fence. Plain text with a checked
/// flag; rendered as a checkbox + text by `draw_fence` and toggled by a
/// click on its checkbox column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteItem {
    #[serde(rename = "Text")]
    pub text: String,

    #[serde(default, rename = "Checked")]
    pub checked: bool,
}

impl FenceItem {
    pub fn display_name_or_filename(&self) -> &str {
        if self.display_name.is_empty() {
            std::path::Path::new(&self.filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&self.filename)
        } else {
            &self.display_name
        }
    }
}
