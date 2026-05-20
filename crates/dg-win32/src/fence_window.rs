use dg_core::fence::{Fence, FenceItem};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Ole::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::drop_target::FenceDropTarget;
use crate::layout::IconLayout;
use crate::render::{D2DContext, DragHint, draw_fence};
use crate::shortcut::resolve_lnk;
use windows::Win32::UI::HiDpi::*;

pub const FENCE_CLASS_NAME: PCWSTR = w!("DG_FENCE_CLASS");
// Title bar height in logical DIPs. Multiplied by dpi/96 for physical px.
pub const TITLE_H_DIP: i32 = 32;

#[inline]
fn dip_to_px(dip: f64, dpi: u32) -> i32 {
    (dip * dpi as f64 / 96.0).round() as i32
}

#[inline]
fn px_to_dip(px: i32, dpi: u32) -> f64 {
    px as f64 * 96.0 / dpi as f64
}

/// Read the per-monitor DPI for an HWND, falling back to 96 if the API
/// isn't available (e.g. pre-Win10 1607).
pub fn window_dpi(hwnd: HWND) -> u32 {
    unsafe {
        let d = GetDpiForWindow(hwnd);
        if d == 0 { 96 } else { d }
    }
}

/// Title-bar height for this window in physical pixels.
#[inline]
fn title_h_px(hwnd: HWND) -> i32 {
    dip_to_px(TITLE_H_DIP as f64, window_dpi(hwnd))
}

/// True when client x-coord `lx_px` falls on the painted title text
/// (not the empty space to its right). Mirrors the geometry used in
/// `draw_fence`: title is left-aligned at 10 DIPs from the left edge.
fn title_text_hit(hwnd: HWND, lx_px: i32) -> bool {
    const TITLE_LEFT_DIP: f64 = 10.0;
    // Generous trailing slop so a click landing just past the glyphs
    // still counts as "on the text" — feels less finicky than a strict
    // measure boundary.
    const TRAILING_SLOP_DIP: f64 = 6.0;

    let dpi = window_dpi(hwnd);
    let left_px = dip_to_px(TITLE_LEFT_DIP, dpi);
    if lx_px < left_px {
        return false;
    }
    unsafe {
        crate::app::with_state_mut(|s| {
            let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) else {
                return false;
            };
            let title = fw.fence_data.title.clone();
            let bold = fw.fence_data.bold_title_text == "true";
            let size = match fw.fence_data.title_text_size.as_str() {
                "Small" => 11.0,
                "Large" => 15.0,
                _ => 13.0,
            };
            let text_w_dip = fw.d2d.measure_text_width(&title, size, bold).unwrap_or(0.0) as f64;
            let right_px = dip_to_px(TITLE_LEFT_DIP + text_w_dip + TRAILING_SLOP_DIP, dpi);
            lx_px < right_px
        })
        .unwrap_or(false)
    }
}

// Right-click menu IDs (fence/icon scope). Use 2000+ to avoid tray collision.
const ID_FENCE_ROLL: usize = 2001;
const ID_FENCE_RENAME: usize = 2002;
const ID_FENCE_DELETE: usize = 2003;
const ID_FENCE_LOCK_TOGGLE: usize = 2004;
const ID_ITEM_OPEN: usize = 2010;
const ID_ITEM_REMOVE: usize = 2011;
const ID_ITEM_OPEN_LOCATION: usize = 2012;

// Customize submenu IDs. Encoding (id_base + kind * 64 + value) is
// shared with the tray's "Default fence settings" menu — see
// crate::customize for the layout. Kept here because it's the per-fence
// base and the per-fence dispatcher matches against it.
pub const ID_CUSTOMIZE_BASE: usize = 3000;

// One-shot menu item for editing the blur radius. Uses an input dialog
// instead of a preset list so the user can dial any value in 0..=150.
const ID_FENCE_BLUR_RADIUS: usize = 2005;

// Global animation FPS presets. ID base is consumed by tray.rs (the
// menu now lives in the tray, not per-fence). Kept here because the
// preset list and the labels are shared visual constants.
pub const ANIM_FPS_PRESETS: &[(i32, &windows::core::PCWSTR)] = &[
    (0, &w!("Off (snap)")),
    (30, &w!("30 FPS")),
    (60, &w!("60 FPS (default)")),
    (90, &w!("90 FPS")),
    (120, &w!("120 FPS")),
    (144, &w!("144 FPS")),
    (240, &w!("240 FPS")),
];

pub fn register_class() -> windows::core::Result<()> {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(fence_wndproc),
        hInstance: unsafe { GetModuleHandleW(None)?.into() },
        lpszClassName: FENCE_CLASS_NAME,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        ..Default::default()
    };
    unsafe {
        if RegisterClassW(&wc) == 0 {
            let err = GetLastError();
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(Error::from_thread());
            }
        }
    }
    Ok(())
}

pub struct FenceWindow {
    pub hwnd: HWND,
    pub fence_data: Fence,
    pub d2d: D2DContext,
    _drop_target: Option<IDropTarget>,
    // Press registered on an icon but not yet promoted to a drag. Stores
    // (icon index, press_x_px, press_y_px). On WM_MOUSEMOVE we compare
    // the cursor delta against SM_CXDRAG/SM_CYDRAG to decide whether to
    // promote into `drag_render` (real drag) or leave as-is (still a
    // click candidate launched on WM_LBUTTONUP).
    pub drag_pending: Option<(usize, i32, i32)>,
    // Active reorder drag. Holds enough state to render the dragged icon
    // following the cursor and animate the displaced siblings into their
    // new slots. None when no drag is in progress.
    pub drag_render: Option<DragRenderState>,
    pub anim: Option<AnimState>,
    // Session-only toggle for the "double-click whitespace" Z-order gesture:
    // false = next double-click promotes to top, true = next demotes to bottom.
    // Not persisted; resets on app restart.
    pub z_promoted: bool,
}

