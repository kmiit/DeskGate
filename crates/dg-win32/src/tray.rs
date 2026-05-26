use dg_core::config::FenceDefaults;
use dg_locales as loc;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::*;
use crate::customize::{
    self, KIND_BG_COLOR, KIND_BG_OPACITY, KIND_BLUR_TOGGLE, KIND_BOLD_TOGGLE, KIND_BORDER_COLOR,
    KIND_BORDER_THICK, KIND_COUNT, KIND_ICON_SIZE, KIND_ICON_SPACING, KIND_LABELS_TOGGLE,
    KIND_STRIDE, KIND_TEXT_COLOR, KIND_TITLE_ALIGN, KIND_TITLE_COLOR,
};
use crate::fence_window::{ANIM_FPS_PRESETS, FenceWindow};

pub const TRAY_CLASS_NAME: PCWSTR = w!("DG_TRAY_CLASS");
pub const HOTKEY_ID_TOGGLE_ALL: i32 = 1;

pub struct TrayIcon {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
}

impl TrayIcon {
    pub fn new() -> windows::core::Result<Self> {
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None)?.into() };

        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_wndproc),
            hInstance: hinstance,
            lpszClassName: TRAY_CLASS_NAME,
            ..Default::default()
        };
        unsafe {
            let _ = RegisterClassW(&wc);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                TRAY_CLASS_NAME,
                w!(""),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinstance),
                None,
            )?
        };

        let hicon = unsafe { LoadIconW(None, IDI_APPLICATION)? };

        let tip: Vec<u16> = "DeskGate\0".encode_utf16().collect();
        let mut tip_arr = [0u16; 128];
        for (i, c) in tip.iter().enumerate() {
            if i >= 127 {
                break;
            }
            tip_arr[i] = *c;
        }

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: hicon,
            szTip: tip_arr,
            ..Default::default()
        };

        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err(Error::from_thread());
            }
            // Win+D-like global hotkey: Ctrl+Alt+D toggles all fences. (Win+D itself
            // is reserved by the shell; intercepting it via low-level hook is
            // intrusive, so we pick a less invasive combo.)
            let _ = RegisterHotKey(
                Some(hwnd),
                HOTKEY_ID_TOGGLE_ALL,
                MOD_CONTROL | MOD_ALT,
                'D' as u32,
            );
        }

        Ok(Self { hwnd, nid })
    }

    pub fn show_context_menu(&self) {
        unsafe {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let _ = SetForegroundWindow(self.hwnd);

            let menu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_TRAY_NEW_FENCE,
                loc::tw!(loc::TRAY_NEW_FENCE),
            );
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_TRAY_NEW_NOTE,
                loc::tw!(loc::TRAY_NEW_NOTE),
            );
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_TRAY_NEW_TODO,
                loc::tw!(loc::TRAY_NEW_TODO),
            );
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_RELOAD, loc::tw!(loc::TRAY_RELOAD));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            // Read settings once, build submenus from a stable snapshot.
            // Both submenus only build the menu structure; nothing is
            // mutated yet — that happens in WM_COMMAND.
            let (anim_fps, defaults) = crate::app::with_state(|s| {
                (
                    s.config.settings.anim_fps,
                    s.config.settings.fence_defaults.clone(),
                )
            })
            .unwrap_or((60, FenceDefaults::default()));

            let fps_menu = build_anim_fps_menu(anim_fps);
            let _ = AppendMenuW(
                menu,
                MF_POPUP,
                fps_menu.0 as usize,
                loc::tw!(loc::TRAY_ANIM_FPS),
            );

            let defaults_menu = build_defaults_menu(&defaults);
            let _ = AppendMenuW(
                menu,
                MF_POPUP,
                defaults_menu.0 as usize,
                loc::tw!(loc::TRAY_DEFAULT_SETTINGS),
            );

            let lang_menu = build_lang_menu();
            let _ = AppendMenuW(
                menu,
                MF_POPUP,
                lang_menu.0 as usize,
                loc::tw!(loc::LANG_LABEL),
            );

            let autostart_flags = if crate::autostart::is_enabled() {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let _ = AppendMenuW(
                menu,
                autostart_flags,
                ID_TRAY_AUTOSTART,
                loc::tw!(loc::TRAY_AUTOSTART),
            );

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, loc::tw!(loc::TRAY_EXIT));

            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
                cursor.x,
                cursor.y,
                None,
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID_TOGGLE_ALL);
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}

