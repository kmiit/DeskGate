// Lifecycle and message loop of a self-drawn modal: window creation,
// local message pump, hit-testing, mouse capture/hover state. All the
// painting lives in the sibling `render` module — this file decides
// *when* to repaint (WM_PAINT, hover changes, button presses) and
// stashes the per-window `ModalState` that both halves talk to via
// GWLP_USERDATA.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::ModalSpec;
use super::render;
use super::{
    AVG_BODY_GLYPH_W, AVG_TITLE_GLYPH_W, BODY_LINE_H, BTN_H, BTN_W, DRAG_STRIP_H, EDIT_H,
    EDIT_H_MULTILINE, PAD, TITLE_LINE_H,
};

const MODAL_CLASS: PCWSTR = w!("DG_MODAL_CLASS");

// WM_MOUSELEAVE lives in Win32_UI_Controls; declare locally to avoid
// pulling in the whole namespace.
const WM_MOUSELEAVE_U: u32 = 0x02A3;

pub(super) struct ModalState {
    pub(super) spec: ModalSpec,
    pub(super) dwrite_factory: Option<IDWriteFactory>,
    pub(super) rt: Option<ID2D1HwndRenderTarget>,
    pub(super) edit_hwnd: Option<HWND>,
    pub(super) hover_btn: i32,
    pub(super) pressed_btn: i32,
    pub(super) result: i32,
    pub(super) done: bool,
    /// Cached background brush for EDIT — keeps it visually consistent
    /// with the rounded panel we paint behind it.
    pub(super) edit_bg_brush: HBRUSH,
}