/// State for an active icon-reorder drag. Lives on `FenceWindow` for the
/// duration of the gesture (press past threshold → release). All
/// positions in DIPs and relative to the fence's client area.
pub struct DragRenderState {
    pub src: usize,
    // Most recent cursor position in client DIPs.
    pub cursor_dip: (f32, f32),
    // Offset from the source cell's top-left to the cursor at press
    // time. Subtracting this from the live cursor gives where to draw
    // the floating cell so the icon stays grabbed at the same point.
    pub grab_offset: (f32, f32),
    // Currently hovered insertion target (item index), if any.
    pub target: Option<usize>,
    // Per-item slot index at `anim_start_tick`. The renderer lerps from
    // these toward the target layout's slots using `anim_start_tick`
    // and `DRAG_ANIM_MS`. When `target` changes we snapshot the
    // currently-interpolated positions back into this Vec so the new
    // animation starts from wherever the icons happen to be — no jumps.
    pub from_slots: Vec<f32>,
    pub anim_start_tick: u32,
    // Snapshot of the global animation FPS at the moment this drag
    // started. 0 = no animation (slots snap to target instantly, no
    // timer); >0 = animated with a 1000/fps timer tick. Snapshotted
    // (not re-read) so a mid-drag settings change can't break the
    // running timer assumption — the next drag picks up the new value.
    pub anim_fps: i32,
}

pub struct AnimState {
    pub start_tick: u32,
    pub duration_ms: u32,
    pub start_h: i32,
    pub target_h: i32,
}

const TIMER_ID_ANIM: usize = 1;
const ROLL_ANIM_MS: u32 = 120;
const TIMER_ID_DRAG: usize = 2;
const DRAG_ANIM_MS: u32 = 180;

/// Where item `i` would visually sit if the dragged item (`src`) were
/// inserted at `target`. Pure layout maths — no drag context needed.
/// `target == None` (no current insertion target) means everyone stays
/// at their natural slot.
fn slot_of(i: usize, src: usize, target: Option<usize>) -> f32 {
    let Some(t) = target else {
        return i as f32;
    };
    if i == src {
        return t as f32;
    }
    if src < t {
        if i > src && i <= t {
            (i - 1) as f32
        } else {
            i as f32
        }
    } else if src > t {
        if i >= t && i < src {
            (i + 1) as f32
        } else {
            i as f32
        }
    } else {
        i as f32
    }
}

impl DragRenderState {
    /// Bootstrap state at the moment a press is promoted into a real
    /// drag. `from_slots` starts at the natural layout so the very first
    /// animation eases from "everything in place" toward whatever the
    /// first observed target is.
    fn new(
        src: usize,
        cursor_dip: (f32, f32),
        grab_offset: (f32, f32),
        target: Option<usize>,
        items_len: usize,
        now: u32,
        anim_fps: i32,
    ) -> Self {
        Self {
            src,
            cursor_dip,
            grab_offset,
            target,
            from_slots: (0..items_len).map(|i| i as f32).collect(),
            anim_start_tick: now,
            anim_fps,
        }
    }

    /// Lerped per-item slot indices to feed `DragHint::item_slots`.
    /// Linear interp on the slot scale; `slot_of` semantics handle the
    /// "src vacates, others shift" behaviour. When `anim_fps == 0` the
    /// caller has chosen "no animation": skip the lerp and snap straight
    /// to target slots so each render shows the final layout.
    fn current_slots(&self, now: u32, items_len: usize) -> Vec<f32> {
        if self.anim_fps <= 0 {
            return (0..items_len)
                .map(|i| slot_of(i, self.src, self.target))
                .collect();
        }
        let t = ease_out_cubic(self.anim_t(now));
        (0..items_len)
            .map(|i| {
                let from = self.from_slots.get(i).copied().unwrap_or(i as f32);
                let to = slot_of(i, self.src, self.target);
                from + (to - from) * t
            })
            .collect()
    }

    /// Switch insertion target. Snapshots the currently-interpolated
    /// slots into `from_slots` before changing `target` so the next
    /// animation eases from the visible positions instead of teleporting.
    fn set_target(&mut self, new_target: Option<usize>, items_len: usize, now: u32) {
        if new_target == self.target {
            return;
        }
        self.from_slots = self.current_slots(now, items_len);
        self.target = new_target;
        self.anim_start_tick = now;
    }

    fn anim_t(&self, now: u32) -> f32 {
        let elapsed = now.wrapping_sub(self.anim_start_tick);
        (elapsed as f32 / DRAG_ANIM_MS as f32).clamp(0.0, 1.0)
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Convert a chosen animation FPS into the `SetTimer` interval in
/// milliseconds. Returns `None` when fps is 0 or negative — caller
/// should skip `SetTimer` entirely (no animation, snap to end state).
/// Clamped to a sane band so a tiny fps doesn't drift past
/// USER_TIMER_MAXIMUM and a huge fps doesn't pin a CPU.
fn anim_timer_interval(fps: i32) -> Option<u32> {
    if fps <= 0 {
        return None;
    }
    let ms = (1000 / fps).clamp(1, 100) as u32;
    Some(ms)
}

impl FenceWindow {
    pub fn create(fence_data: &Fence) -> windows::core::Result<Self> {
        let hinstance = unsafe { GetModuleHandleW(None)?.into() };

        // Position is stored in physical pixels (where the user dropped it
        // previously) but width/height are logical DIPs. Convert to physical
        // pixels for CreateWindowEx; the actual DPI is read after the window
        // exists, so use the position's monitor as a best-effort proxy.
        let x = fence_data.x as i32;
        let y = fence_data.y as i32;
        // Read the DPI of the monitor under the saved position so the very
        // first frame is the right size on the user's actual display.
        let initial_dpi = unsafe {
            use windows::Win32::Graphics::Gdi::*;
            let pt = POINT { x, y };
            let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut x_dpi = 96u32;
            let mut y_dpi = 96u32;
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x_dpi, &mut y_dpi);
            if x_dpi == 0 { 96 } else { x_dpi }
        };
        let w = dip_to_px(fence_data.width, initial_dpi);
        let h = if fence_data.is_rolled == "true" {
            dip_to_px(TITLE_H_DIP as f64, initial_dpi)
        } else {
            dip_to_px(fence_data.height, initial_dpi)
        };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
                FENCE_CLASS_NAME,
                w!(""),
                WS_POPUP | WS_CLIPCHILDREN,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(hinstance),
                None,
            )?
        };

