use crate::config::clamp_panel_padding_px;
use crate::render::TextRenderer;

use super::App;

pub(super) const TOPBAR_ROWS: i32 = 3;
pub(super) const SIDEBAR_COLS: i32 = 26;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LayoutSpacing {
    pub(super) panel_pad: i32,
    pub(super) terminal_pad: i32,
    pub(super) outer_pad: i32,
    pub(super) column_gap: i32,
    pub(super) topbar_gap: i32,
}

impl LayoutSpacing {
    fn from_panel_padding_px(panel_padding_px: Option<i32>) -> Self {
        let mut spacing = Self::default();
        if let Some(px) = panel_padding_px.map(clamp_panel_padding_px) {
            spacing.panel_pad = px;
        }
        spacing
    }
}

impl Default for LayoutSpacing {
    fn default() -> Self {
        Self {
            panel_pad: PANEL_PAD,
            terminal_pad: TERMINAL_PAD,
            outer_pad: OUTER_PAD,
            column_gap: COLUMN_GAP,
            topbar_gap: TOPBAR_GAP,
        }
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
    pub(super) spacing: LayoutSpacing,
}

#[cfg(test)]
pub(super) fn compute_layout_for_metrics(
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
) -> Layout {
    compute_layout_for_metrics_with_panel_padding_px(width, height, cell_width, cell_height, None)
}

pub(super) fn compute_layout_for_metrics_with_panel_padding_px(
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
    panel_padding_px: Option<i32>,
) -> Layout {
    let spacing = LayoutSpacing::from_panel_padding_px(panel_padding_px);
    let cell_w = cell_width.max(1);
    let cell_h = cell_height.max(1);
    let topbar_h = TOPBAR_ROWS * cell_h + spacing.panel_pad * 2;
    let sidebar_w = SIDEBAR_COLS * cell_w + spacing.panel_pad * 2;
    let content = Rect {
        x: spacing.outer_pad,
        y: spacing.outer_pad,
        w: (width - spacing.outer_pad * 2).max(0),
        h: (height - spacing.outer_pad * 2).max(0),
    };

    let target_card_w = ((content.w as f32) * CENTER_WIDTH_FRAC).round() as i32;
    let avail_term_w = (target_card_w - spacing.terminal_pad * 2)
        .max(cell_w)
        .min((content.w - spacing.terminal_pad * 2).max(cell_w));
    let avail_term_h =
        (content.h - topbar_h - spacing.topbar_gap - spacing.terminal_pad * 2).max(cell_h);
    let terminal_cols = (avail_term_w / cell_w).max(1).min(i32::from(MAX_TERM_COLS)) as u16;
    let terminal_rows = (avail_term_h / cell_h).max(1).min(i32::from(MAX_TERM_ROWS)) as u16;

    let inner_w = i32::from(terminal_cols) * cell_w;
    let inner_h = i32::from(terminal_rows) * cell_h;
    let card_w = inner_w + spacing.terminal_pad * 2;
    let card_h = inner_h + spacing.terminal_pad * 2;
    let block_h = topbar_h + spacing.topbar_gap + card_h;
    let terminal_card_x = content.x + (content.w - card_w).max(0) / 2;
    let topbar_y = content.y + (content.h - block_h).max(0) / 2;
    let terminal_card_y = topbar_y + topbar_h + spacing.topbar_gap;

    let topbar = Rect {
        x: terminal_card_x,
        y: topbar_y,
        w: card_w,
        h: topbar_h,
    };
    let sidebar = Rect {
        x: (terminal_card_x - spacing.column_gap - sidebar_w).max(content.x),
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
        x: terminal_card.x + spacing.terminal_pad,
        y: terminal_card.y + spacing.terminal_pad,
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
        spacing,
    }
}

impl App {
    pub(super) fn compute_layout(&self, width: i32, height: i32, text: &TextRenderer) -> Layout {
        compute_layout_for_metrics_with_panel_padding_px(
            width,
            height,
            text.metrics.cell_width,
            text.metrics.cell_height,
            self.config.panel_padding_px(),
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

        let huge = compute_layout_for_metrics(i32::MAX, i32::MAX, 8, 16);
        assert_eq!(huge.terminal_cols, MAX_TERM_COLS);
        assert_eq!(huge.terminal_rows, MAX_TERM_ROWS);
    }

    #[test]
    fn compute_layout_keeps_terminal_padded_inside_card_for_tiny_windows() {
        let layout = compute_layout_for_metrics(40, 40, 8, 16);

        assert_eq!(layout.terminal_cols, 1);
        assert_eq!(layout.terminal_rows, 1);
        assert_eq!(layout.terminal.w, 8);
        assert_eq!(layout.terminal.h, 16);
        assert_eq!(
            layout.terminal.x,
            layout.terminal_card.x + layout.spacing.terminal_pad
        );
        assert_eq!(
            layout.terminal.y,
            layout.terminal_card.y + layout.spacing.terminal_pad
        );
        assert_eq!(layout.topbar.w, layout.terminal_card.w);
        assert_eq!(layout.sidebar.y, layout.terminal_card.y);
        assert_eq!(layout.sidebar.h, layout.terminal_card.h);
    }

    #[test]
    fn compact_panel_padding_px_tightens_panels_without_changing_margins() {
        let normal = compute_layout_for_metrics(1280, 840, 8, 16);
        let compact = compute_layout_for_metrics_with_panel_padding_px(1280, 840, 8, 16, Some(6));

        assert_eq!(normal.spacing.panel_pad, PANEL_PAD);
        assert_eq!(compact.spacing.panel_pad, 6);
        assert_eq!(compact.spacing.terminal_pad, TERMINAL_PAD);
        assert_eq!(compact.spacing.outer_pad, OUTER_PAD);
        assert_eq!(compact.spacing.column_gap, COLUMN_GAP);
        assert_eq!(compact.spacing.topbar_gap, TOPBAR_GAP);
        assert!(compact.sidebar.w < normal.sidebar.w);
        assert!(compact.topbar.h < normal.topbar.h);
        assert_eq!(compact.terminal.w, i32::from(compact.terminal_cols) * 8);
        assert_eq!(compact.terminal.h, i32::from(compact.terminal_rows) * 16);
    }
}