pub(super) fn run_modal(owner: HWND, spec: ModalSpec) -> (i32, Option<String>) {
    unsafe {
        if let Err(e) = register_class() {
            eprintln!("[dg] modal register_class failed: {:?}", e);
            return (i32::MIN, None);
        }

        let dpi = crate::fence_window::window_dpi(owner);
        let total_h_dip = total_height(&spec, dpi);
        let w_px = dip_to_px(spec.width, dpi);
        let h_px = dip_to_px(total_h_dip, dpi);

        let mut orect = RECT::default();
        let _ = GetWindowRect(owner, &mut orect);
        let x = orect.left + ((orect.right - orect.left) - w_px) / 2;
        let y = orect.top + ((orect.bottom - orect.top) - h_px) / 2;

        let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();

        let hwnd = match CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            MODAL_CLASS,
            w!(""),
            // WS_CLIPCHILDREN keeps our D2D paint from clobbering child
            // controls (the embedded EDIT). Without it, we redraw the
            // whole client area each frame and overwrite the EDIT's text.
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            w_px,
            h_px,
            Some(owner),
            None,
            Some(hinstance),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[dg] modal CreateWindowExW failed: {:?}", e);
                return (i32::MIN, None);
            }
        };

        // No DWM BlurBehind: it can't be clipped to our rounded corners
        // (so the square modal silhouette shows through outside the
        // radius) and it overlays a blur layer on the EDIT control's
        // rectangle that swallows the typed glyphs. The modal is short-
        // lived, so the plain dark rounded panel reads fine on its own.
        //
        // To still get rounded *window* corners (and not a black square
        // outside the painted radius), ask DWM to round them. The OS does
        // antialiasing for us on Win11; on Win10 the call no-ops.
        apply_dwm_round_corners(hwnd);

        let edit_bg_brush = CreateSolidBrush(COLORREF(0x00181818));
        let state = Box::new(ModalState {
            spec,
            dwrite_factory: None,
            rt: None,
            edit_hwnd: None,
            hover_btn: -1,
            pressed_btn: -1,
            result: i32::MIN,
            done: false,
            edit_bg_brush,
        });
        let state_ptr = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

        // Build D2D HwndRenderTarget.
        match build_render_target(hwnd, dpi) {
            Ok((rt, dwrite)) => {
                (*state_ptr).rt = Some(rt);
                (*state_ptr).dwrite_factory = Some(dwrite);
            }
            Err(e) => {
                eprintln!("[dg] modal RT create failed: {:?}", e);
                let _ = DestroyWindow(hwnd);
                let _ = Box::from_raw(state_ptr);
                return (i32::MIN, None);
            }
        }

        if let Some(def) = (*state_ptr).spec.edit_default.clone() {
            let multiline = (*state_ptr).spec.multiline;
            // EDIT controls expect CRLF line breaks, not raw LF.
            let def_for_ctrl = if multiline {
                def.replace("\r\n", "\n").replace('\n', "\r\n")
            } else {
                def
            };
            let wdef: Vec<u16> = def_for_ctrl
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let edit_rect = edit_rect_px(&(*state_ptr).spec, dpi);
            const ES_AUTOHSCROLL: u32 = 0x0080;
            const ES_AUTOVSCROLL: u32 = 0x0040;
            const ES_MULTILINE: u32 = 0x0004;
            const ES_WANTRETURN: u32 = 0x1000;
            let extra_style = if multiline {
                WS_VSCROLL.0 | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN
            } else {
                ES_AUTOHSCROLL
            };
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR(wdef.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(extra_style),
                edit_rect.left,
                edit_rect.top,
                edit_rect.right - edit_rect.left,
                edit_rect.bottom - edit_rect.top,
                Some(hwnd),
                None,
                Some(hinstance),
                None,
            )
            .unwrap_or_default();
            (*state_ptr).edit_hwnd = Some(edit);

            // DPI-aware Segoe UI font.
            let hfont = create_segoe_font(dpi, 14);
            SendMessageW(
                edit,
                WM_SETFONT,
                Some(WPARAM(hfont.0 as usize)),
                Some(LPARAM(1)),
            );
            const EM_SETSEL: u32 = 0x00B1;
            if multiline {
                // Place the caret at the end so the user can keep typing
                // without overwriting the existing content.
                SendMessageW(edit, EM_SETSEL, Some(WPARAM(-1i32 as usize)), Some(LPARAM(-1)));
            } else {
                SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
            let _ = SetFocus(Some(edit));
        }

        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetForegroundWindow(hwnd);
        if (*state_ptr).edit_hwnd.is_none() {
            let _ = SetFocus(Some(hwnd));
        }
        let _ = render::render(hwnd);

        // Modal loop.
        let mut msg = MSG::default();
        while !(*state_ptr).done && GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.message == WM_KEYDOWN {
                let vk = msg.wParam.0 as u32;
                let st = &mut *state_ptr;
                if vk == VK_ESCAPE.0 as u32 {
                    if let Some(idx) = st.spec.buttons.iter().position(|b| b.cancel) {
                        st.result = st.spec.buttons[idx].result;
                    }
                    st.done = true;
                    continue;
                }
                // In a multi-line editor Enter must insert a newline, not
                // commit OK — otherwise the user can't enter more than
                // one TODO line. Ctrl+Enter still confirms.
                let ctrl_held = (GetKeyState(VK_CONTROL.0 as i32) as u16) & 0x8000 != 0;
                let enter_commits = !st.spec.multiline || ctrl_held;
                if vk == VK_RETURN.0 as u32
                    && enter_commits
                    && let Some(idx) = st.spec.buttons.iter().position(|b| b.default)
                {
                    st.result = st.spec.buttons[idx].result;
                    st.done = true;
                    continue;
                }
                if vk == VK_TAB.0 as u32 {
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let result = (*state_ptr).result;
        let text = if let Some(edit) = (*state_ptr).edit_hwnd {
            let len = GetWindowTextLengthW(edit) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = GetWindowTextW(edit, &mut buf) as usize;
            let raw = String::from_utf16_lossy(&buf[..n]);
            // The multiline EDIT control hands back CRLF line breaks.
            // Normalize to plain \n so callers don't have to know whether
            // they got input from a single-line or multiline editor.
            Some(if (*state_ptr).spec.multiline {
                raw.replace("\r\n", "\n")
            } else {
                raw
            })
        } else {
            None
        };

        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
        let _ = DestroyWindow(hwnd);
        let _ = DeleteObject((*state_ptr).edit_bg_brush.into());
        let _ = Box::from_raw(state_ptr);

        (result, text)
    }
}

#[inline]
pub(super) fn dip_to_px(dip: f32, dpi: u32) -> i32 {
    (dip * dpi as f32 / 96.0).round() as i32
}

/// Round the window corners using DWM (Win11+). On Win10 this call
/// silently fails — the corner will then look square but without the
/// jagged aliasing that SetWindowRgn produces. We accept that trade-off
/// in exchange for clean antialiased corners on Win11.
fn apply_dwm_round_corners(hwnd: HWND) {
    unsafe {
        use windows::Win32::Graphics::Dwm::*;
        let pref: i32 = DWMWCP_ROUND.0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

fn register_class() -> Result<()> {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(modal_wndproc),
            hInstance: hinstance,
            lpszClassName: MODAL_CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            // No GDI background brush — we paint the whole client area
            // with D2D each WM_PAINT.
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            let err = GetLastError();
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(Error::from_thread());
            }
        }
        Ok(())
    }
}

