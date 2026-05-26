// D2D paint of the modal's chrome: backdrop panel, title text, body or
// EDIT rounded background, and the bottom button strip. Read-only on
// ModalState — it gets style + hover/pressed state from there but never
// mutates it.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::run::{ModalState, body_y};
use super::{BODY_FONT, BTN_GAP, BTN_H, BTN_W, EDIT_H, EDIT_H_MULTILINE, PAD, TITLE_FONT};

pub(super) unsafe fn render(hwnd: HWND) -> Result<()> {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ModalState;
    if state_ptr.is_null() {
        return Ok(());
    }
    let st = &mut *state_ptr;
    let Some(rt) = &st.rt else { return Ok(()) };
    let Some(dwrite) = &st.dwrite_factory else {
        return Ok(());
    };

    let mut crect = RECT::default();
    let _ = GetClientRect(hwnd, &mut crect);
    let dpi = crate::fence_window::window_dpi(hwnd);
    let w = (crect.right as f32) * 96.0 / dpi as f32;
    let h = (crect.bottom as f32) * 96.0 / dpi as f32;
    if w < 1.0 || h < 1.0 {
        return Ok(());
    }

    rt.BeginDraw();
    rt.Clear(Some(&D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    }));

    // Solid dark panel. The window frame itself is rounded by DWM
    // (DWMWA_WINDOW_CORNER_PREFERENCE) so we just fill the whole client
    // area and let the OS clip the corners.
    let tint = D2D1_COLOR_F {
        r: 0.11,
        g: 0.11,
        b: 0.13,
        a: 1.0,
    };
    let bg_brush: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(&tint, None)?;
    let body_rect = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: w,
        bottom: h,
    };
    rt.FillRectangle(&body_rect, &bg_brush);

    // Title — wraps freely; the spec height accounted for that.
    let title_brush: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(
        &D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        None,
    )?;
    let title_format = make_text_format(dwrite, TITLE_FONT, true)?;
    let title_u16: Vec<u16> = st.spec.title.encode_utf16().collect();
    let title_rect = D2D_RECT_F {
        left: PAD,
        top: PAD,
        right: w - PAD,
        bottom: body_y(&st.spec),
    };
    rt.DrawText(
        &title_u16,
        &title_format,
        &title_rect,
        &title_brush,
        D2D1_DRAW_TEXT_OPTIONS_NONE,
        DWRITE_MEASURING_MODE_NATURAL,
    );

    // EDIT decoration or body text.
    if st.edit_hwnd.is_some() {
        let edit_y = body_y(&st.spec);
        let edit_h = if st.spec.multiline {
            EDIT_H_MULTILINE
        } else {
            EDIT_H
        };
        let r = D2D_RECT_F {
            left: PAD,
            top: edit_y,
            right: w - PAD,
            bottom: edit_y + edit_h,
        };
        let border: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.22,
            },
            None,
        )?;
        let edit_fill: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.094,
                g: 0.094,
                b: 0.094,
                a: 1.0,
            },
            None,
        )?;
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        rt.FillRoundedRectangle(&rr, &edit_fill);
        rt.DrawRoundedRectangle(&rr, &border, 1.0, None);
    } else if let Some(body) = &st.spec.body {
        let body_brush: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.82,
            },
            None,
        )?;
        let body_format = make_text_format(dwrite, BODY_FONT, false)?;
        let body_u16: Vec<u16> = body.encode_utf16().collect();
        let body_top = body_y(&st.spec);
        let body_bottom = h - PAD - BTN_H - PAD;
        let r = D2D_RECT_F {
            left: PAD,
            top: body_top,
            right: w - PAD,
            bottom: body_bottom,
        };
        rt.DrawText(
            &body_u16,
            &body_format,
            &r,
            &body_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }

    // Buttons.
    let btn_format = make_text_format(dwrite, 12.5, false)?;
    btn_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    btn_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    let btn_top = h - PAD - BTN_H;
    let mut bx_right = w - PAD;
    for (i, btn) in st.spec.buttons.iter().enumerate() {
        let right = bx_right;
        let left = right - BTN_W;
        let rect = D2D_RECT_F {
            left,
            top: btn_top,
            right,
            bottom: btn_top + BTN_H,
        };
        let hovered = st.hover_btn == i as i32;
        let pressed = st.pressed_btn == i as i32 && hovered;

        let (fill, text_col) = if btn.destructive {
            let mut a = 0.88;
            if pressed {
                a = 1.0;
            } else if hovered {
                a = 0.96;
            }
            (
                D2D1_COLOR_F {
                    r: 0.78,
                    g: 0.20,
                    b: 0.20,
                    a,
                },
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            )
        } else if btn.default {
            let mut a = 0.95;
            if pressed || hovered {
                a = 1.0;
            }
            (
                D2D1_COLOR_F {
                    r: 0.95,
                    g: 0.95,
                    b: 0.97,
                    a,
                },
                D2D1_COLOR_F {
                    r: 0.05,
                    g: 0.05,
                    b: 0.08,
                    a: 1.0,
                },
            )
        } else {
            let mut a = 0.10;
            if pressed {
                a = 0.28;
            } else if hovered {
                a = 0.18;
            }
            (
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a,
                },
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.95,
                },
            )
        };
        let fill_brush: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(&fill, None)?;
        let text_brush: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(&text_col, None)?;
        let rr = D2D1_ROUNDED_RECT {
            rect,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        rt.FillRoundedRectangle(&rr, &fill_brush);
        if !btn.default && !btn.destructive {
            let outline: ID2D1SolidColorBrush = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.22,
                },
                None,
            )?;
            rt.DrawRoundedRectangle(&rr, &outline, 1.0, None);
        }
        let label_u16: Vec<u16> = btn.label.encode_utf16().collect();
        rt.DrawText(
            &label_u16,
            &btn_format,
            &rect,
            &text_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        bx_right = left - BTN_GAP;
    }

    let _ = rt.EndDraw(None, None);
    Ok(())
}

fn make_text_format(factory: &IDWriteFactory, size: f32, bold: bool) -> Result<IDWriteTextFormat> {
    let weight = if bold {
        DWRITE_FONT_WEIGHT_SEMI_BOLD
    } else {
        DWRITE_FONT_WEIGHT_NORMAL
    };
    unsafe {
        factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )
    }
}
