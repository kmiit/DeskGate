// Self-drawn modal dialog framework — *traditional* HWND variant.
//
// An earlier version used WS_EX_NOREDIRECTIONBITMAP + WinRT Composition
// (same pipeline as fence windows) but that breaks child HWND controls:
// the EDIT control's text never reaches the screen because there is no
// redirection surface for GDI to blit into. So this version uses a normal
// HWND, paints its chrome with D2D on an ID2D1HwndRenderTarget, and asks
// DWM for a BlurBehind backdrop. Looks slightly less crisp than the
// Composition-backed blur on fences but lets the native EDIT control
// (and its IME, selection, clipboard) keep working.
//
// Layout (logical DIPs):
//
//   ┌──────────────────────────────────────────────────────────┐  ▲
//   │  18  TITLE (wraps to as many lines as needed)        18  │  │  title row
//   │                                                          │  │
//   │  18  body / EDIT control area                        18  │  │  body  (variable)
//   │                                                          │  │
//   │  18  ┌───────────────┐ ┌────────┐                    18  │  │  button row (48)
//   │      │  Confirm/OK   │ │ Cancel │                        │  │
//   └──────────────────────────────────────────────────────────┘  ▼

mod render;
mod run;

use dg_locales as loc;
use windows::Win32::Foundation::HWND;

// Layout constants in DIPs. Shared between hit-testing (run) and drawing
// (render), so kept here at the module root rather than duplicated.
pub(super) const PAD: f32 = 18.0;
pub(super) const TITLE_FONT: f32 = 17.0;
pub(super) const BODY_FONT: f32 = 13.0;
pub(super) const BTN_W: f32 = 96.0;
pub(super) const BTN_H: f32 = 30.0;
pub(super) const BTN_GAP: f32 = 8.0;
pub(super) const EDIT_H: f32 = 32.0;
pub(super) const TITLE_LINE_H: f32 = TITLE_FONT * 1.35;
pub(super) const BODY_LINE_H: f32 = BODY_FONT * 1.4;
// Average DIPs per glyph at TITLE_FONT/BODY_FONT, used as a quick wrap
// heuristic for height pre-allocation. Real width is measured per-draw.
pub(super) const AVG_TITLE_GLYPH_W: f32 = 9.0;
pub(super) const AVG_BODY_GLYPH_W: f32 = 6.6;
// Drag region: the top strip behaves like a title bar even though there
// is no painted caption — gives the user something to grab.
pub(super) const DRAG_STRIP_H: f32 = 28.0;

#[derive(Clone)]
pub struct ButtonSpec {
    pub label: String,
    pub result: i32,
    pub default: bool,
    pub cancel: bool,
    pub destructive: bool,
}

pub struct ModalSpec {
    pub title: String,
    pub body: Option<String>,
    pub buttons: Vec<ButtonSpec>,
    pub edit_default: Option<String>,
    pub width: f32,
}

const RESULT_OK: i32 = 1;
const RESULT_CANCEL: i32 = 2;

pub fn input(owner: HWND, title: &str, default: &str) -> Option<String> {
    let spec = ModalSpec {
        title: title.to_string(),
        body: None,
        edit_default: Some(default.to_string()),
        buttons: vec![
            ButtonSpec {
                label: loc::t(loc::MODAL_CANCEL).to_string(),
                result: RESULT_CANCEL,
                default: false,
                cancel: true,
                destructive: false,
            },
            ButtonSpec {
                label: loc::t(loc::MODAL_OK).to_string(),
                result: RESULT_OK,
                default: true,
                cancel: false,
                destructive: false,
            },
        ],
        width: 380.0,
    };
    let (result, text) = run::run_modal(owner, spec);
    if result == RESULT_OK { text } else { None }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    Confirmed,
    Cancelled,
}

pub fn confirm_destructive(
    owner: HWND,
    _title: &str,
    instruction: &str,
    details: &str,
    confirm_text: &str,
) -> ConfirmResult {
    let spec = ModalSpec {
        title: instruction.to_string(),
        body: Some(details.to_string()),
        edit_default: None,
        buttons: vec![
            ButtonSpec {
                label: loc::t(loc::MODAL_CANCEL).to_string(),
                result: RESULT_CANCEL,
                default: false,
                cancel: true,
                destructive: false,
            },
            ButtonSpec {
                label: confirm_text.to_string(),
                result: RESULT_OK,
                default: false,
                cancel: false,
                destructive: true,
            },
        ],
        width: 440.0,
    };
    let (result, _) = run::run_modal(owner, spec);
    if result == RESULT_OK {
        ConfirmResult::Confirmed
    } else {
        ConfirmResult::Cancelled
    }
}