        let actual_dpi = window_dpi(hwnd);
        let mut d2d = D2DContext::create(hwnd)?;
        d2d.set_dpi(actual_dpi);

        // Apply the per-fence blur preference before the first render so the
        // backdrop layer is in place when the window first paints. Goes
        // through the blur module so the WCA_ACCENT_POLICY opt-in (required
        // for HostBackdropBrush to actually receive the wallpaper) fires too.
        let blur_on = fence_data.blur_enabled == "true";
        crate::blur::enable_host_backdrop(hwnd, blur_on);
        if let Err(e) = d2d.set_blur_radius(fence_data.blur_radius as f32) {
            eprintln!(
                "[dg] set_blur_radius({}) failed: {:?}",
                fence_data.blur_radius, e
            );
        }
        if let Err(e) = d2d.set_blur_enabled(blur_on) {
            eprintln!("[dg] set_blur_enabled({}) failed: {:?}", blur_on, e);
        }

        let drop_target = FenceDropTarget::new(hwnd);
        let dt_for_register = drop_target.clone();
        unsafe {
            let _ = RegisterDragDrop(hwnd, &dt_for_register);
        }

        let mut fw = FenceWindow {
            hwnd,
            fence_data: fence_data.clone(),
            d2d,
            _drop_target: Some(drop_target),
            drag_pending: None,
            drag_render: None,
            anim: None,
            z_promoted: false,
        };

        fw.render()?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(fw)
    }

    pub fn render(&mut self) -> windows::core::Result<()> {
        let drag = self.drag_render.as_ref().map(|d| {
            let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
            let floating_dip = (
                d.cursor_dip.0 - d.grab_offset.0,
                d.cursor_dip.1 - d.grab_offset.1,
            );
            let item_slots = d.current_slots(now, self.fence_data.items.len());
            DragHint {
                src: d.src,
                floating_dip,
                item_slots,
            }
        });
        draw_fence(&mut self.d2d, &self.fence_data, drag)?;
        Ok(())
    }

    pub fn hit_test_icon(&self, lx_px: i32, ly_px: i32) -> Option<usize> {
        if self.fence_data.is_rolled == "true" {
            return None;
        }
        // Layout maths live in DIPs to match draw_fence; convert the click
        // point (which Windows hands us in physical client pixels) first.
        let dpi = self.d2d.dpi;
        let lxf = px_to_dip(lx_px, dpi) as f32;
        let lyf = px_to_dip(ly_px, dpi) as f32;
        IconLayout::from_fence(&self.fence_data).hit(lxf, lyf, self.fence_data.items.len())
    }

    fn toggle_rolled(&mut self, anim_fps: i32) -> windows::core::Result<()> {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut rect);
        }
        let start_h = rect.bottom - rect.top;
        let dpi = self.d2d.dpi;

        let target_h = if self.fence_data.is_rolled == "true" {
            self.fence_data.is_rolled = "false".into();
            dip_to_px(self.fence_data.unrolled_height, dpi)
        } else {
            self.fence_data.is_rolled = "true".into();
            self.fence_data.unrolled_height = self.fence_data.height;
            dip_to_px(TITLE_H_DIP as f64, dpi)
        };

        // Animation off → resize in one shot and skip the timer. Keeps
        // the same final state (height, is_rolled) as the animated path.
        let Some(interval) = anim_timer_interval(anim_fps) else {
            let w = dip_to_px(self.fence_data.width, dpi);
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    w,
                    target_h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let _ = self.render();
            return Ok(());
        };

        self.anim = Some(AnimState {
            start_tick: unsafe { windows::Win32::System::SystemInformation::GetTickCount() },
            duration_ms: ROLL_ANIM_MS,
            start_h,
            target_h,
        });
        unsafe {
            let _ = SetTimer(Some(self.hwnd), TIMER_ID_ANIM, interval, None);
        }
        Ok(())
    }
}

impl Drop for FenceWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
            if !self.hwnd.is_invalid() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

const MA_NOACTIVATE: u32 = 3;