/// Number of wrapped lines a single-paragraph string will take at the
/// given inner width and average glyph DIPs. Always at least one line.
fn wrap_line_count(text: &str, inner_w: f32, glyph_w: f32) -> usize {
    if inner_w <= 0.0 || glyph_w <= 0.0 {
        return 1;
    }
    let chars_per_line = (inner_w / glyph_w).floor().max(1.0) as usize;
    // Honour hard line breaks too.
    let mut total = 0usize;
    for piece in text.split('\n') {
        let n = piece.chars().count().max(1);
        total += n.div_ceil(chars_per_line);
    }
    total.max(1)
}

/// Total modal height in DIPs for the given spec at the given DPI.
fn total_height(spec: &ModalSpec, _dpi: u32) -> f32 {
    let inner_w = spec.width - PAD * 2.0;
    let title_lines = wrap_line_count(&spec.title, inner_w, AVG_TITLE_GLYPH_W);
    let mut h = PAD + (title_lines as f32) * TITLE_LINE_H + PAD;
    if spec.edit_default.is_some() {
        let edit_h = if spec.multiline { EDIT_H_MULTILINE } else { EDIT_H };
        h += edit_h + PAD;
    } else if let Some(body) = &spec.body {
        let body_lines = wrap_line_count(body, inner_w, AVG_BODY_GLYPH_W);
        h += (body_lines as f32) * BODY_LINE_H + PAD;
    }
    h += BTN_H + PAD;
    h
}

/// Y offset (DIPs) where the EDIT control / body section starts.
pub(super) fn body_y(spec: &ModalSpec) -> f32 {
    let inner_w = spec.width - PAD * 2.0;
    let title_lines = wrap_line_count(&spec.title, inner_w, AVG_TITLE_GLYPH_W);
    PAD + (title_lines as f32) * TITLE_LINE_H + PAD
}

fn edit_rect_px(spec: &ModalSpec, dpi: u32) -> RECT {
    let pad_px = dip_to_px(PAD, dpi);
    let w_px = dip_to_px(spec.width, dpi);
    let edit_h_dip = if spec.multiline { EDIT_H_MULTILINE } else { EDIT_H };
    let edit_h_px = dip_to_px(edit_h_dip, dpi);
    let inner_pad_px = dip_to_px(8.0, dpi);
    let edit_y_px = dip_to_px(body_y(spec), dpi);
    RECT {
        left: pad_px + inner_pad_px,
        top: edit_y_px + inner_pad_px / 2,
        right: w_px - pad_px - inner_pad_px,
        bottom: edit_y_px + edit_h_px - inner_pad_px / 2,
    }
}

fn build_render_target(hwnd: HWND, dpi: u32) -> Result<(ID2D1HwndRenderTarget, IDWriteFactory)> {
    unsafe {
        let factory: ID2D1Factory =
            D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let size = D2D_SIZE_U {
            width: (rc.right - rc.left) as u32,
            height: (rc.bottom - rc.top) as u32,
        };
        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: size,
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let rt = factory.CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
        Ok((rt, dwrite))
    }
}

fn create_segoe_font(dpi: u32, dip_size: i32) -> HFONT {
    unsafe {
        let height = -dip_to_px(dip_size as f32, dpi);
        let mut name = [0u16; 32];
        for (i, c) in "Segoe UI".encode_utf16().enumerate() {
            name[i] = c;
        }
        let lf = LOGFONTW {
            lfHeight: height,
            lfWeight: 400,
            lfCharSet: DEFAULT_CHARSET,
            lfOutPrecision: OUT_TT_PRECIS,
            lfClipPrecision: CLIP_DEFAULT_PRECIS,
            lfQuality: CLEARTYPE_QUALITY,
            lfPitchAndFamily: 0,
            lfFaceName: name,
            ..Default::default()
        };
        CreateFontIndirectW(&lf)
    }
}

