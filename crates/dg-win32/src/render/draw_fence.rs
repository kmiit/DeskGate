// One frame of fence rendering: backdrop tint, border, title, items (icons +
// labels, or note body). Everything is authored in logical DIPs against the
// D2D device context that `D2DContext::begin_frame` returns; the caller has
// already ensured the surface exists at the right physical pixel size.
//
// `DragHint` is the optional payload for a live drag-reorder gesture — it
// tells us which item is being carried (drawn last as a floating ghost) and
// where every other item should currently sit (caller animates these slot
// positions between frames).

use dg_core::fence::Fence;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::System::WinRT::Composition::*;
use windows::core::*;

use crate::layout::IconLayout;

use super::colors::{parse_fence_color, parse_text_color};
use super::d2d_context::{CORNER_RADIUS, D2DContext, TITLE_H_DIP, dip_to_px};

// Visual hint to `draw_fence` while a drag-reorder is in progress.
// `src` is the icon being moved — drawn separately as a floating ghost
// at `floating_dip` (top-left of the cell to draw it in) so the user
// can carry it under the cursor. `item_slots` overrides the natural
// slot position of every other item: each entry is a (possibly
// fractional) linear slot index, animated by the caller between the
// "before" and "after" layouts so that displaced icons slide into
// their new positions instead of popping. The entry at index `src`
// is ignored.
pub struct DragHint {
    pub src: usize,
    pub floating_dip: (f32, f32),
    pub item_slots: Vec<f32>,
}