/// Build the Animation FPS submenu, marking the current global FPS as
/// checked. IDs are dense (ID_TRAY_ANIM_FPS_BASE + preset index) so the
/// click handler can dispatch by subtracting the base.
fn build_anim_fps_menu(current_fps: i32) -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        for (i, val) in ANIM_FPS_PRESETS.iter().enumerate() {
            let id = ID_TRAY_ANIM_FPS_BASE + i;
            let flags = if *val == current_fps {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let w = loc::tw(crate::fence_window::fps_label(*val));
            let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
        }
        menu
    }
}

/// Build the "Default fence settings" submenu, mirroring the per-fence
/// Customize menu but reading from / writing to `FenceDefaults` instead
/// of an individual fence.
fn build_defaults_menu(d: &FenceDefaults) -> HMENU {
    let view = customize::CustomizeView::from(d);
    customize::build_customize_menu(&view, ID_TRAY_DEFAULTS_BASE, ID_TRAY_DEFAULTS_BLUR_RADIUS)
}

/// Build the language submenu with a check mark on the current language.
fn build_lang_menu() -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        let current = loc::lang();
        for (i, (code, label_key)) in loc::languages().iter().enumerate() {
            let id = ID_TRAY_LANG_BASE + i;
            let flags = if *code == current {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let w = loc::tw(label_key);
            let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
        }
        menu
    }
}

/// Apply a click in the "Default fence settings" submenu, decoded as
/// `code = kind * 64 + value`. Mirrors apply_customize in fence_window
/// but the destination is `AppSettings::fence_defaults` rather than an
/// individual fence — existing fences are deliberately untouched.
fn apply_defaults(code: usize) {
    let (kind, value) = (code / KIND_STRIDE, code % KIND_STRIDE);
    unsafe {
        crate::app::with_state_mut(|s| {
            let d = &mut s.config.settings.fence_defaults;
            match kind {
                KIND_BG_COLOR | KIND_BORDER_COLOR | KIND_TITLE_COLOR | KIND_TEXT_COLOR => {
                    let Some(opt) = customize::decoded_color(value) else {
                        return;
                    };
                    match kind {
                        KIND_BG_COLOR => d.custom_color = opt,
                        KIND_BORDER_COLOR => d.fence_border_color = opt,
                        KIND_TITLE_COLOR => d.title_text_color = opt,
                        KIND_TEXT_COLOR => d.text_color = opt,
                        _ => {}
                    }
                }
                KIND_BORDER_THICK => {
                    let Some(v) = customize::decoded_border_thick(value) else {
                        return;
                    };
                    d.fence_border_thickness = v;
                }
                KIND_ICON_SIZE => {
                    let Some(v) = customize::decoded_icon_size(value) else {
                        return;
                    };
                    d.icon_size = v;
                }
                KIND_ICON_SPACING => {
                    let Some(v) = customize::decoded_icon_spacing(value) else {
                        return;
                    };
                    d.icon_spacing = v;
                }
                KIND_BOLD_TOGGLE => {
                    d.bold_title_text = crate::fence_window::toggle_bool_str(&d.bold_title_text);
                }
                KIND_BLUR_TOGGLE => {
                    d.blur_enabled = crate::fence_window::toggle_bool_str(&d.blur_enabled);
                }
                KIND_BG_OPACITY => {
                    let Some(v) = customize::decoded_opacity(value) else {
                        return;
                    };
                    d.bg_opacity = v;
                }
                KIND_LABELS_TOGGLE => {
                    d.show_item_labels = crate::fence_window::toggle_bool_str(&d.show_item_labels);
                }
                KIND_TITLE_ALIGN => {
                    let Some(v) = customize::decoded_title_align(value) else {
                        return;
                    };
                    d.title_text_align = v;
                }
                _ => {}
            }
            let _ = s.config.save_settings();
        });
    }
}