unsafe extern "system" fn fence_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // Override the system cursor to the 4-way "move" arrow once a
            // drag has been confirmed, so the user sees the reorder mode.
            // Anything else (resize grips, normal idle) keeps the OS default.
            let drag_active = unsafe {
                crate::app::with_state(|s| {
                    s.fences
                        .iter()
                        .find(|f| f.hwnd == hwnd)
                        .map(|fw| fw.drag_render.is_some())
                })
                .flatten()
                .unwrap_or(false)
            };
            if drag_active {
                unsafe {
                    if let Ok(c) = LoadCursorW(None, IDC_SIZEALL) {
                        let _ = SetCursor(Some(c));
                    }
                }
                return LRESULT(1);
            }
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ID_ANIM {
                tick_animation(hwnd);
                return LRESULT(0);
            }
            if wparam.0 == TIMER_ID_DRAG {
                tick_drag(hwnd);
                return LRESULT(0);
            }
        }
        WM_SYSCOMMAND => {
            let cmd = (wparam.0) & 0xFFF0;
            if cmd == (SC_MAXIMIZE as usize) || cmd == (SC_RESTORE as usize) {
                return LRESULT(0);
            }
        }
        WM_NCHITTEST => return handle_nchittest(hwnd, lparam),
        WM_MOVING => unsafe {
            apply_snap(hwnd, lparam);
        },
        WM_LBUTTONDOWN => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            // MK_CONTROL = 0x0008 (not exposed by current windows feature set).
            const MK_CONTROL: u32 = 0x0008;
            let ctrl_held = (wparam.0 as u32) & MK_CONTROL != 0;

            if ctrl_held && ly < title_h_px(hwnd) {
                // Ctrl+Click on title -> rename inline.
                rename_fence_via_modal(hwnd);
                return LRESULT(0);
            }

            // Press on an icon → record as pending. We don't know yet
            // whether this is a click (launch) or a drag (reorder); the
            // decision is made in WM_MOUSEMOVE once the cursor moves
            // past the system drag threshold. Capture the mouse so we
            // see the eventual WM_LBUTTONUP even if it leaves the window.
            let begun = unsafe {
                crate::app::with_state_mut(|s| {
                    if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd)
                        && let Some(idx) = fw.hit_test_icon(lx, ly)
                    {
                        fw.drag_pending = Some((idx, lx, ly));
                        return true;
                    }
                    false
                })
                .unwrap_or(false)
            };
            if begun {
                unsafe {
                    SetCapture(hwnd);
                }
                return LRESULT(0);
            }
        }
        WM_MOUSEMOVE => {
            // Two roles depending on per-fence drag state:
            //   1. drag_pending → not yet a drag. Promote into drag_render
            //      once the cursor has moved past SM_CXDRAG / SM_CYDRAG;
            //      compute grab_offset so the floating icon sticks at
            //      the exact spot the user grabbed it, and start the
            //      TIMER_ID_DRAG ticker that drives the displacement
            //      animation at ~60 fps. No render here — the timer
            //      paints the first frame on its next tick.
            //   2. drag_render active → just refresh cursor + target.
            //      `set_target` snapshots the in-flight slot positions
            //      so a new animation eases from "wherever icons are
            //      right now" instead of teleporting.
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            unsafe {
                crate::app::with_state_mut(|s| {
                    let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) else {
                        return;
                    };
                    let dpi = fw.d2d.dpi;
                    let cursor_dip = (px_to_dip(lx, dpi) as f32, px_to_dip(ly, dpi) as f32);

                    if fw.drag_render.is_some() {
                        let new_target = fw.hit_test_icon(lx, ly);
                        let items_len = fw.fence_data.items.len();
                        let now = windows::Win32::System::SystemInformation::GetTickCount();
                        let mut needs_render = false;
                        if let Some(d) = fw.drag_render.as_mut() {
                            d.cursor_dip = cursor_dip;
                            d.set_target(new_target, items_len, now);
                            // No timer when anim is off — drive the
                            // paint from this message instead so the
                            // floating icon still tracks the cursor.
                            needs_render = d.anim_fps <= 0;
                        }
                        if needs_render {
                            let _ = fw.render();
                        }
                        return;
                    }

                    if let Some((idx, sx, sy)) = fw.drag_pending {
                        let thr_x = GetSystemMetrics(SM_CXDRAG).max(1);
                        let thr_y = GetSystemMetrics(SM_CYDRAG).max(1);
                        if (lx - sx).abs() >= thr_x || (ly - sy).abs() >= thr_y {
                            fw.drag_pending = None;
                            let press_dip = (px_to_dip(sx, dpi) as f32, px_to_dip(sy, dpi) as f32);
                            let (cell_x, cell_y) =
                                IconLayout::from_fence(&fw.fence_data).cell_top_left(idx);
                            let grab_offset = (press_dip.0 - cell_x, press_dip.1 - cell_y);
                            let target = fw.hit_test_icon(lx, ly);
                            let items_len = fw.fence_data.items.len();
                            let now = windows::Win32::System::SystemInformation::GetTickCount();
                            let anim_fps = s.config.settings.anim_fps;
                            fw.drag_render = Some(DragRenderState::new(
                                idx,
                                cursor_dip,
                                grab_offset,
                                target,
                                items_len,
                                now,
                                anim_fps,
                            ));
                            // Animation off → no timer; the next
                            // WM_MOUSEMOVE re-renders. With animation
                            // on, kick the 1000/fps tick that drives
                            // the displacement lerp + floating icon.
                            if let Some(interval) = anim_timer_interval(anim_fps) {
                                let _ = SetTimer(Some(hwnd), TIMER_ID_DRAG, interval, None);
                            }
                            // Paint the very first frame so the user
                            // sees the icon lift off immediately,
                            // regardless of whether a timer is running.
                            let _ = fw.render();
                        }
                    }
                });
            }
        }
        WM_LBUTTONUP => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;

            // Resolve the press into one of three outcomes inside a single
            // borrow: confirmed drag → do the reorder right here; ambiguous
            // press that ended without moving past the threshold → return
            // the clicked item so we can launch it after dropping the
            // borrow (launch_item is a free fn, can't run under with_state);
            // anything else → no-op.
            let to_launch = unsafe {
                crate::app::with_state_mut(|s| {
                    let fw = s.fences.iter_mut().find(|f| f.hwnd == hwnd)?;
                    if fw.drag_render.is_some() || fw.drag_pending.is_some() {
                        let _ = ReleaseCapture();
                    }
                    if let Some(d) = fw.drag_render.take() {
                        let _ = KillTimer(Some(hwnd), TIMER_ID_DRAG);
                        fw.drag_pending = None;
                        let src = d.src;
                        let dst = fw.hit_test_icon(lx, ly).unwrap_or(src);
                        if dst != src && src < fw.fence_data.items.len() {
                            let item = fw.fence_data.items.remove(src);
                            let dst_clamped = dst.min(fw.fence_data.items.len());
                            fw.fence_data.items.insert(dst_clamped, item);
                            for (i, it) in fw.fence_data.items.iter_mut().enumerate() {
                                it.display_order = i as i32;
                            }
                            fw.d2d.icon_cache.invalidate();
                            let _ = fw.render();
                            let id = fw.fence_data.id.clone();
                            let items = fw.fence_data.items.clone();
                            if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                                cf.items = items;
                            }
                            let _ = s.config.save_fences();
                        } else {
                            // No reorder — but the floating ghost was on
                            // screen until this instant, so repaint to
                            // restore the natural layout.
                            let _ = fw.render();
                        }
                        return None;
                    }
                    if let Some((src_idx, _, _)) = fw.drag_pending.take() {
                        // Only count as a click when the release lands on
                        // the same icon the press started on — sliding off
                        // and releasing elsewhere shouldn't launch anything.
                        if fw.hit_test_icon(lx, ly) == Some(src_idx) {
                            return fw.fence_data.items.get(src_idx).cloned();
                        }
                    }
                    None
                })
                .flatten()
            };

            if let Some(item) = to_launch {
                launch_item(&item);
            }
        }
        WM_LBUTTONDBLCLK => {
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            if ly < title_h_px(hwnd) {
                unsafe {
                    crate::app::with_state_mut(|s| {
                        let anim_fps = s.config.settings.anim_fps;
                        if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                            let _ = fw.toggle_rolled(anim_fps);
                            let _ = s.config.save_fences();
                        }
                    });
                };
            }
        }
        WM_NCLBUTTONDOWN if wparam.0 as u32 == HTCAPTION => {
            // Title bar is reported as HTCAPTION by WM_NCHITTEST (when the
            // fence is unlocked), so a Ctrl+click on the title arrives
            // here, NOT in WM_LBUTTONDOWN. We intercept only the Ctrl
            // case and let everything else fall through to DefWindowProc
            // so normal drag-to-move still works.
            let ctrl_held = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16) & 0x8000 != 0 };
            if ctrl_held {
                rename_fence_via_modal(hwnd);
                return LRESULT(0);
            }
        }
        WM_NCLBUTTONDBLCLK => {
            // The title bar is reported as HTCAPTION by WM_NCHITTEST so
            // double-clicks on it arrive here, not WM_LBUTTONDBLCLK.
            // Behaviour:
            //   - double-click on the title text  → toggle rolled state
            //   - double-click on title whitespace → toggle Z-order:
            //       first DC promotes to the top of the desktop band,
            //       second DC pushes back down to the bottom.
            //
            // Coordinates from WM_NCLBUTTONDBLCLK are in *screen* space.
            let sx = (lparam.0 as i32) as i16 as i32;
            let sy = ((lparam.0 as i32) >> 16) as i16 as i32;
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            }
            let lx = sx - rect.left;
            let ly = sy - rect.top;
            if ly >= 0 && ly < title_h_px(hwnd) {
                let on_text = title_text_hit(hwnd, lx);
                // Flip the per-fence toggle and remember which direction
                // we should move in *before* dropping the borrow, so the
                // SetWindowPos call below knows what to do.
                let promote = unsafe {
                    crate::app::with_state_mut(|s| {
                        let anim_fps = s.config.settings.anim_fps;
                        let fw = s.fences.iter_mut().find(|f| f.hwnd == hwnd)?;
                        if on_text {
                            let _ = fw.toggle_rolled(anim_fps);
                            let _ = s.config.save_fences();
                            None
                        } else {
                            fw.z_promoted = !fw.z_promoted;
                            Some(fw.z_promoted)
                        }
                    })
                    .flatten()
                };
                if let Some(promote) = promote {
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                    unsafe {
                        if promote {
                            // Raising a WS_EX_NOACTIVATE popup over its
                            // siblings: BringWindowToTop / SetWindowPos
                            // (HWND_TOP) both silently no-op for non-
                            // activating windows. The documented trick
                            // is the TOPMOST→NOTOPMOST shuffle: briefly
                            // promoting into the topmost band forces the
                            // window manager to re-insert it, and the
                            // immediate demotion drops it back to the
                            // regular band at the very top.
                            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                        } else {
                            // Push back down. HWND_BOTTOM works directly
                            // for non-activating popups — no topmost
                            // shuffle required.
                            let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, flags);
                        }
                    }
                }
                return LRESULT(0);
            }
        }
        WM_RBUTTONUP => {
            let lx = (lparam.0 as i32) as i16 as i32;
            let ly = ((lparam.0 as i32) >> 16) as i16 as i32;
            handle_context_menu(hwnd, lx, ly);
        }
        WM_NCRBUTTONUP => {
            // Right-click on the title (non-client area).
            let sx = (lparam.0 as i32) as i16 as i32;
            let sy = ((lparam.0 as i32) >> 16) as i16 as i32;
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            }
            let lx = sx - rect.left;
            let ly = sy - rect.top;
            handle_context_menu(hwnd, lx, ly);
        }
        WM_SIZE => {
            let new_w_px = (lparam.0 as u32) & 0xFFFF;
            let new_h_px = ((lparam.0 as u32) >> 16) & 0xFFFF;
            unsafe {
                crate::app::with_state_mut(|s| {
                    if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                        // Roll animation drives its own render via WM_TIMER.
                        if fw.anim.is_some() {
                            return;
                        }
                        let dpi = fw.d2d.dpi;
                        fw.fence_data.width = px_to_dip(new_w_px as i32, dpi);
                        if fw.fence_data.is_rolled != "true" {
                            fw.fence_data.height = px_to_dip(new_h_px as i32, dpi);
                        }
                        let _ = fw.render();
                    }
                });
            }
        }
        WM_DPICHANGED => {
            // Windows sends a suggested new rect in lparam so the window
            // keeps the same logical size on the new monitor. wparam packs
            // (yDpi << 16) | xDpi; X and Y are always equal in practice.
            let new_dpi = (wparam.0 as u32) & 0xFFFF;
            unsafe {
                let suggested = lparam.0 as *const RECT;
                if !suggested.is_null() {
                    let r = &*suggested;
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                crate::app::with_state_mut(|s| {
                    if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                        fw.d2d.set_dpi(new_dpi);
                        let _ = fw.render();
                    }
                });
            }
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            }
            unsafe {
                crate::app::with_state_mut(|s| {
                    if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                        let dpi = fw.d2d.dpi;
                        // Position stays in screen-physical pixels (it's
                        // monitor-relative); only width/height live in DIPs.
                        fw.fence_data.x = rect.left as f64;
                        fw.fence_data.y = rect.top as f64;
                        fw.fence_data.width = px_to_dip(rect.right - rect.left, dpi);
                        if fw.fence_data.is_rolled != "true" {
                            fw.fence_data.height = px_to_dip(rect.bottom - rect.top, dpi);
                            fw.fence_data.unrolled_height = fw.fence_data.height;
                        }
                        let _ = fw.render();
                        let id = fw.fence_data.id.clone();
                        let fd = fw.fence_data.clone();
                        if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                            *cf = fd;
                        }
                        let _ = s.config.save_fences();
                    }
                });
            };
        }
        WM_DESTROY => return LRESULT(0),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn handle_nchittest(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let x = (lparam.0 as i32) as i16 as i32;
    let y = ((lparam.0 as i32) >> 16) as i16 as i32;

    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }

    let locked = unsafe {
        crate::app::with_state(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .map(|fw| fw.fence_data.is_locked == "true")
        })
        .flatten()
        .unwrap_or(false)
    };

    let dpi = window_dpi(hwnd);
    let border = dip_to_px(6.0, dpi);
    let grip_size = dip_to_px(16.0, dpi);
    let title_h = dip_to_px(TITLE_H_DIP as f64, dpi);
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    let lx = x - rect.left;
    let ly = y - rect.top;

    if !locked && lx > w - grip_size && ly > h - grip_size {
        return LRESULT(HTBOTTOMRIGHT as isize);
    }
    if ly >= 0 && ly < title_h {
        return if locked {
            LRESULT(HTCLIENT as isize)
        } else {
            LRESULT(HTCAPTION as isize)
        };
    }
    if !locked {
        if lx < border {
            return LRESULT(HTLEFT as isize);
        }
        if lx > w - border {
            return LRESULT(HTRIGHT as isize);
        }
        if ly < border {
            return LRESULT(HTTOP as isize);
        }
        if ly > h - border {
            return LRESULT(HTBOTTOM as isize);
        }
    }

    LRESULT(HTCLIENT as isize)
}

