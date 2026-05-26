// Shared icon-grid layout maths for a fence. Three places need the same
// formula — `draw_fence` (paint), `hit_test_icon` (click → item), and
// the drag-grab offset (cursor → cell). Keeping them in lockstep manually
// kept drifting (cell_h forgot the label strip toggle, cols rounded
// differently, etc.), so they all share this one struct now.
//
// All values are in logical DIPs, relative to the fence's client rect.

use dg_core::fence::Fence;

/// Title-band height in DIPs. Also the bottom of the title text rect.
pub const TITLE_BAND_DIP: f32 = 28.0;
/// Gap between the title band and the first row of icons.
pub const TITLE_GAP_DIP: f32 = 4.0;
/// Extra DIPs added under each icon for its label, when labels are shown.
pub const LABEL_STRIP_DIP: f32 = 16.0;

/// TODO-list row layout for Note fences with note_mode == "todo".
/// Mirrors `IconLayout` but the grid degenerates to a single column of
/// uniform rows so hit-testing and rendering share the same geometry.
pub struct TodoLayout {
    /// Inset from the fence's left edge to the row content (DIPs).
    pub left: f32,
    /// Inset from the fence's right edge to the row content (DIPs).
    pub right_inset: f32,
    /// Top of the first row (just below the title band).
    pub top: f32,
    /// Logical width of the checkbox square (height matches).
    pub checkbox_size: f32,
    /// Horizontal gap between the checkbox and the text column.
    pub checkbox_text_gap: f32,
    /// Vertical padding added to each row in addition to text height.
    pub row_v_pad: f32,
    /// Font size used for items in this fence; needed by the renderer
    /// (and the hit-tester via the shared measure callback) to size the
    /// underlying text layouts.
    pub font_size: f32,
    /// "Left" / "Center" / "Right".
    pub align: TodoAlign,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TodoAlign {
    Left,
    Center,
    Right,
}

impl TodoAlign {
    pub fn parse(s: &str) -> Self {
        match s {
            "Right" => TodoAlign::Right,
            "Center" => TodoAlign::Center,
            _ => TodoAlign::Left,
        }
    }
}

/// Computed geometry of one rendered TODO row. Produced by
/// `TodoLayout::compute_rows` and consumed by both the painter and the
/// click hit-tester so they cannot disagree on where rows live.
pub struct TodoRowGeom {
    /// Row top in DIPs (relative to the fence client origin).
    pub y_top: f32,
    /// Total row height (text height + vertical padding).
    pub height: f32,
    /// Checkbox square's top-left, in DIPs.
    pub checkbox: (f32, f32),
    /// Text column's left edge and width, in DIPs.
    pub text_x: f32,
    pub text_w: f32,
    /// Height of the first visual line in DIPs — used by the painter to
    /// vertically center the checkbox against the first text line.
    pub first_line_h: f32,
}

impl TodoLayout {
    pub fn from_fence(fence: &Fence) -> Self {
        let (row_v_pad, checkbox_size, font_size) = match fence.note_font_size.as_str() {
            "Small" => (6.0, 14.0, 12.0),
            "Large" => (8.0, 18.0, 16.0),
            "Huge" => (10.0, 22.0, 20.0),
            _ => (6.0, 16.0, 13.0),
        };
        Self {
            left: 12.0,
            right_inset: 12.0,
            top: TITLE_BAND_DIP + TITLE_GAP_DIP,
            checkbox_size,
            checkbox_text_gap: 8.0,
            row_v_pad,
            font_size,
            align: TodoAlign::parse(&fence.note_text_align),
        }
    }

    /// Available width for the text column, given the fence's overall
    /// width. The checkbox eats `checkbox_size + checkbox_text_gap` and
    /// the row inset eats another `left + right_inset`.
    pub fn text_max_width(&self, fence_width_dip: f32) -> f32 {
        (fence_width_dip
            - self.left
            - self.right_inset
            - self.checkbox_size
            - self.checkbox_text_gap)
            .max(20.0)
    }