/// Spawn a new fence using the saved `FenceDefaults` as the template.
/// Only fields the user is allowed to preconfigure come from defaults;
/// per-fence identity (id, position, title, items) is freshly minted.
/// `items_type` is "Data" for a normal shortcut fence or "Note" for a
/// sticky-note/TODO fence; `note_mode` only matters for "Note" — pass
/// "text" or "todo".
fn new_fence_from_defaults(items_type: &str, note_mode: &str) -> dg_core::fence::Fence {
    let d = unsafe {
        crate::app::with_state(|s| s.config.settings.fence_defaults.clone()).unwrap_or_default()
    };
    let title_key = if items_type == "Note" {
        if note_mode == "todo" {
            loc::NEW_TODO_TITLE
        } else {
            loc::NEW_NOTE_TITLE
        }
    } else {
        loc::NEW_FENCE_TITLE
    };
    dg_core::fence::Fence {
        id: uuid::Uuid::new_v4().to_string(),
        title: loc::t(title_key).to_string(),
        x: 100.0,
        y: 100.0,
        width: d.width,
        height: d.height,
        items_type: items_type.into(),
        items: Vec::new(),
        is_locked: "false".into(),
        is_hidden: "false".into(),
        is_rolled: "false".into(),
        unrolled_height: d.height,
        tabs_enabled: "false".into(),
        current_tab: 0,
        tabs: Vec::new(),
        custom_color: d.custom_color.clone(),
        fence_border_thickness: d.fence_border_thickness,
        icon_size: d.icon_size.clone(),
        icon_spacing: d.icon_spacing,
        custom_launch_effect: None,
        text_color: d.text_color.clone(),
        title_text_color: d.title_text_color.clone(),
        title_text_size: d.title_text_size.clone(),
        bold_title_text: d.bold_title_text.clone(),
        disable_text_shadow: "false".into(),
        grayscale_icons: "false".into(),
        fence_border_color: d.fence_border_color.clone(),
        note_content: None,
        note_font_size: "Medium".into(),
        note_font_family: None,
        word_wrap: "true".into(),
        note_mode: note_mode.into(),
        note_items: Vec::new(),
        note_text_align: "Left".into(),
        blur_enabled: d.blur_enabled.clone(),
        blur_radius: d.blur_radius,
        bg_opacity: d.bg_opacity,
        show_item_labels: d.show_item_labels.clone(),
        title_text_align: d.title_text_align.clone(),
    }
}

