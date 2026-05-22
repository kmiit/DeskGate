use crate::*;

pub fn translate(key: &'static str) -> &'static str {
    match key {
        TRAY_NEW_FENCE => "新建栏目",
        TRAY_RELOAD => "重新加载",
        TRAY_ANIM_FPS => "动画帧率",
        TRAY_DEFAULT_SETTINGS => "默认栏目设置",
        TRAY_EXIT => "退出",
        TRAY_DEFAULT_BLUR_PROMPT => "默认模糊半径 (0-150)",

        FENCE_OPEN => "打开",
        FENCE_OPEN_LOCATION => "打开文件位置",
        FENCE_REMOVE => "从栏目中移除",
        FENCE_ROLL_UP => "收起",
        FENCE_UNROLL => "展开",
        FENCE_RENAME => "重命名...",
        FENCE_LOCK => "锁定",
        FENCE_UNLOCK => "解锁",
        FENCE_CUSTOMIZE => "自定义",
        FENCE_DELETE => "删除栏目",
        FENCE_BLUR_PROMPT => "模糊半径 (0-150)",
        FENCE_RENAME_PROMPT => "重命名栏目",

        FPS_OFF => "关闭 (吸附)",
        FPS_DEFAULT => "60 帧 (默认)",

        CUSTOMIZE_BG_COLOR => "背景颜色",
        CUSTOMIZE_BORDER_COLOR => "边框颜色",
        CUSTOMIZE_TITLE_COLOR => "标题颜色",
        CUSTOMIZE_LABEL_COLOR => "标签颜色",
        CUSTOMIZE_BORDER_THICK => "边框粗细",
        CUSTOMIZE_ICON_SIZE => "图标大小",
        CUSTOMIZE_ICON_SPACING => "图标间距",
        CUSTOMIZE_BOLD_TITLE => "粗体标题",
        CUSTOMIZE_SHOW_LABELS => "显示标签",
        CUSTOMIZE_BG_BLUR => "背景模糊",
        CUSTOMIZE_BLUR_RADIUS => "模糊半径...",
        CUSTOMIZE_BG_OPACITY => "背景不透明度",

        COLOR_DEFAULT => "(默认)",
        COLOR_RED => "红色",
        COLOR_GREEN => "绿色",
        COLOR_BLUE => "蓝色",
        COLOR_TEAL => "青色",
        COLOR_PURPLE => "紫色",
        COLOR_ORANGE => "橙色",
        COLOR_PINK => "粉色",
        COLOR_YELLOW => "黄色",
        COLOR_GRAY => "灰色",
        COLOR_BLACK => "黑色",
        COLOR_WHITE => "白色",

        SIZE_TINY => "极小 (16)",
        SIZE_SMALL => "小 (24)",
        SIZE_MEDIUM => "中 (32)",
        SIZE_LARGE => "大 (48)",
        SIZE_HUGE => "超大 (64)",

        OPACITY_TRANSPARENT => "0% (透明)",
        OPACITY_DEFAULT => "45% (默认)",
        OPACITY_SOLID => "100% (实心)",

        MODAL_OK => "确定",
        MODAL_CANCEL => "取消",

        DELETE_TITLE => "删除此栏目？",
        DELETE_TITLE_NAMED => "删除栏目\u{201c}{}\u{201d}？",
        DELETE_DETAILS => "内容仅为快捷方式 — 原始文件保持不变。此操作无法在应用内撤销。",
        DELETE_CONFIRM => "删除栏目",

        NEW_FENCE_TITLE => "新建栏目 - 拖放快捷方式到此处",

        LANG_LABEL => "语言",
        LANG_EN => "English",
        LANG_ZH_CN => "简体中文",
        LANG_ZH_TW => "繁體中文",

        _ => super::en::translate(key),
    }
}
