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
    /// Inset from the fence's left edge to the start of the row content.
    pub left: f32,
    /// Top of the first row (just below the title band).
    pub top: f32,
    /// Per-row height in DIPs.
    pub row_h: f32,
    /// Logical width of the checkbox square (height matches).
    pub checkbox_size: f32,
    /// Horizontal gap between the checkbox right edge and the text left.
    pub checkbox_text_gap: f32,
}

impl TodoLayout {
    pub fn from_fence(fence: &Fence) -> Self {
        let row_h = match fence.note_font_size.as_str() {
            "Small" => 22.0,
            "Large" => 28.0,
            "Huge" => 34.0,
            _ => 24.0,
        };
        let checkbox_size = match fence.note_font_size.as_str() {
            "Small" => 14.0,
            "Large" => 18.0,
            "Huge" => 22.0,
            _ => 16.0,
        };
        Self {
            left: 12.0,
            top: TITLE_BAND_DIP + TITLE_GAP_DIP,
            row_h,
            checkbox_size,
            checkbox_text_gap: 8.0,
        }
    }

    /// Top-left DIP of the checkbox square for TODO row `idx`.
    pub fn checkbox_top_left(&self, idx: usize) -> (f32, f32) {
        let y = self.top + (idx as f32) * self.row_h + (self.row_h - self.checkbox_size) / 2.0;
        (self.left, y)
    }

    /// Hit-test a client-relative DIP point against the checkbox column.
    /// Returns the row index if the point is within the checkbox (with a
    /// small slop pad) of an existing row, otherwise None.
    pub fn hit_checkbox(&self, lxf: f32, lyf: f32, items_len: usize) -> Option<usize> {
        const SLOP: f32 = 4.0;
        let lyf_adj = lyf - self.top;
        if lyf_adj < 0.0 {
            return None;
        }
        let row = (lyf_adj / self.row_h) as usize;
        if row >= items_len {
            return None;
        }
        let (cx, cy) = self.checkbox_top_left(row);
        if lxf >= cx - SLOP
            && lxf < cx + self.checkbox_size + SLOP
            && lyf >= cy - SLOP
            && lyf < cy + self.checkbox_size + SLOP
        {
            Some(row)
        } else {
            None
        }
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