fn handle_context_menu(hwnd: HWND, lx: i32, ly: i32) {
    // Decide whether we're over an item.
    let item_idx = unsafe {
        crate::app::with_state(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .and_then(|fw| fw.hit_test_icon(lx, ly))
        })
        .flatten()
    };

    let mut screen_pt = POINT { x: lx, y: ly };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut screen_pt);
        // Deliberately NOT calling SetForegroundWindow here: it would
        // raise the fence above all its siblings just for showing a menu.
        // TrackPopupMenu handles menu activation on its own.
    }

    let id = unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        if let Some(_idx) = item_idx {
            let _ = AppendMenuW(menu, MF_STRING, ID_ITEM_OPEN, w!("Open"));
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_ITEM_OPEN_LOCATION,
                w!("Open file location"),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_ITEM_REMOVE, w!("Remove from fence"));
        } else {
            let fd = crate::app::with_state(|s| {
                s.fences
                    .iter()
                    .find(|f| f.hwnd == hwnd)
                    .map(|fw| fw.fence_data.clone())
            })
            .flatten();
            let roll_label = match fd.as_ref().map(|f| f.is_rolled == "true") {
                Some(true) => w!("Unroll"),
                _ => w!("Roll up"),
            };
            let _ = AppendMenuW(menu, MF_STRING, ID_FENCE_ROLL, roll_label);
            let _ = AppendMenuW(menu, MF_STRING, ID_FENCE_RENAME, w!("Rename..."));
            let lock_label = match fd.as_ref().map(|f| f.is_locked == "true") {
                Some(true) => w!("Unlock"),
                _ => w!("Lock"),
            };
            let _ = AppendMenuW(menu, MF_STRING, ID_FENCE_LOCK_TOGGLE, lock_label);

            if let Some(f) = fd.as_ref() {
                let customize = build_customize_menu(f);
                let _ = AppendMenuW(menu, MF_POPUP, customize.0 as usize, w!("Customize"));
            }

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_FENCE_DELETE, w!("Delete fence"));
        }

        let id = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            screen_pt.x,
            screen_pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        id
    };

    match id.0 as usize {
        ID_ITEM_OPEN => unsafe {
            crate::app::with_state(|s| {
                if let Some(fw) = s.fences.iter().find(|f| f.hwnd == hwnd)
                    && let Some(idx) = item_idx
                    && let Some(item) = fw.fence_data.items.get(idx)
                {
                    launch_item(item);
                }
            });
        },
        ID_ITEM_OPEN_LOCATION => unsafe {
            crate::app::with_state(|s| {
                if let Some(fw) = s.fences.iter().find(|f| f.hwnd == hwnd)
                    && let Some(idx) = item_idx
                    && let Some(item) = fw.fence_data.items.get(idx)
                {
                    open_in_explorer(&item.filename);
                }
            });
        },
        ID_ITEM_REMOVE => unsafe {
            crate::app::with_state_mut(|s| {
                if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd)
                    && let Some(idx) = item_idx
                    && idx < fw.fence_data.items.len()
                {
                    fw.fence_data.items.remove(idx);
                    fw.d2d.icon_cache.invalidate();
                    let _ = fw.render();
                    let id = fw.fence_data.id.clone();
                    let items = fw.fence_data.items.clone();
                    if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                        cf.items = items;
                    }
                    let _ = s.config.save_fences();
                }
            });
        },
        ID_FENCE_ROLL => unsafe {
            crate::app::with_state_mut(|s| {
                let anim_fps = s.config.settings.anim_fps;
                if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                    let _ = fw.toggle_rolled(anim_fps);
                    let id = fw.fence_data.id.clone();
                    let fd = fw.fence_data.clone();
                    if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                        *cf = fd;
                    }
                    let _ = s.config.save_fences();
                }
            });
        },
        ID_FENCE_RENAME => {
            rename_fence_via_modal(hwnd);
        }
        ID_FENCE_LOCK_TOGGLE => unsafe {
            crate::app::with_state_mut(|s| {
                if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                    fw.fence_data.is_locked = if fw.fence_data.is_locked == "true" {
                        "false".into()
                    } else {
                        "true".into()
                    };
                    let id = fw.fence_data.id.clone();
                    let locked = fw.fence_data.is_locked.clone();
                    if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                        cf.is_locked = locked;
                    }
                    let _ = s.config.save_fences();
                }
            });
        },
        ID_FENCE_DELETE => unsafe {
            let title = crate::app::with_state(|s| {
                s.fences
                    .iter()
                    .find(|f| f.hwnd == hwnd)
                    .map(|fw| fw.fence_data.title.clone())
            })
            .flatten()
            .unwrap_or_default();
            let instruction = if title.is_empty() {
                "Delete this fence?".to_string()
            } else {
                format!("Delete the fence \u{201C}{}\u{201D}?", title)
            };
            let result = crate::modal::confirm_destructive(
                hwnd,
                "DeskGate",
                &instruction,
                "Its contents are shortcuts only — the original files stay where they are. This cannot be undone from inside the app.",
                "Delete fence",
            );
            if result == crate::modal::ConfirmResult::Confirmed {
                crate::app::with_state_mut(|s| {
                    if let Some(pos) = s.fences.iter().position(|f| f.hwnd == hwnd) {
                        let id = s.fences[pos].fence_data.id.clone();
                        s.fences.remove(pos);
                        s.config.fences.retain(|f| f.id != id);
                        let _ = s.config.save_fences();
                    }
                });
            }
        },
        ID_FENCE_BLUR_RADIUS => {
            let current = unsafe {
                crate::app::with_state(|s| {
                    s.fences
                        .iter()
                        .find(|f| f.hwnd == hwnd)
                        .map(|fw| fw.fence_data.blur_radius)
                })
                .flatten()
                .unwrap_or(20.0)
            };
            let initial = format!("{}", current.round() as i32);
            if let Some(input) = crate::modal::input(hwnd, "Blur radius (0-150)", &initial) {
                let trimmed = input.trim();
                if let Ok(parsed) = trimmed.parse::<f64>() {
                    let radius = parsed.clamp(0.0, 150.0);
                    unsafe {
                        crate::app::with_state_mut(|s| {
                            if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                                fw.fence_data.blur_radius = radius;
                                let _ = fw.d2d.set_blur_radius(radius as f32);
                                let id = fw.fence_data.id.clone();
                                if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                                    cf.blur_radius = radius;
                                }
                                let _ = s.config.save_fences();
                            }
                        });
                    }
                }
            }
        }
        n if (ID_CUSTOMIZE_BASE
            ..ID_CUSTOMIZE_BASE + crate::customize::KIND_COUNT * crate::customize::KIND_STRIDE)
            .contains(&n) =>
        {
            apply_customize(hwnd, n - ID_CUSTOMIZE_BASE);
        }
        _ => {}
    }
}