pub fn draw_fence(
    ctx: &mut D2DContext,
    fence: &Fence,
    drag: Option<DragHint>,
) -> windows::core::Result<()> {
    let w = fence.width as u32;
    let h = if fence.is_rolled == "true" {
        TITLE_H_DIP as u32
    } else {
        fence.height as u32
    };
    if w == 0 || h == 0 {
        return Ok(());
    }

    ctx.ensure_surface(w, h)?;
    let dpi = ctx.dpi as f32;

    // Pre-fetch text formats before borrowing the surface.
    let title_font_size = match fence.title_text_size.as_str() {
        "Small" => 11.0,
        "Large" => 15.0,
        _ => 13.0,
    };
    let title_format = ctx.get_text_format(title_font_size, fence.bold_title_text == "true")?;
    let label_format = ctx.get_text_format(10.0, false)?;
    // Center-align icon labels under their icons. The cached format is also
    // used elsewhere, so set alignment on a fresh derived format each draw —
    // cheap because TextFormat has no GPU state. Actually IDWriteTextFormat
    // settings are sticky and the format is shared, so set it once on the
    // cached object: we only ever use the (10.0, false) format for labels.
    unsafe {
        label_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    }
    let note_format = if fence.items_type == "Note" {
        let note_size = match fence.note_font_size.as_str() {
            "Small" => 11.0,
            "Large" => 16.0,
            "Huge" => 20.0,
            _ => 13.0,
        };
        Some(ctx.get_text_format(note_size, false)?)
    } else {
        None
    };

    unsafe {
        let surface = ctx.drawing_surface.as_ref().unwrap();
        let surface_interop: ICompositionDrawingSurfaceInterop = surface.cast()?;

        // BeginDraw hands us back an ID2D1DeviceContext for the current frame
        // plus an `offset` that we must add to every coordinate, because the
        // surface may be backed by a larger atlas. The offset is in *physical
        // pixels*; once we switch the DC into DIPs mode below we have to
        // convert it.
        let mut offset = POINT::default();
        let dc: ID2D1DeviceContext =
            surface_interop.BeginDraw::<ID2D1DeviceContext>(None, &mut offset)?;

        // Tell D2D the surface is at our window's DPI. After this every
        // coordinate passed to draw calls is interpreted as DIPs and scaled
        // up automatically to physical pixels. The offset stays in pixels
        // because Composition decided it before we called SetDpi.
        dc.SetDpi(dpi, dpi);

        let scale = dpi / 96.0;
        let ox = offset.x as f32 / scale;
        let oy = offset.y as f32 / scale;

        dc.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));

        // Background tint sitting *on top* of the blur layer. Lower-alpha tint
        // lets more of the blurred wallpaper show through, matching the
        // miuix frosted-glass look. Custom colors stay a touch more saturated.
        let user_alpha = (fence.bg_opacity as f32).clamp(0.0, 1.0);
        let bg_alpha = if fence.custom_color.is_some() {
            (user_alpha + 0.10).min(1.0)
        } else {
            user_alpha
        };
        let bg_color = parse_fence_color(&fence.custom_color, bg_alpha);
        let brush: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&bg_color, None)?;

        let border = fence.fence_border_thickness as f32;
        let rect = D2D_RECT_F {
            left: border / 2.0 + ox,
            top: border / 2.0 + oy,
            right: w as f32 - border / 2.0 + ox,
            bottom: h as f32 - border / 2.0 + oy,
        };
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: CORNER_RADIUS,
            radiusY: CORNER_RADIUS,
        };
        dc.FillRoundedRectangle(&rounded, &brush);

        let border_color =
            parse_fence_color(&fence.fence_border_color, (user_alpha + 0.20).min(0.85));
        let border_brush: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&border_color, None)?;
        if fence.fence_border_thickness > 0 && user_alpha + 0.20 > 0.05 {
            dc.DrawRoundedRectangle(&rounded, &border_brush, border, None);
        }

        let title_h = 28.0f32;
        let title_color = parse_text_color(&fence.title_text_color);
        let title_brush: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&title_color, None)?;

        let title_rect = D2D_RECT_F {
            left: 10.0 + ox,
            top: 2.0 + oy,
            right: w as f32 - 10.0 + ox,
            bottom: title_h + oy,
        };
        let title: Vec<u16> = fence.title.encode_utf16().collect();
        dc.DrawText(
            &title,
            &title_format,
            &title_rect,
            &title_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        if fence.is_rolled != "true" {
            if fence.items_type == "Note" {
                let body_color = parse_text_color(&fence.text_color);
                let body_brush: ID2D1SolidColorBrush =
                    dc.CreateSolidColorBrush(&body_color, None)?;
                let body_text = fence.note_content.clone().unwrap_or_default();
                let body: Vec<u16> = body_text.encode_utf16().collect();
                let body_rect = D2D_RECT_F {
                    left: 12.0 + ox,
                    top: title_h + 4.0 + oy,
                    right: w as f32 - 12.0 + ox,
                    bottom: h as f32 - 8.0 + oy,
                };
                if let Some(fmt) = &note_format {
                    dc.DrawText(
                        &body,
                        fmt,
                        &body_rect,
                        &body_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            } else {
                let layout = IconLayout::from_fence(fence);
                let icon_y_start = layout.icon_y_start;
                let spacing = layout.spacing;
                let icon_size = layout.icon_size;
                let cell_w = layout.cell_w;
                let cell_h = layout.cell_h;
                let cols = layout.cols;
                let show_labels = layout.show_labels;

                let label_color = parse_text_color(&fence.text_color);
                let label_brush: ID2D1SolidColorBrush =
                    dc.CreateSolidColorBrush(&label_color, None)?;
                let placeholder_brush: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.3,
                        g: 0.3,
                        b: 0.3,
                        a: 0.4,
                    },
                    None,
                )?;

                // Resolve the on-screen cell (col, row, both as floats) for
                // an item: the natural slot when no drag is active,
                // otherwise the lerped slot from the drag hint. Linear
                // slot indices wrap by `cols`, which gives diagonal slides
                // at row boundaries — acceptable since most drags stay in
                // the same row.
                let pos_for = |i: usize| -> (f32, f32) {
                    let slot = match &drag {
                        Some(d) => d.item_slots.get(i).copied().unwrap_or(i as f32),
                        None => i as f32,
                    };
                    let s = slot.max(0.0);
                    let s_floor = s.floor();
                    let frac = s - s_floor;
                    let lo = s_floor as usize;
                    let hi = lo + 1;
                    let col_lo = (lo % cols) as f32;
                    let row_lo = (lo / cols) as f32;
                    let col_hi = (hi % cols) as f32;
                    let row_hi = (hi / cols) as f32;
                    (
                        col_lo + (col_hi - col_lo) * frac,
                        row_lo + (row_hi - row_lo) * frac,
                    )
                };

                for (i, item) in fence.items.iter().enumerate() {
                    // The dragged icon paints separately at the cursor.
                    if let Some(d) = &drag
                        && d.src == i
                    {
                        continue;
                    }

                    let (col_f, row_f) = pos_for(i);
                    let ix = spacing + col_f * cell_w + ox;
                    let iy = icon_y_start + row_f * cell_h + oy;

                    let icon_rect = D2D_RECT_F {
                        left: ix + (cell_w - icon_size) / 2.0,
                        top: iy,
                        right: ix + (cell_w + icon_size) / 2.0,
                        bottom: iy + icon_size,
                    };

                    let icon_target_px = dip_to_px(icon_size, ctx.dpi).round() as u32;
                    let bmp = ctx
                        .icon_cache
                        .get_or_load(&dc, &item.filename, icon_target_px);

                    if let Some(bmp) = bmp {
                        dc.DrawBitmap(
                            &bmp,
                            Some(&icon_rect),
                            1.0,
                            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                            None,
                            None,
                        );
                    } else {
                        dc.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: icon_rect,
                                radiusX: 3.0,
                                radiusY: 3.0,
                            },
                            &placeholder_brush,
                        );
                    }

                    if show_labels {
                        let label_name = item.display_name_or_filename();
                        let label: Vec<u16> = label_name.encode_utf16().take(20).collect();
                        let label_rect = D2D_RECT_F {
                            left: ix,
                            top: iy + icon_size + 2.0,
                            right: ix + cell_w,
                            bottom: iy + icon_size + 16.0,
                        };
                        dc.DrawText(
                            &label,
                            &label_format,
                            &label_rect,
                            &label_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                // Floating dragged icon: drawn LAST so it always sits on
                // top of any displaced sibling. The cell rect is positioned
                // by the caller (cursor minus grab offset) so the icon
                // sticks where the user grabbed it. No dim, no outline —
                // just a clean lifted icon, no label (it's transient).
                if let Some(d) = drag.as_ref()
                    && let Some(item) = fence.items.get(d.src)
                {
                    let (cx, cy) = d.floating_dip;
                    let icon_rect = D2D_RECT_F {
                        left: cx + (cell_w - icon_size) / 2.0 + ox,
                        top: cy + oy,
                        right: cx + (cell_w + icon_size) / 2.0 + ox,
                        bottom: cy + icon_size + oy,
                    };
                    let icon_target_px = dip_to_px(icon_size, ctx.dpi).round() as u32;
                    let bmp = ctx
                        .icon_cache
                        .get_or_load(&dc, &item.filename, icon_target_px);
                    if let Some(bmp) = bmp {
                        dc.DrawBitmap(
                            &bmp,
                            Some(&icon_rect),
                            1.0,
                            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                            None,
                            None,
                        );
                    }
                }
            }
        }

        // Release the DC reference returned by BeginDraw before EndDraw.
        drop(dc);
        surface_interop.EndDraw()?;
    }

    Ok(())
}
