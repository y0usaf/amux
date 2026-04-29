use crate::render::Color;
use crate::terminal::{terminal_selection_span, TerminalSelectionRange};

use super::theme::screen_cell_colors;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TerminalCellView {
    pub(super) row: u16,
    pub(super) col: u16,
    pub(super) span: u16,
    pub(super) text: String,
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) underline: bool,
}

pub(super) fn terminal_screen_cells(
    screen: &vt100::Screen,
    rows: u16,
    cols: u16,
    selection: Option<TerminalSelectionRange>,
    default_fg: Color,
    default_bg: Color,
    ansi_palette: &[Color; 16],
    draw_cursor: bool,
) -> Vec<TerminalCellView> {
    let (screen_rows, screen_cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = draw_cursor && screen.scrollback() == 0 && !screen.hide_cursor();
    let mut cells = Vec::new();

    for row in 0..screen_rows.min(rows) {
        let row_selection = terminal_selection_span(selection, row, screen_cols);
        for col in 0..screen_cols.min(cols) {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }

            let span = if col + 1 < screen_cols
                && screen
                    .cell(row, col + 1)
                    .is_some_and(|next| next.is_wide_continuation())
            {
                2
            } else {
                1
            };
            let selected = row_selection.is_some_and(|(start, width)| {
                let end = start + width;
                let cell_end = col + span;
                end > col && start < cell_end
            });
            let cursor_here = cursor_visible && row == cursor_row && col == cursor_col;
            let (fg, bg) = screen_cell_colors(
                cell,
                cursor_here,
                selected,
                default_fg,
                default_bg,
                ansi_palette,
            );
            let text = if cell.contents().is_empty() {
                " ".to_string()
            } else {
                cell.contents().to_string()
            };

            cells.push(TerminalCellView {
                row,
                col,
                span,
                text,
                fg,
                bg,
                underline: cell.underline(),
            });
        }
    }

    cells
}

pub(super) fn terminal_max_scrollback(screen: &vt100::Screen) -> usize {
    let mut snapshot = screen.clone();
    snapshot.set_scrollback(usize::MAX);
    snapshot.scrollback()
}
