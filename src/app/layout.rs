use crate::config::LayoutWidthPercents;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CellRect {
    pub(super) col: i32,
    pub(super) row: i32,
    pub(super) cols: i32,
    pub(super) rows: i32,
}

impl CellRect {
    pub(super) fn new(col: i32, row: i32, cols: i32, rows: i32) -> Self {
        Self {
            col,
            row,
            cols,
            rows,
        }
    }

    pub(super) fn inset(self, cols: i32, rows: i32) -> Self {
        Self {
            col: self.col + cols,
            row: self.row + rows,
            cols: (self.cols - cols * 2).max(0),
            rows: (self.rows - rows * 2).max(0),
        }
    }

    pub(super) fn contains_cell(self, col: i32, row: i32) -> bool {
        col >= self.col
            && row >= self.row
            && col < self.col + self.cols
            && row < self.row + self.rows
    }
}

const GAP_COLS: i32 = 1;
const TOPBAR_ROWS: i32 = 5;
const COMPACT_TOPBAR_ROWS: i32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CellLayout {
    pub(super) topbar: CellRect,
    pub(super) sidebar: CellRect,
    pub(super) terminal_card: CellRect,
    pub(super) terminal: CellRect,
}

pub(super) fn compute_cell_layout(
    cols: u16,
    rows: u16,
    widths: LayoutWidthPercents,
    body_height_percent: u8,
) -> CellLayout {
    let cols = i32::from(cols).max(1);
    let rows = i32::from(rows).max(1);
    let content = CellRect::new(0, 0, cols, rows);
    let topbar_rows = if content.rows >= 12 {
        TOPBAR_ROWS
    } else {
        COMPACT_TOPBAR_ROWS
    }
    .min(content.rows.max(1));
    let body_row = content.row + topbar_rows;
    let body_rows = percent_cells((content.rows - topbar_rows).max(1), body_height_percent);
    let show_sidebar = content.cols >= 72 && body_rows >= 6;

    let (sidebar, terminal_col, terminal_cols) = if show_sidebar {
        let columns = fit_three_panel_columns(
            content,
            percent_cells(content.cols, widths.sidebar),
            percent_cells(content.cols, widths.terminal),
            GAP_COLS,
            1,
        );
        (
            CellRect::new(columns.block_col, body_row, columns.sidebar_cols, body_rows),
            columns.center_col,
            columns.center_cols,
        )
    } else {
        let terminal_cols = percent_cells(content.cols, widths.terminal).min(content.cols.max(1));
        let terminal_col = content.col + (content.cols - terminal_cols).max(0) / 2;
        (CellRect::default(), terminal_col, terminal_cols)
    };

    let topbar = CellRect::new(terminal_col, content.row, terminal_cols, topbar_rows);
    let terminal_card = CellRect::new(terminal_col, body_row, terminal_cols, body_rows);
    let terminal = if terminal_card.cols >= 4 && terminal_card.rows >= 3 {
        CellRect::new(
            terminal_card.col + 1,
            terminal_card.row + 1,
            terminal_card.cols - 3,
            terminal_card.rows - 2,
        )
    } else {
        terminal_card.inset(1, 1)
    };

    CellLayout {
        topbar,
        sidebar,
        terminal_card,
        terminal,
    }
}

fn percent_cells(total_cells: i32, percent: u8) -> i32 {
    (((total_cells.max(1) as i64 * i64::from(percent)) + 50) / 100)
        .max(1)
        .min(i64::from(total_cells.max(1))) as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreePanelColumns {
    block_col: i32,
    sidebar_cols: i32,
    center_col: i32,
    center_cols: i32,
}

fn fit_three_panel_columns(
    content: CellRect,
    requested_sidebar_cols: i32,
    requested_center_cols: i32,
    gap_cols: i32,
    min_center_cols: i32,
) -> ThreePanelColumns {
    let gap_cols = gap_cols.max(0);
    let mut sidebar_cols = requested_sidebar_cols.max(1);
    let mut center_cols = requested_center_cols.max(1);
    let available_cols = (content.cols - gap_cols * 2).max(1);
    let requested_cols = sidebar_cols * 2 + center_cols;

    if requested_cols > available_cols {
        center_cols = center_cols
            .saturating_sub(requested_cols - available_cols)
            .max(min_center_cols.max(1));
    }
    if sidebar_cols * 2 + center_cols > available_cols {
        sidebar_cols = ((available_cols - center_cols).max(0) / 2).max(1);
    }

    let block_cols = sidebar_cols * 2 + center_cols + gap_cols * 2;
    let block_col = content.col + (content.cols - block_cols).max(0) / 2;
    ThreePanelColumns {
        block_col,
        sidebar_cols,
        center_col: block_col + sidebar_cols + gap_cols,
        center_cols,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> LayoutWidthPercents {
        LayoutWidthPercents {
            terminal: crate::config::LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT,
            sidebar: crate::config::LAYOUT_SIDEBAR_WIDTH_PERCENT_DEFAULT,
        }
    }

    #[test]
    fn cell_layout_hides_sidebar_when_compact() {
        let layout = compute_cell_layout(60, 20, widths(), 100);
        assert_eq!(layout.sidebar, CellRect::default());
        assert!(layout.terminal.cols > 0 && layout.terminal.rows > 0);
    }

    #[test]
    fn cell_layout_shows_sidebar_and_centers_terminal_on_wide_grid() {
        let layout = compute_cell_layout(120, 40, widths(), 100);
        assert!(layout.sidebar.cols > 0);
        assert!(layout.terminal_card.col > layout.sidebar.col + layout.sidebar.cols);
        assert_eq!(layout.topbar.col, layout.terminal_card.col);
    }
}