fn build_customize_menu(f: &Fence) -> HMENU {
    let view = crate::customize::CustomizeView::from(f);
    crate::customize::build_customize_menu(&view, ID_CUSTOMIZE_BASE, ID_FENCE_BLUR_RADIUS)
}

fn apply_customize(hwnd: HWND, code: usize) {
    use crate::customize::*;
    let (kind, value) = (code / KIND_STRIDE, code % KIND_STRIDE);
    unsafe {
        crate::app::with_state_mut(|s| {
            let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) else {
                return;
            };
            let f = &mut fw.fence_data;
            match kind {
                KIND_BG_COLOR | KIND_BORDER_COLOR | KIND_TITLE_COLOR | KIND_TEXT_COLOR => {
                    let Some(opt) = decoded_color(value) else {
                        return;
                    };
                    match kind {
                        KIND_BG_COLOR => f.custom_color = opt,
                        KIND_BORDER_COLOR => f.fence_border_color = opt,
                        KIND_TITLE_COLOR => f.title_text_color = opt,
                        KIND_TEXT_COLOR => f.text_color = opt,
                        _ => {}
                    }
                }
                KIND_BORDER_THICK => {
                    let Some(v) = decoded_border_thick(value) else {
                        return;
                    };
                    f.fence_border_thickness = v;
                }
                KIND_ICON_SIZE => {
                    let Some(v) = decoded_icon_size(value) else {
                        return;
                    };
                    f.icon_size = v;
                    fw.d2d.icon_cache.invalidate();
                }
                KIND_ICON_SPACING => {
                    let Some(v) = decoded_icon_spacing(value) else {
                        return;
                    };
                    f.icon_spacing = v;
                }
                KIND_BOLD_TOGGLE => {
                    f.bold_title_text = toggle_bool_str(&f.bold_title_text);
                }
                KIND_BLUR_TOGGLE => {
                    let new_state = f.blur_enabled != "true";
                    f.blur_enabled = if new_state {
                        "true".into()
                    } else {
                        "false".into()
                    };
                    crate::blur::set_blur(fw.hwnd, new_state);
                }
                KIND_BG_OPACITY => {
                    let Some(v) = decoded_opacity(value) else {
                        return;
                    };
                    f.bg_opacity = v;
                }
                KIND_LABELS_TOGGLE => {
                    f.show_item_labels = toggle_bool_str(&f.show_item_labels);
                }
                _ => {}
            }
            let _ = fw.render();
            let id = fw.fence_data.id.clone();
            let fd = fw.fence_data.clone();
            if let Some(cf) = s.config.fences.iter_mut().find(|c| c.id == id) {
                *cf = fd;
            }
            let _ = s.config.save_fences();
        });
    }
}