unsafe extern "system" fn modal_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ModalState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let st = &mut *state_ptr;

    match msg {
        WM_NCHITTEST => {
            // Top strip drags the window like a title bar; the rest is
            // client (button hovers, EDIT focus, …).
            let mut pt = POINT {
                x: (lparam.0 as i32) as i16 as i32,
                y: ((lparam.0 as i32) >> 16) as i16 as i32,
            };
            let _ = ScreenToClient(hwnd, &mut pt);
            let dpi = crate::fence_window::window_dpi(hwnd);
            let drag_h_px = dip_to_px(DRAG_STRIP_H, dpi);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            // Don't grab clicks on the buttons (bottom strip).
            let btn_top_px = rect.bottom - dip_to_px(PAD + BTN_H, dpi);
            if pt.y >= 0 && pt.y < drag_h_px && pt.y < btn_top_px {
                return LRESULT(HTCAPTION as isize);
            }
            return LRESULT(HTCLIENT as isize);
        }
        WM_MOUSEMOVE => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tme);
            let new_hover = hit_test_button(st, hwnd, lx, ly);
            if new_hover != st.hover_btn {
                st.hover_btn = new_hover;
                invalidate_buttons(hwnd);
            }
        }
        m if m == WM_MOUSELEAVE_U && st.hover_btn != -1 => {
            st.hover_btn = -1;
            invalidate_buttons(hwnd);
        }
        WM_LBUTTONDOWN => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            let idx = hit_test_button(st, hwnd, lx, ly);
            if idx >= 0 {
                st.pressed_btn = idx;
                SetCapture(hwnd);
                invalidate_buttons(hwnd);
            }
        }
        WM_LBUTTONUP => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            let idx = hit_test_button(st, hwnd, lx, ly);
            let pressed = st.pressed_btn;
            st.pressed_btn = -1;
            let _ = ReleaseCapture();
            if idx == pressed && idx >= 0 {
                st.result = st.spec.buttons[idx as usize].result;
                st.done = true;
            } else {
                invalidate_buttons(hwnd);
            }
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            SetTextColor(hdc, COLORREF(0x00F0F0F0));
            SetBkColor(hdc, COLORREF(0x00181818));
            return LRESULT(st.edit_bg_brush.0 as isize);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let _ = render::render(hwnd);
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        WM_SIZE => {
            if let Some(rt) = &st.rt {
                let w = (lparam.0 as u32) & 0xFFFF;
                let h = ((lparam.0 as u32) >> 16) & 0xFFFF;
                let _ = rt.Resize(&D2D_SIZE_U {
                    width: w,
                    height: h,
                });
            }
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 as u32) & 0xFFFF;
            let suggested = lparam.0 as *const RECT;
            if !suggested.is_null() {
                let r = &*suggested;
                let w = r.right - r.left;
                let h = r.bottom - r.top;
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                // DWM corner preference is sticky across resizes but a
                // DPI change can recreate the window swap chain; reassert.
                apply_dwm_round_corners(hwnd);
            }
            if let Some(rt) = &st.rt {
                rt.SetDpi(new_dpi as f32, new_dpi as f32);
            }
            if let Some(edit) = st.edit_hwnd {
                let r = edit_rect_px(&st.spec, new_dpi);
                let _ = SetWindowPos(
                    edit,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let hfont = create_segoe_font(new_dpi, 14);
                SendMessageW(
                    edit,
                    WM_SETFONT,
                    Some(WPARAM(hfont.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            let _ = render::render(hwnd);
            return LRESULT(0);
        }
        WM_CLOSE => {
            if let Some(idx) = st.spec.buttons.iter().position(|b| b.cancel) {
                st.result = st.spec.buttons[idx].result;
            }
            st.done = true;
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Invalidate just the bottom strip so a hover state change doesn't
/// repaint the whole modal (mainly: don't trash the EDIT control).
fn invalidate_buttons(hwnd: HWND) {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let dpi = crate::fence_window::window_dpi(hwnd);
        let strip_top = rect.bottom - dip_to_px(PAD + BTN_H + PAD * 0.25, dpi);
        let r = RECT {
            left: 0,
            top: strip_top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let _ = InvalidateRect(Some(hwnd), Some(&r), false);
    }
}

fn hit_test_button(st: &ModalState, hwnd: HWND, lx: i32, ly: i32) -> i32 {
    let dpi = crate::fence_window::window_dpi(hwnd);
    let buttons = &st.spec.buttons;
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    let w_px = rect.right;
    let h_px = rect.bottom;
    let pad_px = dip_to_px(PAD, dpi);
    let btn_w_px = dip_to_px(BTN_W, dpi);
    let btn_h_px = dip_to_px(BTN_H, dpi);
    let btn_gap_px = dip_to_px(super::BTN_GAP, dpi);
    let btn_top = h_px - pad_px - btn_h_px;
    let mut bx = w_px - pad_px;
    for (i, _) in buttons.iter().enumerate() {
        let right = bx;
        let left = bx - btn_w_px;
        if lx >= left && lx < right && ly >= btn_top && ly < btn_top + btn_h_px {
            return i as i32;
        }
        bx = left - btn_gap_px;
    }
    -1
}
