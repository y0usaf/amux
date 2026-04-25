use crate::render::TextRenderer;

use super::App;

pub(super) const TOPBAR_ROWS: i32 = 3;
pub(super) const SIDEBAR_COLS: i32 = 26;
pub(super) const PANEL_PAD_CELLS: i32 = 0;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CellRect {
    pub(super) col: i32,
    pub(super) row: i32,
    pub(super) cols: i32,
    pub(super) rows: i32,
}

impl CellRect {
    pub(super) fn inset(self, cols: i32, rows: i32) -> Self {
        Self {
            col: self.col + cols,
            row: self.row + rows,
            cols: (self.cols - cols * 2).max(0),
            rows: (self.rows - rows * 2).max(0),
        }
    }
}

pub(super) fn panel_content_rect(rect: CellRect, panel_pad_cells: i32) -> CellRect {
    let inset = 1 + panel_pad_cells.max(0);
    rect.inset(inset, inset)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CellGrid {
    pub(super) origin_x: i32,
    pub(super) origin_y: i32,
    pub(super) cols: i32,
    pub(super) rows: i32,
    pub(super) cell_w: i32,
    pub(super) cell_h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LayoutSpacing {
    pub(super) panel_pad_cells: i32,
    pub(super) terminal_pad_cols: i32,
    pub(super) terminal_pad_rows: i32,
    pub(super) outer_pad_cols: i32,
    pub(super) outer_pad_rows: i32,
    pub(super) column_gap_cols: i32,
    pub(super) topbar_gap_rows: i32,
}

fn clamp_panel_padding_cells(panel_padding_cells: i32) -> i32 {
    panel_padding_cells.clamp(0, 4)
}

impl LayoutSpacing {
    fn from_metrics(cell_w: i32, cell_h: i32, panel_padding_cells: Option<i32>) -> Self {
        Self {
            panel_pad_cells: panel_padding_cells
                .map(clamp_panel_padding_cells)
                .unwrap_or(PANEL_PAD_CELLS),
            terminal_pad_cols: cells_for_px(TERMINAL_PAD, cell_w).max(1),
            terminal_pad_rows: cells_for_px(TERMINAL_PAD, cell_h),
            outer_pad_cols: cells_for_px(OUTER_PAD, cell_w),
            outer_pad_rows: cells_for_px(OUTER_PAD, cell_h),
            column_gap_cols: cells_for_px(COLUMN_GAP, cell_w).max(1),
            topbar_gap_rows: cells_for_px(TOPBAR_GAP, cell_h),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Layout {
    pub(super) grid: CellGrid,
    pub(super) sidebar: Rect,
    pub(super) terminal_card: Rect,
    pub(super) terminal: Rect,
    pub(super) topbar_cells: CellRect,
    pub(super) sidebar_cells: CellRect,
    pub(super) terminal_card_cells: CellRect,
    pub(super) terminal_cells: CellRect,
    pub(super) terminal_cols: u16,
    pub(super) terminal_rows: u16,
    pub(super) spacing: LayoutSpacing,
}

fn cells_for_px(px: i32, cell: i32) -> i32 {
    if px <= 0 {
        return 0;
    }
    (px + cell.max(1) / 2) / cell.max(1)
}

fn rect_for_cells(grid: CellGrid, rect: CellRect) -> Rect {
    Rect {
        x: grid.origin_x + rect.col * grid.cell_w,
        y: grid.origin_y + rect.row * grid.cell_h,
        w: rect.cols * grid.cell_w,
        h: rect.rows * grid.cell_h,
    }
}

#[cfg(test)]
pub(super) fn compute_layout_for_metrics(
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
) -> Layout {
    compute_layout_for_metrics_with_panel_padding_cells(
        width,
        height,
        cell_width,
        cell_height,
        None,
    )
}

pub(super) fn compute_layout_for_metrics_with_panel_padding_cells(
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
    panel_padding_cells: Option<i32>,
) -> Layout {
    let cell_w = cell_width.max(1);
    let cell_h = cell_height.max(1);
    let grid_cols = (width.max(0) / cell_w).max(1);
    let grid_rows = (height.max(0) / cell_h).max(1);
    let grid = CellGrid {
        origin_x: (width - grid_cols * cell_w).max(0) / 2,
        origin_y: (height - grid_rows * cell_h).max(0) / 2,
        cols: grid_cols,
        rows: grid_rows,
        cell_w,
        cell_h,
    };
    let spacing = LayoutSpacing::from_metrics(cell_w, cell_h, panel_padding_cells);

    let topbar_rows = TOPBAR_ROWS + 2 + spacing.panel_pad_cells * 2;
    let sidebar_cols = SIDEBAR_COLS + 2 + spacing.panel_pad_cells * 2;
    let content = CellRect {
        col: spacing.outer_pad_cols,
        row: spacing.outer_pad_rows,
        cols: (grid.cols - spacing.outer_pad_cols * 2).max(0),
        rows: (grid.rows - spacing.outer_pad_rows * 2).max(0),
    };

    let target_card_cols = ((content.cols as f32) * CENTER_WIDTH_FRAC).round() as i32;
    let card_chrome_cols = 2 + spacing.terminal_pad_cols * 2;
    let card_chrome_rows = 2 + spacing.terminal_pad_rows * 2;
    let avail_term_cols = (target_card_cols - card_chrome_cols)
        .max(1)
        .min((content.cols - card_chrome_cols).max(1));
    let avail_term_rows =
        (content.rows - topbar_rows - spacing.topbar_gap_rows - card_chrome_rows).max(1);
    let terminal_cols = avail_term_cols.min(i32::from(MAX_TERM_COLS)).max(1) as u16;
    let terminal_rows = avail_term_rows.min(i32::from(MAX_TERM_ROWS)).max(1) as u16;

    let card_cols = i32::from(terminal_cols) + card_chrome_cols;
    let card_rows = i32::from(terminal_rows) + card_chrome_rows;
    let block_rows = topbar_rows + spacing.topbar_gap_rows + card_rows;
    let terminal_card_col = content.col + (content.cols - card_cols).max(0) / 2;
    let topbar_row = content.row + (content.rows - block_rows).max(0) / 2;
    let terminal_card_row = topbar_row + topbar_rows + spacing.topbar_gap_rows;

    let topbar_cells = CellRect {
        col: terminal_card_col,
        row: topbar_row,
        cols: card_cols,
        rows: topbar_rows,
    };
    let sidebar_cells = CellRect {
        col: (terminal_card_col - spacing.column_gap_cols - sidebar_cols).max(content.col),
        row: terminal_card_row,
        cols: sidebar_cols,
        rows: card_rows,
    };
    let terminal_card_cells = CellRect {
        col: terminal_card_col,
        row: terminal_card_row,
        cols: card_cols,
        rows: card_rows,
    };
    let terminal_cells = CellRect {
        col: terminal_card_cells.col + 1 + spacing.terminal_pad_cols,
        row: terminal_card_cells.row + 1 + spacing.terminal_pad_rows,
        cols: i32::from(terminal_cols),
        rows: i32::from(terminal_rows),
    };

    Layout {
        grid,
        sidebar: rect_for_cells(grid, sidebar_cells),
        terminal_card: rect_for_cells(grid, terminal_card_cells),
        terminal: rect_for_cells(grid, terminal_cells),
        topbar_cells,
        sidebar_cells,
        terminal_card_cells,
        terminal_cells,
        terminal_cols,
        terminal_rows,
        spacing,
    }
}

impl App {
    pub(super) fn compute_layout(&self, width: i32, height: i32, text: &TextRenderer) -> Layout {
        compute_layout_for_metrics_with_panel_padding_cells(
            width,
            height,
            text.metrics.cell_width,
            text.metrics.cell_height,
            None,
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
    fn compute_layout_keeps_terminal_inside_card_for_tiny_windows() {
        let layout = compute_layout_for_metrics(40, 40, 8, 16);

        assert_eq!(layout.terminal_cols, 1);
        assert_eq!(layout.terminal_rows, 1);
        assert_eq!(layout.terminal.w, 8);
        assert_eq!(layout.terminal.h, 16);
        assert_eq!(layout.terminal_cells.cols, 1);
        assert_eq!(layout.terminal_cells.rows, 1);
        assert_eq!(layout.topbar_cells.cols, layout.terminal_card_cells.cols);
        assert_eq!(layout.sidebar.y, layout.terminal_card.y);
        assert_eq!(layout.sidebar.h, layout.terminal_card.h);
    }

    #[test]
    fn panel_padding_cells_adjusts_panels_without_changing_outer_spacing() {
        let compact = compute_layout_for_metrics(1280, 840, 8, 16);
        let padded = compute_layout_for_metrics_with_panel_padding_cells(1280, 840, 8, 16, Some(1));

        assert_eq!(compact.spacing.panel_pad_cells, PANEL_PAD_CELLS);
        assert_eq!(padded.spacing.panel_pad_cells, 1);
        assert_eq!(
            padded.spacing.terminal_pad_cols,
            compact.spacing.terminal_pad_cols
        );
        assert_eq!(
            padded.spacing.outer_pad_cols,
            compact.spacing.outer_pad_cols
        );
        assert_eq!(
            padded.spacing.column_gap_cols,
            compact.spacing.column_gap_cols
        );
        assert_eq!(
            padded.spacing.topbar_gap_rows,
            compact.spacing.topbar_gap_rows
        );
        assert!(padded.sidebar.w > compact.sidebar.w);
        assert!(padded.topbar_cells.rows > compact.topbar_cells.rows);
        assert_eq!(compact.terminal.w, i32::from(compact.terminal_cols) * 8);
        assert_eq!(compact.terminal.h, i32::from(compact.terminal_rows) * 16);
    }
}
