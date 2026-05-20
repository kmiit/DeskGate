// Shared scaffolding for the per-fence "Customize" submenu and the
// "Default fence settings" submenu in the tray. The two menus are
// structurally identical — same field list, same choice tables, same
// MF_CHECKED logic — only differing in their ID base and in the source
// the current values are read from.
//
// Encoding: every menu item carries an ID of the form
//   id_base + kind * 64 + value_index
// so a single dispatcher can recover (kind, value) by subtracting the
// base. `KIND_*` constants below number each customizable field; the
// per-kind choice tables below give the value list (label + payload).
//
// Side-effect dispatch (icon cache invalidate, set_blur, save) stays in
// the call sites because per-fence vs defaults behave differently — but
// the menu *building* and the choice tables live here.

use dg_core::config::FenceDefaults;
use dg_core::fence::Fence;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

// Per-fence kind indices. Values are positions in a 64-wide stride: any
// new kind below COUNT is safe; values within a kind are capped at 64
// (currently every choice list has at most 12 entries).
pub const KIND_BG_COLOR: usize = 0;
pub const KIND_BORDER_COLOR: usize = 1;
pub const KIND_TITLE_COLOR: usize = 2;
pub const KIND_TEXT_COLOR: usize = 3;
pub const KIND_BORDER_THICK: usize = 4;
pub const KIND_ICON_SIZE: usize = 5;
pub const KIND_ICON_SPACING: usize = 6;
pub const KIND_BOLD_TOGGLE: usize = 7;
pub const KIND_BLUR_TOGGLE: usize = 8;
pub const KIND_BG_OPACITY: usize = 9;
pub const KIND_LABELS_TOGGLE: usize = 10;
pub const KIND_COUNT: usize = 11;
pub const KIND_STRIDE: usize = 64;

#[inline]
pub fn encode(id_base: usize, kind: usize, value: usize) -> usize {
    id_base + kind * KIND_STRIDE + value
}

#[inline]
pub fn decode(id_base: usize, encoded: usize) -> (usize, usize) {
    let code = encoded - id_base;
    (code / KIND_STRIDE, code % KIND_STRIDE)
}

pub const NAMED_COLORS: &[(&str, &PCWSTR)] = &[
    ("", &w!("(default)")),
    ("Red", &w!("Red")),
    ("Green", &w!("Green")),
    ("Blue", &w!("Blue")),
    ("Teal", &w!("Teal")),
    ("Purple", &w!("Purple")),
    ("Orange", &w!("Orange")),
    ("Pink", &w!("Pink")),
    ("Yellow", &w!("Yellow")),
    ("Gray", &w!("Gray")),
    ("Black", &w!("Black")),
    ("White", &w!("White")),
];

pub const BORDER_THICKNESSES: &[(i32, &PCWSTR)] = &[
    (0, &w!("0 px")),
    (1, &w!("1 px")),
    (2, &w!("2 px")),
    (3, &w!("3 px")),
    (4, &w!("4 px")),
    (6, &w!("6 px")),
];

pub const ICON_SIZES: &[(&str, &PCWSTR)] = &[
    ("Tiny", &w!("Tiny (16)")),
    ("Small", &w!("Small (24)")),
    ("Medium", &w!("Medium (32)")),
    ("Large", &w!("Large (48)")),
    ("Huge", &w!("Huge (64)")),
];

pub const ICON_SPACINGS: &[(i32, &PCWSTR)] = &[
    (0, &w!("0 px")),
    (3, &w!("3 px")),
    (5, &w!("5 px")),
    (8, &w!("8 px")),
    (12, &w!("12 px")),
    (16, &w!("16 px")),
];

// Background opacity presets (value, label). Stored on the fence as bg_opacity.
pub const BG_OPACITIES: &[(f64, &PCWSTR)] = &[
    (0.00, &w!("0% (transparent)")),
    (0.15, &w!("15%")),
    (0.30, &w!("30%")),
    (0.45, &w!("45% (default)")),
    (0.60, &w!("60%")),
    (0.80, &w!("80%")),
    (1.00, &w!("100% (solid)")),
];

/// Minimal read-only view of either a per-fence `Fence` or the global
/// `FenceDefaults`. Constructed cheaply at menu-build time; the borrow
/// lives only for the duration of `build_customize_menu`.
pub struct CustomizeView<'a> {
    pub bg_color: &'a str,
    pub border_color: &'a str,
    pub title_color: &'a str,
    pub text_color: &'a str,
    pub border_thick: i32,
    pub icon_size: &'a str,
    pub icon_spacing: i32,
    pub bold: bool,
    pub blur_enabled: bool,
    pub bg_opacity: f64,
    pub labels: bool,
}

impl<'a> From<&'a Fence> for CustomizeView<'a> {
    fn from(f: &'a Fence) -> Self {
        Self {
            bg_color: f.custom_color.as_deref().unwrap_or(""),
            border_color: f.fence_border_color.as_deref().unwrap_or(""),
            title_color: f.title_text_color.as_deref().unwrap_or(""),
            text_color: f.text_color.as_deref().unwrap_or(""),
            border_thick: f.fence_border_thickness,
            icon_size: &f.icon_size,
            icon_spacing: f.icon_spacing,
            bold: f.bold_title_text == "true",
            blur_enabled: f.blur_enabled == "true",
            bg_opacity: f.bg_opacity,
            labels: f.show_item_labels == "true",
        }
    }
}

