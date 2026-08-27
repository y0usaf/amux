use crate::render::Color;
use crate::terminal::{terminal_selection_span, TerminalSelectionRange};

use super::theme::screen_cell_colors;

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

pub(super) fn for_each_terminal_screen_cell<F>(
    screen: &vt100::Screen,
    rows: u16,
    cols: u16,
    selection_cols: u16,
    selection: Option<TerminalSelectionRange>,
    default_fg: Color,
    default_bg: Color,
    selection_fg: Color,
    selection_bg: Color,
    ansi_palette: &[Color; 16],
    draw_cursor: bool,
    mut visit: F,
) where
    F: FnMut(TerminalCellView<'_>),
{
    let (screen_rows, screen_cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = draw_cursor && screen.scrollback() == 0 && !screen.hide_cursor();

    for row in 0..screen_rows.min(rows) {
        let row_selection = terminal_selection_span(selection, row, selection_cols);
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
                selection_fg,
                selection_bg,
                ansi_palette,
            );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{TerminalSelectionPoint, TerminalSelectionRange};

    fn palette() -> [Color; 16] {
        let mut palette = [Color::default(); 16];
        for (i, color) in palette.iter_mut().enumerate() {
            *color = Color::rgb(i as u8, 0, 0);
        }
        palette
    }

    /// Count the cells that receive the selection style when rendering `screen`
    /// over `rows`x`cols` with the given `selection_cols` (the pty width minus
    /// the rail width) and a selection spanning every row.
    fn count_selected(selection_cols: u16, cols: u16) -> usize {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"abcdefghijklmnopqrstuvwxyz0123456789");
        let screen = parser.screen();
        let selection_fg = Color::rgb(250, 0, 0);
        let selection_bg = Color::rgb(0, 250, 0);
        let selection = Some(TerminalSelectionRange {
            start: TerminalSelectionPoint {
                row: 0,
                col: 0,
            },
            end: TerminalSelectionPoint {
                row: 2,
                col: 6,
            },
        });
        let mut selected = 0usize;
        for_each_terminal_screen_cell(
            screen,
            3,
            cols,
            selection_cols,
            selection,
            Color::default(),
            Color::default(),
            selection_fg,
            selection_bg,
            &palette(),
            false,
            |cell| {
                if cell.fg == selection_fg {
                    selected += 1;
                }
            },
        );
        selected
    }

    #[test]
    fn selection_highlights_full_width_when_rail_disabled() {
        // With selection_cols == cols the interior rows (0 and 1) are covered
        // fully (all 10 cols each) and only the last row is clamped to end.col.
        assert_eq!(count_selected(10, 10), 10 + 10 + 6);
    }

    #[test]
    fn selection_clips_the_right_rail_columns() {
        // With a 4-col rail (selection_cols == 6) every row stops at the rail
        // boundary, so the rightmost rail columns are never highlighted.
        assert_eq!(count_selected(6, 10), 6 + 6 + 6);
    }
}