/// Flip a C#-compat stringly-typed boolean ("true"/"false"). Used by the
/// customize togglers that share the same write-back shape.
pub fn toggle_bool_str(s: &str) -> String {
    if s == "true" {
        "false".into()
    } else {
        "true".into()
    }
}

fn tick_animation(hwnd: HWND) {
    unsafe {
        crate::app::with_state_mut(|s| {
            let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) else {
                return;
            };
            let Some(anim) = fw.anim.as_ref() else {
                return;
            };
            let now = windows::Win32::System::SystemInformation::GetTickCount();
            let elapsed = now.wrapping_sub(anim.start_tick);
            let t: f32 = (elapsed as f32 / anim.duration_ms as f32).clamp(0.0, 1.0);
            // Ease-out cubic.
            let e = 1.0 - (1.0 - t).powi(3);
            let new_h = anim.start_h as f32 + (anim.target_h - anim.start_h) as f32 * e;
            // start_h / target_h are in physical pixels, so new_h is too.
            let w = dip_to_px(fw.fence_data.width, fw.d2d.dpi);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                w,
                new_h.round() as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = fw.render();
            if t >= 1.0 {
                let _ = KillTimer(Some(hwnd), TIMER_ID_ANIM);
                fw.anim = None;
            }
        });
    }
}

