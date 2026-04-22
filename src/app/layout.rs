use crate::render::TextRenderer;

use super::App;

pub(super) const TOPBAR_ROWS: i32 = 2;
pub(super) const SIDEBAR_COLS: i32 = 26;
pub(super) const SIDEBAR_PAD_X: i32 = 2;
pub(super) const SIDEBAR_PAD_Y: i32 = 1;
pub(super) const PANEL_PAD: i32 = 10;
pub(super) const TERMINAL_PAD: i32 = 12;
pub(super) const OUTER_PAD: i32 = 20;
pub(super) const COLUMN_GAP: i32 = 16;
pub(super) const TOPBAR_GAP: i32 = 8;
pub(super) const CENTER_WIDTH_FRAC: f32 = 0.55;
pub(super) const MAX_TERM_COLS: u16 = 220;
pub(super) const MAX_TERM_ROWS: u16 = 120;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Rect {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) w: i32,
    pub(super) h: i32,
}

impl Rect {
    pub(super) fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < (self.x + self.w) as f64
            && y < (self.y + self.h) as f64
    }
}

#[derive(Clone, Debug)]
pub(super) struct Layout {
    pub(super) topbar: Rect,
    pub(super) sidebar: Rect,
    pub(super) terminal_card: Rect,
    pub(super) terminal: Rect,
    pub(super) terminal_cols: u16,
    pub(super) terminal_rows: u16,
}

pub(super) fn compute_layout_for_metrics(
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
) -> Layout {
    let cell_w = cell_width.max(1);
    let cell_h = cell_height.max(1);
    let topbar_h = TOPBAR_ROWS * cell_h + PANEL_PAD * 2;
    let sidebar_w = SIDEBAR_COLS * cell_w + PANEL_PAD * 2;
    let content = Rect {
        x: OUTER_PAD,
        y: OUTER_PAD,
        w: (width - OUTER_PAD * 2).max(0),
        h: (height - OUTER_PAD * 2).max(0),
    };

    let target_card_w = ((content.w as f32) * CENTER_WIDTH_FRAC).round() as i32;
    let avail_term_w = (target_card_w - TERMINAL_PAD * 2)
        .max(cell_w)
        .min((content.w - TERMINAL_PAD * 2).max(cell_w));
    let avail_term_h = (content.h - topbar_h - TOPBAR_GAP - TERMINAL_PAD * 2).max(cell_h);
    let terminal_cols = ((avail_term_w / cell_w).max(1) as u16).clamp(1, MAX_TERM_COLS);
    let terminal_rows = ((avail_term_h / cell_h).max(1) as u16).clamp(1, MAX_TERM_ROWS);

    let inner_w = i32::from(terminal_cols) * cell_w;
    let inner_h = i32::from(terminal_rows) * cell_h;
    let card_w = inner_w + TERMINAL_PAD * 2;
    let card_h = inner_h + TERMINAL_PAD * 2;
    let block_h = topbar_h + TOPBAR_GAP + card_h;
    let terminal_card_x = content.x + (content.w - card_w).max(0) / 2;
    let topbar_y = content.y + (content.h - block_h).max(0) / 2;
    let terminal_card_y = topbar_y + topbar_h + TOPBAR_GAP;

    let topbar = Rect {
        x: terminal_card_x,
        y: topbar_y,
        w: card_w,
        h: topbar_h,
    };
    let sidebar = Rect {
        x: (terminal_card_x - COLUMN_GAP - sidebar_w).max(content.x),
        y: terminal_card_y,
        w: sidebar_w,
        h: card_h,
    };
    let terminal_card = Rect {
        x: terminal_card_x,
        y: terminal_card_y,
        w: card_w,
        h: card_h,
    };
    let terminal = Rect {
        x: terminal_card.x + TERMINAL_PAD,
        y: terminal_card.y + TERMINAL_PAD,
        w: inner_w,
        h: inner_h,
    };

    Layout {
        topbar,
        sidebar,
        terminal_card,
        terminal,
        terminal_cols,
        terminal_rows,
    }
}

impl App {
    pub(super) fn compute_layout(&self, width: i32, height: i32, text: &TextRenderer) -> Layout {
        compute_layout_for_metrics(
            width,
            height,
            text.metrics.cell_width,
            text.metrics.cell_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_includes_top_left_and_excludes_bottom_right() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 30,
            h: 40,
        };

        assert!(rect.contains(10.0, 20.0));
        assert!(rect.contains(39.9, 59.9));
        assert!(!rect.contains(9.9, 20.0));
        assert!(!rect.contains(10.0, 60.0));
        assert!(!rect.contains(40.0, 59.9));
    }

    #[test]
    fn compute_layout_clamps_terminal_dimensions_to_maximums() {
        let layout = compute_layout_for_metrics(20_000, 20_000, 8, 16);

        assert_eq!(layout.terminal_cols, MAX_TERM_COLS);
        assert_eq!(layout.terminal_rows, MAX_TERM_ROWS);
    }

    #[test]
    fn compute_layout_keeps_terminal_padded_inside_card_for_tiny_windows() {
        let layout = compute_layout_for_metrics(40, 40, 8, 16);

        assert_eq!(layout.terminal_cols, 1);
        assert_eq!(layout.terminal_rows, 1);
        assert_eq!(layout.terminal.w, 8);
        assert_eq!(layout.terminal.h, 16);
        assert_eq!(layout.terminal.x, layout.terminal_card.x + TERMINAL_PAD);
        assert_eq!(layout.terminal.y, layout.terminal_card.y + TERMINAL_PAD);
        assert_eq!(layout.topbar.w, layout.terminal_card.w);
        assert_eq!(layout.sidebar.y, layout.terminal_card.y);
        assert_eq!(layout.sidebar.h, layout.terminal_card.h);
    }
}
