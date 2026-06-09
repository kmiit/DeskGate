use dg_core::config::AppConfig;
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::fence_window::FenceWindow;
use crate::tray::TrayIcon;

pub const WM_TRAYICON: u32 = WM_USER + 1;
pub const ID_TRAY_EXIT: usize = 1001;
pub const ID_TRAY_NEW_FENCE: usize = 1002;
pub const ID_TRAY_RELOAD: usize = 1003;
pub const ID_TRAY_AUTOSTART: usize = 1004;
pub const ID_TRAY_NEW_NOTE: usize = 1005;
pub const ID_TRAY_NEW_TODO: usize = 1006;
// Animation FPS submenu: ID_TRAY_ANIM_FPS_BASE + preset index.
pub const ID_TRAY_ANIM_FPS_BASE: usize = 1100;
// One-shot menu item for entering a custom blur radius for new fences.
pub const ID_TRAY_DEFAULTS_BLUR_RADIUS: usize = 1199;
// "Default fence settings" submenu: ID_TRAY_DEFAULTS_BASE + kind * 64
// + value. Same `kind` numbering as fence_window's KIND_* constants so
// the same NAMED_COLORS / BORDER_THICKNESSES tables can be reused.
pub const ID_TRAY_DEFAULTS_BASE: usize = 1200;
// Language submenu: one item per supported language.
pub const ID_TRAY_LANG_BASE: usize = 1300;

// SAFETY: Single-threaded app, all access from the message pump thread.
static mut APP_STATE: Option<AppState> = None;

pub struct AppState {
    pub config: AppConfig,
    pub fences: Vec<FenceWindow>,
    pub tray: TrayIcon,
}

pub fn run() -> Box<dyn std::error::Error> {
    let mutex_name = w!("Global\\DeskGate_Mutex_UniqueId");

    // Single instance check
    unsafe {
        if OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, mutex_name).is_ok() {
            return "Another instance is already running".into();
        }
        let _mutex = match CreateMutexW(None, true, mutex_name) {
            Ok(h) => h,
            Err(e) => return e.into(),
        };
    }

    // DPI awareness
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // OLE for drag-and-drop. Must precede any RegisterDragDrop call.
    unsafe {
        let _ = OleInitialize(None);
    }

    // WinRT Composition (Compositor + DispatcherQueue) — owned for the
    // process lifetime. Must come after OleInitialize so the apartment is
    // already an STA when CreateDispatcherQueueController inherits it.
    if let Err(e) = crate::composition::init() {
        return e.into();
    }

    // Load config
    let profile_dir = AppConfig::default_profile_dir();
    if let Err(e) = std::fs::create_dir_all(&profile_dir) {
        return e.into();
    }
    let mut config = match AppConfig::load(&profile_dir) {
        Ok(c) => c,
        Err(e) => return e,
    };
    if crate::storage::migrate_desktop_folders(&mut config.fences) {
        let _ = config.save_fences();
    }

    // Initialize locale
    let lang = config.settings.language.clone().unwrap_or_else(|| {
        let lang_id = unsafe { GetUserDefaultUILanguage() };
        let primary = lang_id & 0x3FF;
        let sub = (lang_id >> 10) & 0x3F;
        match (primary, sub) {
            (0x04, 0x02) => "zh_CN", // SUBLANG_CHINESE_SIMPLIFIED
            (0x04, _) => "zh_TW",    // SUBLANG_CHINESE_TRADITIONAL & others
            _ => "en",
        }
        .to_string()
    });
    dg_locales::init(&lang);
    #[cfg(debug_assertions)]
    eprintln!(
        "[dg] profile_dir={} fences={}",
        profile_dir.display(),
        config.fences.len()
    );

    // Register window classes
    if let Err(e) = crate::fence_window::register_class() {
        return e.into();
    }

    // Create tray icon
    let tray = match TrayIcon::new() {
        Ok(t) => t,
        Err(e) => return e.into(),
    };

    // Create fence windows
    let mut fences = Vec::new();
    for fence_data in &config.fences {
        if fence_data.is_hidden == "true" {
            continue;
        }
        if fence_data.items_type != "Data" && fence_data.items_type != "Note" {
            continue;
        }
        match FenceWindow::create(fence_data) {
            Ok(fw) => fences.push(fw),
            Err(e) => eprintln!("Failed to create fence '{}': {:?}", fence_data.title, e),
        }
    }

    unsafe {
        APP_STATE = Some(AppState {
            config,
            fences,
            tray,
        });
    }

    // Message loop
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    "Normal exit".into()
}

/// Hand a closure shared access to the singleton `APP_STATE`.
///
/// # Safety
///
/// Must be called from the message pump thread only.
pub unsafe fn with_state<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&AppState) -> R,
{
    APP_STATE.as_ref().map(f)
}

/// Hand a closure exclusive access to the singleton `APP_STATE`.
///
/// # Safety
///
/// Must be called from the message pump thread only.
pub unsafe fn with_state_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut AppState) -> R,
{
    APP_STATE.as_mut().map(f)
}