impl<'a> From<&'a FenceDefaults> for CustomizeView<'a> {
    fn from(d: &'a FenceDefaults) -> Self {
        Self {
            bg_color: d.custom_color.as_deref().unwrap_or(""),
            border_color: d.fence_border_color.as_deref().unwrap_or(""),
            title_color: d.title_text_color.as_deref().unwrap_or(""),
            text_color: d.text_color.as_deref().unwrap_or(""),
            border_thick: d.fence_border_thickness,
            icon_size: &d.icon_size,
            icon_spacing: d.icon_spacing,
            bold: d.bold_title_text == "true",
            blur_enabled: d.blur_enabled == "true",
            bg_opacity: d.bg_opacity,
            labels: d.show_item_labels == "true",
        }
    }
}

/// Build the (sub)menu that exposes every customizable field. Used both
/// by the per-fence right-click "Customize" submenu and the tray
/// "Default fence settings" submenu — they only differ in `id_base`
/// (which determines where the click event is routed) and in
/// `blur_radius_prompt_id` (the one-shot menu item that triggers an
/// input dialog rather than a preset list).
pub fn build_customize_menu(
    view: &CustomizeView,
    id_base: usize,
    blur_radius_prompt_id: usize,
) -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();

        append_str_color_submenu(
            menu,
            id_base,
            KIND_BG_COLOR,
            view.bg_color,
            w!("Background color"),
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_BORDER_COLOR,
            view.border_color,
            w!("Border color"),
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_TITLE_COLOR,
            view.title_color,
            w!("Title color"),
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_TEXT_COLOR,
            view.text_color,
            w!("Label color"),
        );

        append_int_submenu(
            menu,
            id_base,
            KIND_BORDER_THICK,
            BORDER_THICKNESSES,
            view.border_thick,
            w!("Border thickness"),
        );
        append_str_submenu(
            menu,
            id_base,
            KIND_ICON_SIZE,
            ICON_SIZES,
            view.icon_size,
            w!("Icon size"),
        );
        append_int_submenu(
            menu,
            id_base,
            KIND_ICON_SPACING,
            ICON_SPACINGS,
            view.icon_spacing,
            w!("Icon spacing"),
        );

        append_toggle(
            menu,
            encode(id_base, KIND_BOLD_TOGGLE, 0),
            view.bold,
            w!("Bold title"),
        );
        append_toggle(
            menu,
            encode(id_base, KIND_LABELS_TOGGLE, 0),
            view.labels,
            w!("Show item labels"),
        );
        append_toggle(
            menu,
            encode(id_base, KIND_BLUR_TOGGLE, 0),
            view.blur_enabled,
            w!("Background blur"),
        );

        let _ = AppendMenuW(menu, MF_STRING, blur_radius_prompt_id, w!("Blur radius..."));

        append_opacity_submenu(menu, id_base, view.bg_opacity);

        menu
    }
}

unsafe fn append_str_color_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    current: &str,
    label: PCWSTR,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in NAMED_COLORS.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if val.eq_ignore_ascii_case(current) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe { AppendMenuW(sub, flags, id, **item_label) };
    }
    let _ = unsafe { AppendMenuW(parent, MF_POPUP, sub.0 as usize, label) };
}

unsafe fn append_int_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    choices: &[(i32, &PCWSTR)],
    current: i32,
    label: PCWSTR,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in choices.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if *val == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe { AppendMenuW(sub, flags, id, **item_label) };
    }
    let _ = unsafe { AppendMenuW(parent, MF_POPUP, sub.0 as usize, label) };
}

unsafe fn append_str_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    choices: &[(&str, &PCWSTR)],
    current: &str,
    label: PCWSTR,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in choices.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if *val == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe { AppendMenuW(sub, flags, id, **item_label) };
    }
    let _ = unsafe { AppendMenuW(parent, MF_POPUP, sub.0 as usize, label) };
}

unsafe fn append_opacity_submenu(parent: HMENU, id_base: usize, current: f64) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in BG_OPACITIES.iter().enumerate() {
        let id = encode(id_base, KIND_BG_OPACITY, i);
        let flags = if (current - val).abs() < 0.0001 {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe { AppendMenuW(sub, flags, id, **item_label) };
    }
    let _ = unsafe { AppendMenuW(parent, MF_POPUP, sub.0 as usize, w!("Background opacity")) };
}

unsafe fn append_toggle(parent: HMENU, id: usize, on: bool, label: PCWSTR) {
    let flags = if on {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = unsafe { AppendMenuW(parent, flags, id, label) };
}

/// Resolve a color-kind value index to its stored payload ("" = inherit
/// → None; "Red" → Some("Red")). Returns None when the index is out of
/// range — caller should ignore the click.
pub fn decoded_color(value: usize) -> Option<Option<String>> {
    let v = NAMED_COLORS.get(value)?.0;
    Some(if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    })
}

pub fn decoded_border_thick(value: usize) -> Option<i32> {
    BORDER_THICKNESSES.get(value).map(|(v, _)| *v)
}

pub fn decoded_icon_size(value: usize) -> Option<String> {
    ICON_SIZES.get(value).map(|(v, _)| v.to_string())
}

pub fn decoded_icon_spacing(value: usize) -> Option<i32> {
    ICON_SPACINGS.get(value).map(|(v, _)| *v)
}

pub fn decoded_opacity(value: usize) -> Option<f64> {
    BG_OPACITIES.get(value).map(|(v, _)| *v)
}