/// 60 fps repaint while an icon drag is active. Both the floating icon's
/// position (cursor + grab offset) and the displacement animation read
/// state mutated by WM_MOUSEMOVE, so a render every ~16 ms keeps the
/// floating icon glued to the cursor and slides displaced siblings
/// smoothly. Timer is killed on WM_LBUTTONUP.
fn tick_drag(hwnd: HWND) {
    unsafe {
        crate::app::with_state_mut(|s| {
            if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd)
                && fw.drag_render.is_some()
            {
                let _ = fw.render();
            }
        });
    }
}

/// Prompt for a new title via the modal input dialog, then write it back
/// to the fence and persist. Centralised so the three entry points
/// (Ctrl+click title, Ctrl+drag on caption, menu Rename) share the same
/// borrow-modal-reborrow shape — the modal spins its own message loop,
/// so the gather/apply borrows MUST straddle it, not nest inside it.
fn rename_fence_via_modal(hwnd: HWND) {
    let current = unsafe {
        crate::app::with_state(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .map(|fw| fw.fence_data.title.clone())
        })
        .flatten()
        .unwrap_or_default()
    };
    let Some(new_title) = crate::modal::input(hwnd, "Rename fence", &current) else {
        return;
    };
    unsafe {
        crate::app::with_state_mut(|s| {
            if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                fw.fence_data.title = new_title.clone();
                let _ = fw.render();
                let id = fw.fence_data.id.clone();
                if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == id) {
                    cf.title = new_title;
                }
                let _ = s.config.save_fences();
            }
        });
    }
}

fn launch_item(item: &FenceItem) {
    let (target, args, working_dir) =
        if item.is_link || item.filename.to_ascii_lowercase().ends_with(".lnk") {
            match resolve_lnk(&item.filename) {
                Some(info) => {
                    let args = item
                        .arguments
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(info.arguments);
                    (info.target, args, info.working_dir)
                }
                None => (
                    item.filename.clone(),
                    item.arguments.clone().unwrap_or_default(),
                    String::new(),
                ),
            }
        } else {
            (
                item.filename.clone(),
                item.arguments.clone().unwrap_or_default(),
                String::new(),
            )
        };

    if target.is_empty() {
        return;
    }

    let wtarget: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let wargs: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
    let wdir: Vec<u16> = working_dir
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let _ = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(wtarget.as_ptr()),
            if args.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(wargs.as_ptr())
            },
            if working_dir.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(wdir.as_ptr())
            },
            SW_SHOWNORMAL,
        );
    }
}

fn open_in_explorer(path: &str) {
    // Resolve link target first if applicable.
    let target = if path.to_ascii_lowercase().ends_with(".lnk") {
        resolve_lnk(path)
            .map(|i| i.target)
            .filter(|s| !s.is_empty())
            .unwrap_or(path.to_string())
    } else {
        path.to_string()
    };
    let arg = format!("/select,\"{}\"", target);
    let warg: Vec<u16> = arg.encode_utf16().chain(std::iter::once(0)).collect();
    let exe: Vec<u16> = "explorer.exe\0".encode_utf16().collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(exe.as_ptr()),
            PCWSTR(warg.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

// Snap distance in logical DIPs; converted to physical pixels per-window.
// Snap distance in logical DIPs; converted to physical pixels per-window.
const SNAP_THRESHOLD_DIP: i32 = 16;

unsafe fn apply_snap(hwnd: HWND, lparam: LPARAM) {
    let rect_ptr = lparam.0 as *mut RECT;
    if rect_ptr.is_null() {
        return;
    }
    let snap_thr = dip_to_px(SNAP_THRESHOLD_DIP as f64, window_dpi(hwnd));
    let rect = &mut *rect_ptr;
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;

    // Snap to work area edges of the monitor under the window.
    let mut work = RECT::default();
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mon = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
    if GetMonitorInfoW(mon, &mut info).as_bool() {
        work = info.rcWork;
    }

    let mut new_left = rect.left;
    let mut new_top = rect.top;

    if (rect.left - work.left).abs() <= snap_thr {
        new_left = work.left;
    } else if (rect.right - work.right).abs() <= snap_thr {
        new_left = work.right - w;
    }
    if (rect.top - work.top).abs() <= snap_thr {
        new_top = work.top;
    } else if (rect.bottom - work.bottom).abs() <= snap_thr {
        new_top = work.bottom - h;
    }

    // Snap to other fences' edges.
    crate::app::with_state(|s| {
        for fw in &s.fences {
            if fw.hwnd == hwnd {
                continue;
            }
            let mut other = RECT::default();
            let _ = GetWindowRect(fw.hwnd, &mut other);
            // Left-to-right edge.
            if (rect.left - other.right).abs() <= snap_thr
                && rect.bottom > other.top
                && rect.top < other.bottom
            {
                new_left = other.right;
            }
            // Right-to-left edge.
            if (rect.right - other.left).abs() <= snap_thr
                && rect.bottom > other.top
                && rect.top < other.bottom
            {
                new_left = other.left - w;
            }
            // Top-to-bottom.
            if (rect.top - other.bottom).abs() <= snap_thr
                && rect.right > other.left
                && rect.left < other.right
            {
                new_top = other.bottom;
            }
            // Bottom-to-top.
            if (rect.bottom - other.top).abs() <= snap_thr
                && rect.right > other.left
                && rect.left < other.right
            {
                new_top = other.top - h;
            }
        }
    });

    if new_left != rect.left || new_top != rect.top {
        rect.left = new_left;
        rect.top = new_top;
        rect.right = new_left + w;
        rect.bottom = new_top + h;
    }
}

// Silence unused-import warning when keyboard module is added later.
#[allow(dead_code)]
fn _vk_unused(_: VIRTUAL_KEY) {}