/// Build, register, and persist a fresh fence created from the tray
/// menu. Shared between "New Fence", "New Note", and "New TODO" so all
/// three entry points use the same FenceDefaults-driven shape and the
/// same save path. `items_type` and `note_mode` are forwarded to
/// `new_fence_from_defaults`.
fn spawn_new_fence(items_type: &str, note_mode: &str) {
    unsafe {
        crate::app::with_state_mut(|s| {
            let new_fence = new_fence_from_defaults(items_type, note_mode);
            match FenceWindow::create(&new_fence) {
                Ok(fw) => {
                    s.fences.push(fw);
                    s.config.fences.push(new_fence);
                    let _ = s.config.save_fences();
                }
                Err(e) => {
                    eprintln!("Failed to create new fence: {:?}", e)
                }
            }
        });
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID_TOGGLE_ALL => {
            unsafe { toggle_all_fences() };
            return LRESULT(0);
        }
        WM_TRAYICON => {
            let event = (lparam.0 as u32) & 0xFFFF;
            if event == (WM_RBUTTONUP) {
                unsafe {
                    crate::app::with_state(|s| s.tray.show_context_menu());
                };
            } else if event == (WM_LBUTTONDBLCLK) {
                unsafe { toggle_all_fences() };
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            let id = ((wparam.0 as u32) & 0xFFFF) as usize;
            match id {
                ID_TRAY_EXIT => {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                    PostQuitMessage(0);
                }
                ID_TRAY_AUTOSTART => {
                    let enable = !crate::autostart::is_enabled();
                    if let Err(e) = crate::autostart::set_enabled(enable) {
                        eprintln!("Failed to toggle autostart: {:?}", e);
                    }
                }
                ID_TRAY_NEW_FENCE => {
                    spawn_new_fence("Data", "text");
                }
                ID_TRAY_NEW_NOTE => {
                    spawn_new_fence("Note", "text");
                }
                ID_TRAY_NEW_TODO => {
                    spawn_new_fence("Note", "todo");
                }
                ID_TRAY_RELOAD => {
                    unsafe {
                        crate::app::with_state_mut(|s| {
                            // FenceWindow::Drop handles RevokeDragDrop + DestroyWindow.
                            s.fences.clear();
                            if let Ok(new_config) =
                                dg_core::config::AppConfig::load(&s.config.config_dir)
                            {
                                s.config.fences = new_config.fences;
                                s.config.settings = new_config.settings;
                            }
                            let fences_data = s.config.fences.clone();
                            for fence_data in &fences_data {
                                if fence_data.is_hidden == "true" {
                                    continue;
                                }
                                if fence_data.items_type != "Data"
                                    && fence_data.items_type != "Note"
                                {
                                    continue;
                                }
                                if let Ok(fw) = FenceWindow::create(fence_data) {
                                    s.fences.push(fw);
                                }
                            }
                        });
                    };
                }
                ID_TRAY_DEFAULTS_BLUR_RADIUS => {
                    let current = unsafe {
                        crate::app::with_state(|s| s.config.settings.fence_defaults.blur_radius)
                            .unwrap_or(20.0)
                    };
                    let initial = format!("{}", current.round() as i32);
                    if let Some(input) =
                        crate::modal::input(hwnd, loc::t(loc::TRAY_DEFAULT_BLUR_PROMPT), &initial)
                        && let Ok(parsed) = input.trim().parse::<f64>()
                    {
                        let radius = parsed.clamp(0.0, 150.0);
                        unsafe {
                            crate::app::with_state_mut(|s| {
                                s.config.settings.fence_defaults.blur_radius = radius;
                                let _ = s.config.save_settings();
                            });
                        }
                    }
                }
                n if n >= ID_TRAY_ANIM_FPS_BASE
                    && n < ID_TRAY_ANIM_FPS_BASE + ANIM_FPS_PRESETS.len() =>
                {
                    let idx = n - ID_TRAY_ANIM_FPS_BASE;
                    let new_fps = ANIM_FPS_PRESETS[idx];
                    unsafe {
                        crate::app::with_state_mut(|s| {
                            s.config.settings.anim_fps = new_fps;
                            let _ = s.config.save_settings();
                        });
                    }
                }
                n if (ID_TRAY_DEFAULTS_BASE..ID_TRAY_DEFAULTS_BASE + KIND_COUNT * KIND_STRIDE)
                    .contains(&n) =>
                {
                    apply_defaults(n - ID_TRAY_DEFAULTS_BASE);
                }
                n if n >= ID_TRAY_LANG_BASE && n < ID_TRAY_LANG_BASE + loc::languages().len() => {
                    let idx = n - ID_TRAY_LANG_BASE;
                    let code = loc::languages()[idx].0;
                    loc::init(code);
                    unsafe {
                        crate::app::with_state_mut(|s| {
                            s.config.settings.language = Some(code.to_string());
                            let _ = s.config.save_settings();
                        });
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn toggle_all_fences() {
    crate::app::with_state_mut(|s| {
        let any_visible = s.fences.iter().any(|fw| IsWindowVisible(fw.hwnd).as_bool());
        for fw in &s.fences {
            let _ = ShowWindow(
                fw.hwnd,
                if any_visible {
                    SW_HIDE
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
        }
    });
}
