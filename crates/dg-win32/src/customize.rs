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
use dg_locales as loc;
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
pub const KIND_TITLE_ALIGN: usize = 11;
pub const KIND_NOTE_ALIGN: usize = 12;
pub const KIND_TEXT_OUTLINE_TOGGLE: usize = 13;
pub const KIND_COUNT: usize = 14;
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

pub const NAMED_COLORS: &[(&str, &str)] = &[
    ("", "color.default"),
    ("Red", "color.red"),
    ("Green", "color.green"),
    ("Blue", "color.blue"),
    ("Teal", "color.teal"),
    ("Purple", "color.purple"),
    ("Orange", "color.orange"),
    ("Pink", "color.pink"),
    ("Yellow", "color.yellow"),
    ("Gray", "color.gray"),
    ("Black", "color.black"),
    ("White", "color.white"),
];

pub const BORDER_THICKNESSES: &[(i32, &str)] = &[
    (0, "0 px"),
    (1, "1 px"),
    (2, "2 px"),
    (3, "3 px"),
    (4, "4 px"),
    (6, "6 px"),
];

pub const ICON_SIZES: &[(&str, &str)] = &[
    ("Tiny", "size.tiny"),
    ("Small", "size.small"),
    ("Medium", "size.medium"),
    ("Large", "size.large"),
    ("Huge", "size.huge"),
];

pub const ICON_SPACINGS: &[(i32, &str)] = &[
    (0, "0 px"),
    (3, "3 px"),
    (5, "5 px"),
    (8, "8 px"),
    (12, "12 px"),
    (16, "16 px"),
];

pub const TITLE_ALIGNS: &[(&str, &str)] = &[
    ("Left", "align.left"),
    ("Center", "align.center"),
    ("Right", "align.right"),
];

pub const BG_OPACITIES: &[(f64, &str)] = &[
    (0.00, "opacity.transparent"),
    (0.15, "15%"),
    (0.30, "30%"),
    (0.45, "opacity.default"),
    (0.60, "60%"),
    (0.80, "80%"),
    (1.00, "opacity.solid"),
];

/// Resolve a label string — if it matches a known locale key, translate it;
/// otherwise return it as-is (for literal labels like "15%").
fn resolve_label(label: &'static str) -> &'static str {
    match label {
        "color.default" => loc::t(loc::COLOR_DEFAULT),
        "color.red" => loc::t(loc::COLOR_RED),
        "color.green" => loc::t(loc::COLOR_GREEN),
        "color.blue" => loc::t(loc::COLOR_BLUE),
        "color.teal" => loc::t(loc::COLOR_TEAL),
        "color.purple" => loc::t(loc::COLOR_PURPLE),
        "color.orange" => loc::t(loc::COLOR_ORANGE),
        "color.pink" => loc::t(loc::COLOR_PINK),
        "color.yellow" => loc::t(loc::COLOR_YELLOW),
        "color.gray" => loc::t(loc::COLOR_GRAY),
        "color.black" => loc::t(loc::COLOR_BLACK),
        "color.white" => loc::t(loc::COLOR_WHITE),
        "size.tiny" => loc::t(loc::SIZE_TINY),
        "size.small" => loc::t(loc::SIZE_SMALL),
        "size.medium" => loc::t(loc::SIZE_MEDIUM),
        "size.large" => loc::t(loc::SIZE_LARGE),
        "size.huge" => loc::t(loc::SIZE_HUGE),
        "opacity.transparent" => loc::t(loc::OPACITY_TRANSPARENT),
        "opacity.default" => loc::t(loc::OPACITY_DEFAULT),
        "opacity.solid" => loc::t(loc::OPACITY_SOLID),
        "align.left" => loc::t(loc::ALIGN_LEFT),
        "align.center" => loc::t(loc::ALIGN_CENTER),
        "align.right" => loc::t(loc::ALIGN_RIGHT),
        other => other,
    }
}

fn label_with_current(label_key: &'static str, current_label: &str) -> String {
    format!("{}: {}", loc::t(label_key), current_label)
}

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
    pub text_outline: bool,
    pub title_align: &'a str,
    // Some(align) when the underlying fence is a Note type so the
    // alignment submenu should appear. None for shortcut fences and the
    // global FenceDefaults (which doesn't carry a note alignment).
    pub note_align: Option<&'a str>,
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
            text_outline: f.text_outline_enabled == "true",
            title_align: &f.title_text_align,
            note_align: if f.items_type == "Note" {
                Some(&f.note_text_align)
            } else {
                None
            },
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
            text_outline: d.text_outline_enabled == "true",
            title_align: &d.title_text_align,
            note_align: None,
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
            loc::CUSTOMIZE_BG_COLOR,
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_BORDER_COLOR,
            view.border_color,
            loc::CUSTOMIZE_BORDER_COLOR,
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_TITLE_COLOR,
            view.title_color,
            loc::CUSTOMIZE_TITLE_COLOR,
        );
        append_str_color_submenu(
            menu,
            id_base,
            KIND_TEXT_COLOR,
            view.text_color,
            loc::CUSTOMIZE_LABEL_COLOR,
        );

        append_int_submenu(
            menu,
            id_base,
            KIND_BORDER_THICK,
            BORDER_THICKNESSES,
            view.border_thick,
            loc::CUSTOMIZE_BORDER_THICK,
        );
        append_str_submenu(
            menu,
            id_base,
            KIND_ICON_SIZE,
            ICON_SIZES,
            view.icon_size,
            loc::CUSTOMIZE_ICON_SIZE,
        );
        append_int_submenu(
            menu,
            id_base,
            KIND_ICON_SPACING,
            ICON_SPACINGS,
            view.icon_spacing,
            loc::CUSTOMIZE_ICON_SPACING,
        );

        append_toggle(
            menu,
            encode(id_base, KIND_BOLD_TOGGLE, 0),
            view.bold,
            loc::CUSTOMIZE_BOLD_TITLE,
        );
        append_str_submenu(
            menu,
            id_base,
            KIND_TITLE_ALIGN,
            TITLE_ALIGNS,
            view.title_align,
            loc::CUSTOMIZE_TITLE_ALIGN,
        );
        if let Some(note_align) = view.note_align {
            append_str_submenu(
                menu,
                id_base,
                KIND_NOTE_ALIGN,
                TITLE_ALIGNS,
                note_align,
                loc::CUSTOMIZE_NOTE_ALIGN,
            );
        }
        append_toggle(
            menu,
            encode(id_base, KIND_LABELS_TOGGLE, 0),
            view.labels,
            loc::CUSTOMIZE_SHOW_LABELS,
        );
        append_toggle(
            menu,
            encode(id_base, KIND_TEXT_OUTLINE_TOGGLE, 0),
            view.text_outline,
            loc::CUSTOMIZE_TEXT_OUTLINE,
        );
        append_toggle(
            menu,
            encode(id_base, KIND_BLUR_TOGGLE, 0),
            view.blur_enabled,
            loc::CUSTOMIZE_BG_BLUR,
        );

        append_menu_key(
            menu,
            MF_STRING,
            blur_radius_prompt_id,
            loc::CUSTOMIZE_BLUR_RADIUS,
        );

        append_opacity_submenu(menu, id_base, view.bg_opacity);

        menu
    }
}