    /// Walk the TODO items and produce per-row geometry using
    /// `measure_height` to find each item's wrapped text height. The
    /// callback's signature `(text, max_width) -> height` lets the
    /// caller plug in D2DContext::measure_text_height; reusing the same
    /// closure for both render and hit-test keeps geometry consistent.
    pub fn compute_rows(
        &self,
        fence: &Fence,
        measure_height: &mut dyn FnMut(&str, f32) -> f32,
    ) -> Vec<TodoRowGeom> {
        let text_max_w = self.text_max_width(fence.width as f32);
        let right_edge = (fence.width as f32) - self.right_inset;
        let checkbox_on_right = matches!(self.align, TodoAlign::Right);

        let (checkbox_x, text_x) = if checkbox_on_right {
            // Checkbox at the right edge, text fills the column to its left.
            (right_edge - self.checkbox_size, self.left)
        } else {
            // Checkbox at the left edge, text fills the column to its right.
            (
                self.left,
                self.left + self.checkbox_size + self.checkbox_text_gap,
            )
        };

        let mut out = Vec::with_capacity(fence.note_items.len());
        let mut y = self.top;
        for it in &fence.note_items {
            // Empty rows still take up one font-size worth of room so
            // the user can click them.
            let raw_h = if it.text.is_empty() {
                self.font_size * 1.3
            } else {
                measure_height(&it.text, text_max_w)
            };
            let first_line_h = self.font_size * 1.3;
            let row_h = raw_h.max(self.checkbox_size) + self.row_v_pad;
            out.push(TodoRowGeom {
                y_top: y,
                height: row_h,
                checkbox: (
                    checkbox_x,
                    y + (first_line_h - self.checkbox_size).max(0.0) / 2.0,
                ),
                text_x,
                text_w: text_max_w,
                first_line_h,
            });
            y += row_h;
        }
        out
    }

    /// Hit-test a client-relative DIP point against the precomputed
    /// row layouts. Returns the row index when the cursor lands inside
    /// any row's checkbox square (with a small slop pad).
    pub fn hit_checkbox(&self, rows: &[TodoRowGeom], lxf: f32, lyf: f32) -> Option<usize> {
        const SLOP: f32 = 4.0;
        for (i, r) in rows.iter().enumerate() {
            let (cx, cy) = r.checkbox;
            if lxf >= cx - SLOP
                && lxf < cx + self.checkbox_size + SLOP
                && lyf >= cy - SLOP
                && lyf < cy + self.checkbox_size + SLOP
            {
                return Some(i);
            }
        }
        None
    }
}

pub struct IconLayout {
    pub spacing: f32,
    pub icon_size: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub cols: usize,
    pub icon_y_start: f32,
    pub show_labels: bool,
}

impl IconLayout {
    pub fn from_fence(fence: &Fence) -> Self {
        let icon_size = icon_size_px(&fence.icon_size);
        let spacing = fence.icon_spacing as f32;
        let cell_w = icon_size + spacing * 2.0;
        let show_labels = fence.show_item_labels == "true";
        let label_strip = if show_labels { LABEL_STRIP_DIP } else { 0.0 };
        let cell_h = icon_size + spacing * 2.0 + label_strip;
        let cols = (((fence.width as f32) - spacing) / cell_w).max(1.0) as usize;
        Self {
            spacing,
            icon_size,
            cell_w,
            cell_h,
            cols,
            icon_y_start: TITLE_BAND_DIP + TITLE_GAP_DIP,
            show_labels,
        }
    }

    /// Top-left DIP of the cell that holds item `idx`.
    pub fn cell_top_left(&self, idx: usize) -> (f32, f32) {
        let col = idx % self.cols;
        let row = idx / self.cols;
        (
            self.spacing + col as f32 * self.cell_w,
            self.icon_y_start + row as f32 * self.cell_h,
        )
    }

    /// Resolve a client-relative DIP point to an item index, if it falls
    /// inside a populated cell.
    pub fn hit(&self, lxf: f32, lyf: f32, items_len: usize) -> Option<usize> {
        let lyf_adj = lyf - self.icon_y_start;
        if lyf_adj < 0.0 {
            return None;
        }
        let col_f = (lxf - self.spacing) / self.cell_w;
        if col_f < 0.0 {
            return None;
        }
        let col = col_f as usize;
        if col >= self.cols {
            return None;
        }
        let row = (lyf_adj / self.cell_h) as usize;
        let idx = row * self.cols + col;
        if idx >= items_len { None } else { Some(idx) }
    }
}

pub fn icon_size_px(s: &str) -> f32 {
    match s {
        "Tiny" => 16.0,
        "Small" => 24.0,
        "Large" => 48.0,
        "Huge" => 64.0,
        _ => 32.0,
    }
}
