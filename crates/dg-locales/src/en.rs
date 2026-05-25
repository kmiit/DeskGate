use crate::*;

pub fn translate(key: &'static str) -> &'static str {
    match key {
        TRAY_NEW_FENCE => "New Fence",
        TRAY_RELOAD => "Reload All",
        TRAY_ANIM_FPS => "Animation FPS",
        TRAY_DEFAULT_SETTINGS => "Default fence settings",
        TRAY_AUTOSTART => "Start with Windows",
        TRAY_EXIT => "Exit",
        TRAY_DEFAULT_BLUR_PROMPT => "Default blur radius (0-150)",

        FENCE_OPEN => "Open",
        FENCE_OPEN_LOCATION => "Open file location",
        FENCE_REMOVE => "Remove from fence",
        FENCE_ROLL_UP => "Roll up",
        FENCE_UNROLL => "Unroll",
        FENCE_RENAME => "Rename...",
        FENCE_LOCK => "Lock",
        FENCE_UNLOCK => "Unlock",
        FENCE_CUSTOMIZE => "Customize",
        FENCE_DELETE => "Delete fence",
        FENCE_BLUR_PROMPT => "Blur radius (0-150)",
        FENCE_RENAME_PROMPT => "Rename fence",

        FPS_OFF => "Off (snap)",
        FPS_DEFAULT => "60 FPS (default)",

        CUSTOMIZE_BG_COLOR => "Background color",
        CUSTOMIZE_BORDER_COLOR => "Border color",
        CUSTOMIZE_TITLE_COLOR => "Title color",
        CUSTOMIZE_LABEL_COLOR => "Label color",
        CUSTOMIZE_BORDER_THICK => "Border thickness",
        CUSTOMIZE_ICON_SIZE => "Icon size",
        CUSTOMIZE_ICON_SPACING => "Icon spacing",
        CUSTOMIZE_BOLD_TITLE => "Bold title",
        CUSTOMIZE_SHOW_LABELS => "Show item labels",
        CUSTOMIZE_BG_BLUR => "Background blur",
        CUSTOMIZE_BLUR_RADIUS => "Blur radius...",
        CUSTOMIZE_BG_OPACITY => "Background opacity",
        CUSTOMIZE_TITLE_ALIGN => "Title alignment",
        ALIGN_LEFT => "Left",
        ALIGN_CENTER => "Center",
        ALIGN_RIGHT => "Right",

        COLOR_DEFAULT => "(default)",
        COLOR_RED => "Red",
        COLOR_GREEN => "Green",
        COLOR_BLUE => "Blue",
        COLOR_TEAL => "Teal",
        COLOR_PURPLE => "Purple",
        COLOR_ORANGE => "Orange",
        COLOR_PINK => "Pink",
        COLOR_YELLOW => "Yellow",
        COLOR_GRAY => "Gray",
        COLOR_BLACK => "Black",
        COLOR_WHITE => "White",

        SIZE_TINY => "Tiny (16)",
        SIZE_SMALL => "Small (24)",
        SIZE_MEDIUM => "Medium (32)",
        SIZE_LARGE => "Large (48)",
        SIZE_HUGE => "Huge (64)",

        OPACITY_TRANSPARENT => "0% (transparent)",
        OPACITY_DEFAULT => "45% (default)",
        OPACITY_SOLID => "100% (solid)",

        MODAL_OK => "OK",
        MODAL_CANCEL => "Cancel",

        DELETE_TITLE => "Delete this fence?",
        DELETE_TITLE_NAMED => "Delete the fence \u{201c}{}\u{201d}?",
        DELETE_DETAILS => {
            "Its contents are shortcuts only \u{2014} the original files stay where they are. This cannot be undone from inside the app."
        }
        DELETE_CONFIRM => "Delete fence",

        NEW_FENCE_TITLE => "New Fence - Drop your shortcuts here",

        LANG_LABEL => "Language",
        LANG_EN => "English",
        LANG_ZH_CN => "简体中文",
        LANG_ZH_TW => "繁體中文",

        _ => key,
    }
}
