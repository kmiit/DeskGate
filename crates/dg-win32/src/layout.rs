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
