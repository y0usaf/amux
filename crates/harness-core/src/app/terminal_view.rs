use crate::render::Color;
use crate::terminal::{terminal_selection_span, TerminalSelectionRange};

use super::theme::{screen_cell_colors, TerminalPalette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalCellView<'a> {
    pub(super) row: u16,
    pub(super) col: u16,
    pub(super) span: u16,
    pub(super) text: &'a str,
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) underline: bool,
    pub(super) bold: bool,
}

pub(super) struct TerminalCellViewConfig<'a> {
    pub(super) rows: u16,
    pub(super) cols: u16,
    pub(super) selection_cols: u16,
    pub(super) selection: Option<TerminalSelectionRange>,
    pub(super) default_fg: Color,
    pub(super) default_bg: Color,
    pub(super) selection_fg: Color,
    pub(super) selection_bg: Color,
    pub(super) ansi_palette: &'a [Color; 16],
    pub(super) draw_cursor: bool,
}

pub(super) fn for_each_terminal_screen_cell<F>(
    screen: &vt100::Screen,
    config: &TerminalCellViewConfig<'_>,
    mut visit: F,
) where
    F: FnMut(TerminalCellView<'_>),
{
    let (screen_rows, screen_cols) = screen.size();
    let palette = TerminalPalette {
        default_fg: config.default_fg,
        default_bg: config.default_bg,
        selection_fg: config.selection_fg,
        selection_bg: config.selection_bg,
        ansi: config.ansi_palette,
    };
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = config.draw_cursor && screen.scrollback() == 0 && !screen.hide_cursor();

    for row in 0..screen_rows.min(config.rows) {
        let row_selection = terminal_selection_span(config.selection, row, config.selection_cols);
        for col in 0..screen_cols.min(config.cols) {
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
            let (fg, bg) = screen_cell_colors(cell, cursor_here, selected, &palette);

            visit(TerminalCellView {
                row,
                col,
                span,
                text: if cell.contents().is_empty() {
                    " "
                } else {
                    cell.contents()
                },
                fg,
                bg,
                underline: cell.underline(),
                bold: cell.bold(),
            });
        }
    }
}