unsafe fn append_str_color_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    current: &str,
    label_key: &'static str,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in NAMED_COLORS.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if val.eq_ignore_ascii_case(current) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        append_menu_text(sub, flags, id, resolve_label(item_label));
    }
    let current_label = NAMED_COLORS
        .iter()
        .find(|(val, _)| val.eq_ignore_ascii_case(current))
        .map(|(_, item_label)| resolve_label(item_label))
        .unwrap_or(resolve_label("color.default"));
    append_menu_text(
        parent,
        MF_POPUP | MF_STRING,
        sub.0 as usize,
        &label_with_current(label_key, current_label),
    );
}

unsafe fn append_int_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    choices: &[(i32, &'static str)],
    current: i32,
    label_key: &'static str,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in choices.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if *val == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        append_menu_text(sub, flags, id, resolve_label(item_label));
    }
    let current_label = choices
        .iter()
        .find(|(val, _)| *val == current)
        .map(|(_, item_label)| resolve_label(item_label))
        .unwrap_or("?");
    append_menu_text(
        parent,
        MF_POPUP | MF_STRING,
        sub.0 as usize,
        &label_with_current(label_key, current_label),
    );
}

unsafe fn append_str_submenu(
    parent: HMENU,
    id_base: usize,
    kind: usize,
    choices: &[(&'static str, &'static str)],
    current: &str,
    label_key: &'static str,
) {
    let sub = unsafe { CreatePopupMenu().unwrap_or_default() };
    for (i, (val, item_label)) in choices.iter().enumerate() {
        let id = encode(id_base, kind, i);
        let flags = if *val == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        append_menu_text(sub, flags, id, resolve_label(item_label));
    }
    let current_label = choices
        .iter()
        .find(|(val, _)| *val == current)
        .map(|(_, item_label)| resolve_label(item_label))
        .unwrap_or("?");
    append_menu_text(
        parent,
        MF_POPUP | MF_STRING,
        sub.0 as usize,
        &label_with_current(label_key, current_label),
    );
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
        append_menu_text(sub, flags, id, resolve_label(item_label));
    }
    let current_label = BG_OPACITIES
        .iter()
        .find(|(val, _)| (current - val).abs() < 0.0001)
        .map(|(_, item_label)| resolve_label(item_label))
        .unwrap_or("?");
    append_menu_text(
        parent,
        MF_POPUP | MF_STRING,
        sub.0 as usize,
        &label_with_current(loc::CUSTOMIZE_BG_OPACITY, current_label),
    );
}

unsafe fn append_toggle(parent: HMENU, id: usize, on: bool, label_key: &'static str) {
    let flags = if on {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    append_menu_key(parent, flags, id, label_key);
}

unsafe fn append_menu_key(parent: HMENU, flags: MENU_ITEM_FLAGS, id: usize, key: &'static str) {
    append_menu_text(parent, flags, id, loc::t(key));
}

unsafe fn append_menu_text(parent: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = unsafe { AppendMenuW(parent, flags, id, PCWSTR(w.as_ptr())) };
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

pub fn decoded_title_align(value: usize) -> Option<String> {
    TITLE_ALIGNS.get(value).map(|(v, _)| v.to_string())
}

/// Note text alignment shares the same string values as the title
/// alignment ("Left"/"Center"/"Right"), so it just defers to the same
/// table. Kept under its own name so the per-kind dispatch reads
/// symmetrically alongside the others.
pub fn decoded_note_align(value: usize) -> Option<String> {
    TITLE_ALIGNS.get(value).map(|(v, _)| v.to_string())
}
