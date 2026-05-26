use crate::*;

pub fn translate(key: &'static str) -> &'static str {
    match key {
        TRAY_NEW_FENCE => "新建欄目",
        TRAY_RELOAD => "重新載入",
        TRAY_ANIM_FPS => "動畫幀率",
        TRAY_DEFAULT_SETTINGS => "預設欄目設定",
        TRAY_AUTOSTART => "開機自啟",
        TRAY_EXIT => "退出",
        TRAY_DEFAULT_BLUR_PROMPT => "預設模糊半徑 (0-150)",

        FENCE_OPEN => "開啟",
        FENCE_OPEN_LOCATION => "開啟檔案位置",
        FENCE_REMOVE => "從欄目中移除",
        FENCE_ROLL_UP => "收起",
        FENCE_UNROLL => "展開",
        FENCE_RENAME => "重新命名...",
        FENCE_LOCK => "鎖定",
        FENCE_UNLOCK => "解鎖",
        FENCE_CUSTOMIZE => "自訂",
        FENCE_DELETE => "刪除欄目",
        FENCE_BLUR_PROMPT => "模糊半徑 (0-150)",
        FENCE_RENAME_PROMPT => "重新命名欄目",

        FPS_OFF => "關閉 (吸附)",
        FPS_DEFAULT => "60 幀 (預設)",

        CUSTOMIZE_BG_COLOR => "背景顏色",
        CUSTOMIZE_BORDER_COLOR => "邊框顏色",
        CUSTOMIZE_TITLE_COLOR => "標題顏色",
        CUSTOMIZE_LABEL_COLOR => "標籤顏色",
        CUSTOMIZE_BORDER_THICK => "邊框粗細",
        CUSTOMIZE_ICON_SIZE => "圖示大小",
        CUSTOMIZE_ICON_SPACING => "圖示間距",
        CUSTOMIZE_BOLD_TITLE => "粗體標題",
        CUSTOMIZE_SHOW_LABELS => "顯示標籤",
        CUSTOMIZE_BG_BLUR => "背景模糊",
        CUSTOMIZE_BLUR_RADIUS => "模糊半徑...",
        CUSTOMIZE_BG_OPACITY => "背景不透明度",
        CUSTOMIZE_TITLE_ALIGN => "標題對齊",
        ALIGN_LEFT => "靠左",
        ALIGN_CENTER => "置中",
        ALIGN_RIGHT => "靠右",

        COLOR_DEFAULT => "(預設)",
        COLOR_RED => "紅色",
        COLOR_GREEN => "綠色",
        COLOR_BLUE => "藍色",
        COLOR_TEAL => "青色",
        COLOR_PURPLE => "紫色",
        COLOR_ORANGE => "橙色",
        COLOR_PINK => "粉色",
        COLOR_YELLOW => "黃色",
        COLOR_GRAY => "灰色",
        COLOR_BLACK => "黑色",
        COLOR_WHITE => "白色",

        SIZE_TINY => "極小 (16)",
        SIZE_SMALL => "小 (24)",
        SIZE_MEDIUM => "中 (32)",
        SIZE_LARGE => "大 (48)",
        SIZE_HUGE => "超大 (64)",

        OPACITY_TRANSPARENT => "0% (透明)",
        OPACITY_DEFAULT => "45% (預設)",
        OPACITY_SOLID => "100% (實心)",

        MODAL_OK => "確定",
        MODAL_CANCEL => "取消",

        DELETE_TITLE => "刪除此欄目？",
        DELETE_TITLE_NAMED => "刪除欄目\u{201c}{}\u{201d}？",
        DELETE_DETAILS => "內容僅為捷徑 — 原始檔案保持不變。此操作無法在應用內撤銷。",
        DELETE_CONFIRM => "刪除欄目",

        NEW_FENCE_TITLE => "新建欄目 - 拖放捷徑到此處",

        LANG_LABEL => "語言",
        LANG_EN => "English",
        LANG_ZH_CN => "简体中文",
        LANG_ZH_TW => "繁體中文",

        DROP_DESC_OPEN_WITH => "以 %1 開啟",
        DROP_DESC_ADD_TO => "新增至 %1",

        _ => super::en::translate(key),
    }
}
