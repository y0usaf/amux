use crate::config::{panel_columns, LayoutSidebarWidth, LayoutWidths};

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

const STATUSBAR_ROWS: i32 = 1;
const SIDEBAR_SHOW_MIN_COLS: i32 = 72;

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
        LayoutSidebarWidth::Percent(percent) => {
            let total = total_cols.clamp(0, i32::from(u16::MAX)) as u16;
            i32::from(panel_columns(total, percent))
        }
    };
    requested.min(max_for_main).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> LayoutWidths {
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Percent(crate::config::PANEL_WIDTH_PERCENT_DEFAULT),
        }
    }

    #[test]
    fn cell_layout_hides_sidebar_when_compact() {
        let layout = compute_cell_layout(60, 20, widths());
        assert_eq!(layout.sidebar, CellRect::default());
        assert_eq!(layout.statusbar, CellRect::new(0, 19, 60, 1));
        assert_eq!(layout.terminal.row, 0);
        assert_eq!(layout.terminal.rows, 19);
        assert!(layout.terminal.cols > 0);
    }

    #[test]
    fn cell_layout_shows_left_sidebar_and_unboxed_terminal_on_wide_grid() {
        let layout = compute_cell_layout(120, 40, widths());
        assert_eq!(
            layout.sidebar.cols,
            i32::from(panel_columns(
                120,
                crate::config::PANEL_WIDTH_PERCENT_DEFAULT
            ))
        );
        assert_eq!(layout.sidebar.col, 0);
        assert_eq!(layout.sidebar.row, 0);
        assert_eq!(layout.sidebar.rows, 39);
        assert_eq!(layout.terminal_card.col, layout.sidebar.cols);
        assert_eq!(layout.terminal_card.cols, 120 - layout.sidebar.cols);
        assert_eq!(layout.terminal_card.rows, 39);
        assert_eq!(layout.terminal.row, 0);
        assert_eq!(layout.statusbar, CellRect::new(0, 39, 120, 1));
        assert_eq!(layout.terminal.cols, layout.terminal_card.cols);
    }
}
