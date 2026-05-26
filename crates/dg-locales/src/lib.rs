pub mod en;
pub mod zh_cn;
pub mod zh_tw;

use std::sync::RwLock;

/// Convenience macro: expands to a `PCWSTR` pointing to a localized,
/// null-terminated UTF-16 string. The temporary `Vec<u16>` lives until
/// the end of the enclosing statement, so the pointer is valid for the
/// duration of any Win32 call that uses it.
///
/// ```ignore
/// AppendMenuW(menu, MF_STRING, id, tw!(TRAY_NEW_FENCE));
/// ```
#[macro_export]
macro_rules! tw {
    ($key:expr) => {{
        let _w = $crate::tw($key);
        ::windows::core::PCWSTR(_w.as_ptr())
    }};
}

// ---------------------------------------------------------------------------
// String keys
// ---------------------------------------------------------------------------

// Tray menu
pub const TRAY_NEW_FENCE: &str = "tray.new_fence";
pub const TRAY_RELOAD: &str = "tray.reload";
pub const TRAY_ANIM_FPS: &str = "tray.anim_fps";
pub const TRAY_DEFAULT_SETTINGS: &str = "tray.default_settings";
pub const TRAY_AUTOSTART: &str = "tray.autostart";
pub const TRAY_EXIT: &str = "tray.exit";
pub const TRAY_DEFAULT_BLUR_PROMPT: &str = "tray.default_blur_prompt";

// Fence context menu
pub const FENCE_OPEN: &str = "fence.open";
pub const FENCE_OPEN_LOCATION: &str = "fence.open_location";
pub const FENCE_REMOVE: &str = "fence.remove";
pub const FENCE_ROLL_UP: &str = "fence.roll_up";
pub const FENCE_UNROLL: &str = "fence.unroll";
pub const FENCE_RENAME: &str = "fence.rename";
pub const FENCE_LOCK: &str = "fence.lock";
pub const FENCE_UNLOCK: &str = "fence.unlock";
pub const FENCE_CUSTOMIZE: &str = "fence.customize";
pub const FENCE_DELETE: &str = "fence.delete";
pub const FENCE_BLUR_PROMPT: &str = "fence.blur_prompt";
pub const FENCE_RENAME_PROMPT: &str = "fence.rename_prompt";

// FPS presets
pub const FPS_OFF: &str = "fps.off";
pub const FPS_DEFAULT: &str = "fps.default";

// Customize menu labels
pub const CUSTOMIZE_BG_COLOR: &str = "customize.bg_color";
pub const CUSTOMIZE_BORDER_COLOR: &str = "customize.border_color";
pub const CUSTOMIZE_TITLE_COLOR: &str = "customize.title_color";
pub const CUSTOMIZE_LABEL_COLOR: &str = "customize.label_color";
pub const CUSTOMIZE_BORDER_THICK: &str = "customize.border_thick";
pub const CUSTOMIZE_ICON_SIZE: &str = "customize.icon_size";
pub const CUSTOMIZE_ICON_SPACING: &str = "customize.icon_spacing";
pub const CUSTOMIZE_BOLD_TITLE: &str = "customize.bold_title";
pub const CUSTOMIZE_SHOW_LABELS: &str = "customize.show_labels";
pub const CUSTOMIZE_BG_BLUR: &str = "customize.bg_blur";
pub const CUSTOMIZE_BLUR_RADIUS: &str = "customize.blur_radius";
pub const CUSTOMIZE_BG_OPACITY: &str = "customize.bg_opacity";

// Color names
pub const COLOR_DEFAULT: &str = "color.default";
pub const COLOR_RED: &str = "color.red";
pub const COLOR_GREEN: &str = "color.green";
pub const COLOR_BLUE: &str = "color.blue";
pub const COLOR_TEAL: &str = "color.teal";
pub const COLOR_PURPLE: &str = "color.purple";
pub const COLOR_ORANGE: &str = "color.orange";
pub const COLOR_PINK: &str = "color.pink";
pub const COLOR_YELLOW: &str = "color.yellow";
pub const COLOR_GRAY: &str = "color.gray";
pub const COLOR_BLACK: &str = "color.black";
pub const COLOR_WHITE: &str = "color.white";

