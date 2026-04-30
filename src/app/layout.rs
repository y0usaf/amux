use crate::config::{LayoutSidebarWidth, LayoutWidths};

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

    pub(super) fn inset_edges(self, left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            col: self.col + left,
            row: self.row + top,
            cols: (self.cols - left - right).max(0),
            rows: (self.rows - top - bottom).max(0),
        }
    }

    pub(super) fn contains_cell(self, col: i32, row: i32) -> bool {
        col >= self.col
            && row >= self.row
            && col < self.col + self.cols
            && row < self.row + self.rows
    }
}

const STATUSBAR_ROWS: i32 = 2;
const SIDEBAR_SHOW_MIN_COLS: i32 = 72;
const SIDEBAR_LEGACY_PERCENT_MIN_COLS: i32 = 18;
const SIDEBAR_LEGACY_PERCENT_MAX_COLS: i32 = 38;
const MIN_TERMINAL_COLS_WITH_SIDEBAR: i32 = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CellLayout {
    pub(super) sidebar: CellRect,
    pub(super) terminal_card: CellRect,
    pub(super) terminal: CellRect,
    pub(super) statusbar: CellRect,
}

pub(super) fn compute_cell_layout(cols: u16, rows: u16, widths: LayoutWidths) -> CellLayout {
    let cols = i32::from(cols).max(1);
    let rows = i32::from(rows).max(1);
    let content = CellRect::new(0, 0, cols, rows);

    let statusbar_rows = STATUSBAR_ROWS.min(content.rows.max(1));
    let body_rows_available = (content.rows - statusbar_rows).max(0);
    let body_rows = body_rows_available;

    let workspace_row = content.row;
    let workspace_rows = body_rows;
    let show_sidebar = content.cols >= SIDEBAR_SHOW_MIN_COLS && workspace_rows >= 4;
    let sidebar_cols = if show_sidebar {
        sidebar_columns(content.cols, widths.sidebar)
    } else {
        0
    };

    let sidebar = if sidebar_cols > 0 {
        CellRect::new(content.col, workspace_row, sidebar_cols, workspace_rows)
    } else {
        CellRect::default()
    };

    let terminal_col = content.col + sidebar_cols;
    let terminal_cols = (content.cols - sidebar_cols).max(0);
    let terminal_card = CellRect::new(terminal_col, workspace_row, terminal_cols, workspace_rows);
    let terminal = CellRect::new(
        terminal_card.col,
        terminal_card.row,
        terminal_card.cols,
        terminal_card.rows,
    );
    let statusbar = CellRect::new(
        content.col,
        content.row + content.rows - statusbar_rows,
        content.cols,
        statusbar_rows,
    );

    CellLayout {
        sidebar,
        terminal_card,
        terminal,
        statusbar,
    }
}

pub(super) fn sidebar_content_rect(sidebar: CellRect) -> CellRect {
    if sidebar.cols <= 0 || sidebar.rows <= 0 {
        return CellRect::default();
    }

    sidebar
}

fn sidebar_columns(total_cols: i32, width: LayoutSidebarWidth) -> i32 {
    let max_for_main = (total_cols - MIN_TERMINAL_COLS_WITH_SIDEBAR).max(0);
    let requested = match width {
        LayoutSidebarWidth::Columns(cols) => i32::from(cols),
        LayoutSidebarWidth::Percent(percent) => percent_cells(total_cols, percent).clamp(
            SIDEBAR_LEGACY_PERCENT_MIN_COLS,
            SIDEBAR_LEGACY_PERCENT_MAX_COLS,
        ),
    };
    requested.min(max_for_main).max(0)
}

fn percent_cells(total_cells: i32, percent: u8) -> i32 {
    (((total_cells.max(1) as i64 * i64::from(percent)) + 50) / 100)
        .max(1)
        .min(i64::from(total_cells.max(1))) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> LayoutWidths {
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Columns(crate::config::LAYOUT_SIDEBAR_WIDTH_DEFAULT),
        }
    }

    #[test]
    fn cell_layout_hides_sidebar_when_compact() {
        let layout = compute_cell_layout(60, 20, widths());
        assert_eq!(layout.sidebar, CellRect::default());
        assert_eq!(layout.statusbar, CellRect::new(0, 18, 60, 2));
        assert_eq!(layout.terminal.row, 0);
        assert_eq!(layout.terminal.rows, 18);
        assert!(layout.terminal.cols > 0);
    }

    #[test]
    fn cell_layout_shows_left_sidebar_and_unboxed_terminal_on_wide_grid() {
        let layout = compute_cell_layout(120, 40, widths());
        assert_eq!(
            layout.sidebar.cols,
            crate::config::LAYOUT_SIDEBAR_WIDTH_DEFAULT as i32
        );
        assert_eq!(layout.sidebar.col, 0);
        assert_eq!(layout.sidebar.row, 0);
        assert_eq!(layout.sidebar.rows, 38);
        assert_eq!(layout.terminal_card.col, layout.sidebar.cols);
        assert_eq!(layout.terminal_card.rows, 38);
        assert_eq!(layout.terminal.row, 0);
        assert_eq!(layout.statusbar, CellRect::new(0, 38, 120, 2));
        assert_eq!(layout.terminal.cols, layout.terminal_card.cols);
    }
}
