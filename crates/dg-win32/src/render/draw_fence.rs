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

use crate::layout::{IconLayout, TodoAlign, TodoLayout, TodoRowGeom};

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
    anim_h_dip: Option<f32>,
) -> windows::core::Result<()> {
    let w = fence.width as u32;
    // While a roll animation is running, the caller hands us the current
    // interpolated height so the D2D surface tracks the window — content
    // gets clipped as the window shrinks (roll-up) or revealed as it grows
    // (roll-down). Without this the surface would snap to its final size
    // on the first animation frame, making icons disappear/reappear in one
    // jarring step.
    let (h, animating) = match anim_h_dip {
        Some(ah) => (ah.round().max(1.0) as u32, true),
        None => (
            if fence.is_rolled == "true" {
                TITLE_H_DIP as u32
            } else {
                fence.height as u32
            },
            false,
        ),
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
    let title_align = match fence.title_text_align.as_str() {
        "Left" => DWRITE_TEXT_ALIGNMENT_LEADING,
        "Right" => DWRITE_TEXT_ALIGNMENT_TRAILING,
        _ => DWRITE_TEXT_ALIGNMENT_CENTER,
    };
    unsafe {
        title_format.SetTextAlignment(title_align)?;
    }
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

    // Apply the note's alignment to the shared format. The format is
    // cached across draws so setting it each frame is required —
    // otherwise the title's alignment (set above) would leak through
    // for plain-text notes.
    let note_dwrite_align = match fence.note_text_align.as_str() {
        "Right" => DWRITE_TEXT_ALIGNMENT_TRAILING,
        "Center" => DWRITE_TEXT_ALIGNMENT_CENTER,
        _ => DWRITE_TEXT_ALIGNMENT_LEADING,
    };
    if let Some(fmt) = &note_format {
        unsafe {
            fmt.SetTextAlignment(note_dwrite_align)?;
        }
    }

    // Pre-compute per-row geometry for the TODO variant. This must
    // happen before the `unsafe { BeginDraw … }` block below because
    // measure_text_height borrows ctx mutably and a TODO row's height
    // depends on wrapped-text height for the row's font size + max
    // width.
    let todo_geom: Option<(TodoLayout, Vec<TodoRowGeom>)> =
        if fence.items_type == "Note" && fence.note_mode == "todo" {
            let layout = TodoLayout::from_fence(fence);
            let font_size = layout.font_size;
            let rows = layout.compute_rows(fence, &mut |text, max_w| {
                ctx.measure_text_height(text, font_size, false, max_w)
                    .unwrap_or(font_size * 1.3)
            });
            Some((layout, rows))
        } else {
            None
        };

    // Snapshot the DWrite factory so the unsafe block can build per-row
    // IDWriteTextLayouts without holding a mutable borrow on `ctx`.
    let dwrite = ctx.dwrite_factory.clone();

    // Background tint sitting *on top* of the blur layer. Lower-alpha tint
    // lets more of the blurred wallpaper show through, matching the
    // frosted-glass look. If the tint is effectively solid, D2DContext can
    // detach the expensive backdrop brush because it would be hidden.
    let user_alpha = (fence.bg_opacity as f32).clamp(0.0, 1.0);
    let bg_alpha = if fence.custom_color.is_some() {
        (user_alpha + 0.10).min(1.0)
    } else {
        user_alpha
    };
    ctx.set_blur_tint_alpha(bg_alpha)?;

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

        let outline_enabled = fence.text_outline_enabled == "true";
        let title_h = TITLE_H_DIP;
        let title_x_pad = (10.0 + border * 0.5)
            .max(border + 2.0)
            .min((w as f32 * 0.5 - 1.0).max(1.0));
        let title_top_pad = (2.0 + border * 0.5).max(border + 1.0);
        let title_color = parse_text_color(&fence.title_text_color);
        let title_brush: ID2D1SolidColorBrush = dc.CreateSolidColorBrush(&title_color, None)?;
        let title_outline_brush = if outline_enabled {
            Some(dc.CreateSolidColorBrush(&outline_color_for(&title_color), None)?)
        } else {
            None
        };

        let title_rect = D2D_RECT_F {
            left: title_x_pad + ox,
            top: title_top_pad + oy,
            right: w as f32 - title_x_pad + ox,
            bottom: title_h + oy,
        };
        let title: Vec<u16> = fence.title.encode_utf16().collect();
        draw_text_with_optional_outline(
            &dc,
            &title,
            &title_format,
            &title_rect,
            &title_brush,
            title_outline_brush.as_ref(),
        );

        // Outside an animation, the `is_rolled` flag gates whether body
        // content draws at all. During a roll-up/-down animation we draw
        // the body unconditionally so icons fade away with the shrinking
        // surface (or appear with the growing one) instead of popping.
        if animating || fence.is_rolled != "true" {
            if fence.items_type == "Note" {
                let body_color = parse_text_color(&fence.text_color);
                let body_brush: ID2D1SolidColorBrush =
                    dc.CreateSolidColorBrush(&body_color, None)?;
                let body_outline_brush = if outline_enabled {
                    Some(dc.CreateSolidColorBrush(&outline_color_for(&body_color), None)?)
                } else {
                    None
                };

                if fence.note_mode == "todo" {
                    if let (Some((layout, rows)), Some(fmt)) =
                        (todo_geom.as_ref(), note_format.as_ref())
                    {
                        draw_todo_list(
                            &dc,
                            fence,
                            layout,
                            rows,
                            &dwrite,
                            fmt,
                            &body_brush,
                            body_outline_brush.as_ref(),
                            ox,
                            oy,
                            title_h,
                            w as f32,
                            h as f32,
                        )?;
                    }
                } else {
                    let body_text = fence.note_content.clone().unwrap_or_default();
                    let show_text = if body_text.is_empty() {
                        dg_locales::t(dg_locales::NOTE_EMPTY_HINT).to_string()
                    } else {
                        body_text
                    };
                    let body: Vec<u16> = show_text.encode_utf16().collect();
                    let body_rect = D2D_RECT_F {
                        left: 12.0 + ox,
                        top: title_h + 4.0 + oy,
                        right: w as f32 - 12.0 + ox,
                        bottom: h as f32 - 8.0 + oy,
                    };
                    if let Some(fmt) = &note_format {
                        draw_text_with_optional_outline(
                            &dc,
                            &body,
                            fmt,
                            &body_rect,
                            &body_brush,
                            body_outline_brush.as_ref(),
                        );
                    }
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
                let label_outline_brush = if outline_enabled {
                    Some(dc.CreateSolidColorBrush(&outline_color_for(&label_color), None)?)
                } else {
                    None
                };
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
                        draw_text_with_optional_outline(
                            &dc,
                            &label,
                            &label_format,
                            &label_rect,
                            &label_brush,
                            label_outline_brush.as_ref(),
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

fn outline_color_for(fill: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    let luminance = fill.r * 0.299 + fill.g * 0.587 + fill.b * 0.114;
    if luminance > 0.56 {
        D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.78,
        }
    } else {
        D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.86,
        }
    }
}

fn draw_text_with_optional_outline(
    dc: &ID2D1DeviceContext,
    text: &[u16],
    format: &IDWriteTextFormat,
    rect: &D2D_RECT_F,
    fill_brush: &ID2D1SolidColorBrush,
    outline_brush: Option<&ID2D1SolidColorBrush>,
) {
    if let Some(outline_brush) = outline_brush {
        for &(dx, dy) in TEXT_OUTLINE_OFFSETS {
            let outline_rect = D2D_RECT_F {
                left: rect.left + dx,
                top: rect.top + dy,
                right: rect.right + dx,
                bottom: rect.bottom + dy,
            };
            unsafe {
                dc.DrawText(
                    text,
                    format,
                    &outline_rect,
                    outline_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
    unsafe {
        dc.DrawText(
            text,
            format,
            rect,
            fill_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

fn draw_text_layout_with_optional_outline(
    dc: &ID2D1DeviceContext,
    origin: windows_numerics::Vector2,
    layout: &IDWriteTextLayout,
    fill_brush: &ID2D1SolidColorBrush,
    outline_brush: Option<&ID2D1SolidColorBrush>,
) {
    if let Some(outline_brush) = outline_brush {
        for &(dx, dy) in TEXT_OUTLINE_OFFSETS {
            let outline_origin = windows_numerics::Vector2 {
                X: origin.X + dx,
                Y: origin.Y + dy,
            };
            unsafe {
                dc.DrawTextLayout(
                    outline_origin,
                    layout,
                    outline_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
    }
    unsafe {
        dc.DrawTextLayout(origin, layout, fill_brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

const TEXT_OUTLINE_OFFSETS: &[(f32, f32)] = &[
    (-1.0, 0.0),
    (1.0, 0.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (-0.75, -0.75),
    (0.75, -0.75),
    (-0.75, 0.75),
    (0.75, 0.75),
];

/// Paint the TODO-list variant of a Note fence: one row per `NoteItem`,
/// each row a checkbox square + label text. Checked rows fill the box
/// with the brand tint, draw a checkmark glyph, fade the label, and let
/// IDWriteTextLayout's built-in strikethrough run through *only the
/// glyph runs* (so wrapped lines get their own per-line strike that
/// stops at the last character).
///
/// Geometry comes from the precomputed `rows` slice rather than being
/// recalculated here, so the click hit-tester in fence_window sees the
/// same per-row rects this paints.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_todo_list(
    dc: &ID2D1DeviceContext,
    fence: &Fence,
    layout: &TodoLayout,
    rows: &[TodoRowGeom],
    dwrite: &IDWriteFactory,
    text_format: &IDWriteTextFormat,
    body_brush: &ID2D1SolidColorBrush,
    body_outline_brush: Option<&ID2D1SolidColorBrush>,
    ox: f32,
    oy: f32,
    title_h: f32,
    w: f32,
    h: f32,
) -> windows::core::Result<()> {
    let row_top = layout.top.max(title_h + 4.0);
    let bottom = h - 4.0;

    // Empty-state hint. The caller already filtered out the `text` mode,
    // so an empty `note_items` here means a brand-new TODO fence.
    if fence.note_items.is_empty() {
        let hint = dg_locales::t(dg_locales::NOTE_TODO_EDIT_HINT);
        let hint_u16: Vec<u16> = hint.encode_utf16().collect();
        let hint_rect = D2D_RECT_F {
            left: layout.left + ox,
            top: row_top + oy,
            right: w - layout.right_inset + ox,
            bottom: bottom + oy,
        };
        draw_text_with_optional_outline(
            dc,
            &hint_u16,
            text_format,
            &hint_rect,
            body_brush,
            body_outline_brush,
        );
        return Ok(());
    }

    let dwrite_align = match layout.align {
        TodoAlign::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
        TodoAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        TodoAlign::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
    };

    // Brushes created once per draw — D2D brushes are cheap but not
    // free, and we redo this on every frame already.
    let box_outline: ID2D1SolidColorBrush = unsafe {
        dc.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.55,
            },
            None,
        )?
    };
    let box_check_fill: ID2D1SolidColorBrush = unsafe {
        dc.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.30,
                g: 0.62,
                b: 0.98,
                a: 0.90,
            },
            None,
        )?
    };
    let box_check_glyph: ID2D1SolidColorBrush = unsafe {
        dc.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            None,
        )?
    };
    // Faded twin of the body color for the struck-through row label.
    let body_color = parse_text_color(&fence.text_color);
    let faded_color = D2D1_COLOR_F {
        a: body_color.a * 0.55,
        ..body_color
    };
    let faded_brush: ID2D1SolidColorBrush =
        unsafe { dc.CreateSolidColorBrush(&faded_color, None)? };
    let faded_outline_brush = if body_outline_brush.is_some() {
        let mut outline = outline_color_for(&faded_color);
        outline.a *= 0.65;
        Some(unsafe { dc.CreateSolidColorBrush(&outline, None)? })
    } else {
        None
    };

    for (it, geom) in fence.note_items.iter().zip(rows.iter()) {
        // Drop rows that start past the bottom of the body; rows
        // themselves may overflow slightly (last row clipped by the
        // surface) which is OK — that mirrors how shortcut fences
        // behave when there are too many icons to fit.
        if geom.y_top >= bottom {
            break;
        }

        // Checkbox.
        let (cx, cy) = (geom.checkbox.0 + ox, geom.checkbox.1 + oy);
        let box_rect = D2D_RECT_F {
            left: cx,
            top: cy,
            right: cx + layout.checkbox_size,
            bottom: cy + layout.checkbox_size,
        };
        let box_rr = D2D1_ROUNDED_RECT {
            rect: box_rect,
            radiusX: 3.0,
            radiusY: 3.0,
        };
        if it.checked {
            unsafe {
                dc.FillRoundedRectangle(&box_rr, &box_check_fill);
                draw_checkmark(dc, &box_rect, &box_check_glyph);
            }
        } else {
            unsafe {
                dc.DrawRoundedRectangle(&box_rr, &box_outline, 1.5, None);
            }
        }

        // Text — built as a per-row IDWriteTextLayout so the
        // strikethrough decoration runs through each wrapped line up
        // to the last glyph (instead of being drawn manually across
        // the row width).
        let text_u16: Vec<u16> = it.text.encode_utf16().collect();
        let max_w = geom.text_w.max(1.0);
        let max_h = geom.height.max(layout.font_size * 1.3);
        let text_layout: IDWriteTextLayout =
            unsafe { dwrite.CreateTextLayout(&text_u16, text_format, max_w, max_h)? };
        unsafe {
            text_layout.SetTextAlignment(dwrite_align)?;
            if it.checked && !text_u16.is_empty() {
                let range = DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: text_u16.len() as u32,
                };
                text_layout.SetStrikethrough(true, range)?;
            }
        }

        let origin = windows_numerics::Vector2 {
            X: geom.text_x + ox,
            Y: geom.y_top + oy,
        };
        let row_brush: &ID2D1SolidColorBrush = if it.checked { &faded_brush } else { body_brush };
        let row_outline_brush = if it.checked {
            faded_outline_brush.as_ref().or(body_outline_brush)
        } else {
            body_outline_brush
        };
        draw_text_layout_with_optional_outline(
            dc,
            origin,
            &text_layout,
            row_brush,
            row_outline_brush,
        );
    }

    Ok(())
}

/// Paint a simple two-segment checkmark glyph inside the checked box.
/// Stays geometric (not a font) so it scales with the box size and
/// looks the same on every machine.
unsafe fn draw_checkmark(dc: &ID2D1DeviceContext, rect: &D2D_RECT_F, brush: &ID2D1SolidColorBrush) {
    let cx = rect.left;
    let cy = rect.top;
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    let stroke = (w.min(h) * 0.18).max(1.5);
    let p1 = windows_numerics::Vector2 {
        X: cx + w * 0.22,
        Y: cy + h * 0.52,
    };
    let p2 = windows_numerics::Vector2 {
        X: cx + w * 0.42,
        Y: cy + h * 0.72,
    };
    let p3 = windows_numerics::Vector2 {
        X: cx + w * 0.78,
        Y: cy + h * 0.30,
    };
    unsafe {
        dc.DrawLine(p1, p2, brush, stroke, None);
        dc.DrawLine(p2, p3, brush, stroke, None);
    }
}