// Size names
pub const SIZE_TINY: &str = "size.tiny";
pub const SIZE_SMALL: &str = "size.small";
pub const SIZE_MEDIUM: &str = "size.medium";
pub const SIZE_LARGE: &str = "size.large";
pub const SIZE_HUGE: &str = "size.huge";

// Opacity labels
pub const OPACITY_TRANSPARENT: &str = "opacity.transparent";
pub const OPACITY_DEFAULT: &str = "opacity.default";
pub const OPACITY_SOLID: &str = "opacity.solid";

// Modal buttons
pub const MODAL_OK: &str = "modal.ok";
pub const MODAL_CANCEL: &str = "modal.cancel";

// Delete fence dialog
pub const DELETE_TITLE: &str = "delete.title";
pub const DELETE_TITLE_NAMED: &str = "delete.title_named";
pub const DELETE_DETAILS: &str = "delete.details";
pub const DELETE_CONFIRM: &str = "delete.confirm";

// New fence default title
pub const NEW_FENCE_TITLE: &str = "new_fence_title";

// Title alignment
pub const CUSTOMIZE_TITLE_ALIGN: &str = "customize.title_align";
pub const ALIGN_LEFT: &str = "align.left";
pub const ALIGN_CENTER: &str = "align.center";
pub const ALIGN_RIGHT: &str = "align.right";

// Language menu
pub const LANG_LABEL: &str = "lang.label";
pub const LANG_EN: &str = "lang.en";
pub const LANG_ZH_CN: &str = "lang.zh_cn";
pub const LANG_ZH_TW: &str = "lang.zh_tw";

// Shell drop-description templates. Shown next to the cursor while
// dragging files over a fence. `%1` is replaced by Shell with the
// `szInsert` field of the DROPDESCRIPTION struct (NOT a Rust format
// arg) — keep the literal "%1" intact.
pub const DROP_DESC_OPEN_WITH: &str = "drop.open_with";
pub const DROP_DESC_ADD_TO: &str = "drop.add_to";

// ---------------------------------------------------------------------------
// Locale
// ---------------------------------------------------------------------------

struct Locale {
    lang: &'static str,
}

static LOCALE: RwLock<Locale> = RwLock::new(Locale { lang: "en" });

/// Initialize the global locale. Call once at startup, or again to
/// switch languages at runtime.
/// `lang` should be `"en"`, `"zh_CN"`, `"zh_TW"`, etc. Anything
/// unrecognized falls back to English.
pub fn init(lang: &str) {
    let normalized = normalize_lang(lang);
    if let Ok(mut lock) = LOCALE.write() {
        *lock = Locale { lang: normalized };
    }
}

fn normalize_lang(lang: &str) -> &'static str {
    match lang {
        "zh_CN" | "zh-CN" | "zh-Hans" | "zh" => "zh_CN",
        "zh_TW" | "zh-TW" | "zh-Hant" => "zh_TW",
        "en_US" | "en-US" | "en" => "en",
        _ => "en",
    }
}

/// Return the current language code.
pub fn lang() -> &'static str {
    match LOCALE.read() {
        Ok(lock) => lock.lang,
        Err(_) => "en",
    }
}

/// Return a translated UTF-8 string for the given key.
pub fn t(key: &'static str) -> &'static str {
    translate(lang(), key)
}

/// Return a null-terminated UTF-16 vector for Win32 wide-string APIs.
pub fn tw(key: &'static str) -> Vec<u16> {
    t(key).encode_utf16().chain(std::iter::once(0)).collect()
}

/// Build a named string like `Delete the fence "Foo"?` from a template key.
/// `{}` in the translated template is replaced with `name`.
pub fn t_named(key: &'static str, name: &str) -> String {
    translate(lang(), key).replace("{}", name)
}

/// Null-terminated UTF-16 version of [`t_named`].
pub fn tw_named(key: &'static str, name: &str) -> Vec<u16> {
    t_named(key, name)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

/// Supported languages: (code, label key).
pub fn languages() -> &'static [(&'static str, &'static str)] {
    &[
        ("en", LANG_EN),
        ("zh_CN", LANG_ZH_CN),
        ("zh_TW", LANG_ZH_TW),
    ]
}

fn translate(lang: &str, key: &'static str) -> &'static str {
    match lang {
        "zh_CN" => zh_cn::translate(key),
        "zh_TW" => zh_tw::translate(key),
        _ => en::translate(key),
    }
}
