#![allow(clippy::too_many_arguments)]

use crate::render::Color;

use super::layout::CellRect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) text: String,
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) bold: bool,
    pub(super) underline: bool,
    pub(super) reverse: bool,
    pub(super) continuation: bool,
}

impl Cell {
    fn blank(fg: Color, bg: Color) -> Self {
        Self {
            text: " ".to_string(),
            fg,
            bg,
            bold: false,
            underline: false,
            reverse: false,
            continuation: false,
        }
    }
}

#[derive(Clone)]
pub(super) struct CellSurface {
    pub(super) cols: i32,
    pub(super) rows: i32,
    pub(super) cells: Vec<Cell>,
}

impl CellSurface {
    pub(super) fn new(cols: i32, rows: i32, fg: Color, bg: Color) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            cells: vec![Cell::blank(fg, bg); (cols * rows) as usize],
        }
    }

    fn index(&self, col: i32, row: i32) -> Option<usize> {
        (col >= 0 && row >= 0 && col < self.cols && row < self.rows)
            .then_some((row * self.cols + col) as usize)
    }

    fn cell_mut(&mut self, col: i32, row: i32) -> Option<&mut Cell> {
        let index = self.index(col, row)?;
        self.cells.get_mut(index)
    }

    pub(super) fn fill_rect(&mut self, rect: CellRect, fg: Color, bg: Color) {
        let row0 = rect.row.max(0);
        let row1 = (rect.row + rect.rows).min(self.rows).max(row0);
        let col0 = rect.col.max(0);
        let col1 = (rect.col + rect.cols).min(self.cols).max(col0);
        for row in row0..row1 {
            for col in col0..col1 {
                if let Some(cell) = self.cell_mut(col, row) {
                    *cell = Cell::blank(fg, bg);
                }
            }
        }
    }

    pub(super) fn set_cell(
        &mut self,
        col: i32,
        row: i32,
        fg: Color,
        bg: Color,
        text: impl Into<String>,
        underline: bool,
    ) {
        self.set_cell_styled(col, row, fg, bg, text, underline, false);
    }

    pub(super) fn set_cell_styled(
        &mut self,
        col: i32,
        row: i32,
        fg: Color,
        bg: Color,
        text: impl Into<String>,
        underline: bool,
        reverse: bool,
    ) {
        if let Some(cell) = self.cell_mut(col, row) {
            *cell = Cell {
                text: text.into(),
                fg,
                bg,
                bold: false,
                underline,
                reverse,
                continuation: false,
            };
        }
    }

    pub(super) fn set_reverse_rect(&mut self, rect: CellRect, reverse: bool) {
        let row0 = rect.row.max(0);
        let row1 = (rect.row + rect.rows).min(self.rows).max(row0);
        let col0 = rect.col.max(0);
        let col1 = (rect.col + rect.cols).min(self.cols).max(col0);
        for row in row0..row1 {
            for col in col0..col1 {
                if let Some(cell) = self.cell_mut(col, row) {
                    cell.reverse = reverse;
                }
            }
        }
    }

    pub(super) fn put_cell_span(
        &mut self,
        col: i32,
        row: i32,
        span: i32,
        text: &str,
        fg: Color,
        bg: Color,
        underline: bool,
    ) {
        self.put_cell_span_styled(col, row, span, text, fg, bg, underline, false);
    }

    pub(super) fn put_cell_span_styled(
        &mut self,
        col: i32,
        row: i32,
        span: i32,
        text: &str,
        fg: Color,
        bg: Color,
        underline: bool,
        reverse: bool,
    ) {
        let span = span.max(1);
        for offset in 0..span {
            if let Some(cell) = self.cell_mut(col + offset, row) {
                *cell = Cell {
                    text: if offset == 0 {
                        if text.is_empty() {
                            " ".to_string()
                        } else {
                            text.to_string()
                        }
                    } else {
                        " ".to_string()
                    },
                    fg,
                    bg,
                    bold: false,
                    underline,
                    reverse,
                    continuation: offset > 0,
                };
            }
        }
    }

    pub(super) fn put_text(
        &mut self,
        col: i32,
        row: i32,
        max_cols: i32,
        fg: Color,
        bg: Color,
        value: &str,
    ) {
        self.put_text_styled(col, row, max_cols, fg, bg, value, false);
    }

    pub(super) fn put_text_styled(
        &mut self,
        col: i32,
        row: i32,
        max_cols: i32,
        fg: Color,
        bg: Color,
        value: &str,
        reverse: bool,
    ) {
        if max_cols <= 0 {
            return;
        }
        let mut cursor = col;
        let end = col + max_cols;
        for ch in value.chars() {
            if ch == '\n' || cursor >= end {
                break;
            }
            let width = char_cell_width(ch);
            if cursor + width > end {
                break;
            }
            let mut buf = [0; 4];
            self.put_cell_span_styled(
                cursor,
                row,
                width,
                ch.encode_utf8(&mut buf),
                fg,
                bg,
                false,
                reverse,
            );
            cursor += width;
        }
    }

    pub(super) fn put_text_bold(
        &mut self,
        col: i32,
        row: i32,
        max_cols: i32,
        fg: Color,
        bg: Color,
        value: &str,
    ) {
        self.put_text_bold_styled(col, row, max_cols, fg, bg, value, false);
    }

    pub(super) fn put_text_bold_styled(
        &mut self,
        col: i32,
        row: i32,
        max_cols: i32,
        fg: Color,
        bg: Color,
        value: &str,
        reverse: bool,
    ) {
        if max_cols <= 0 {
            return;
        }
        let mut cursor = col;
        let end = col + max_cols;
        for ch in value.chars() {
            if ch == '\n' || cursor >= end {
                break;
            }
            let width = char_cell_width(ch);
            if cursor + width > end {
                break;
            }
            let mut buf = [0; 4];
            self.put_cell_span_styled(
                cursor,
                row,
                width,
                ch.encode_utf8(&mut buf),
                fg,
                bg,
                false,
                reverse,
            );
            if let Some(cell) = self.cell_mut(cursor, row) {
                cell.bold = true;
            }
            cursor += width;
        }
    }
}

pub(super) fn draw_box(
    surface: &mut CellSurface,
    rect: CellRect,
    fill_fg: Color,
    bg: Color,
    border: Color,
) {
    surface.fill_rect(rect, fill_fg, bg);
    if rect.cols <= 0 || rect.rows <= 0 {
        return;
    }
    let left = rect.col;
    let right = rect.col + rect.cols - 1;
    let top = rect.row;
    let bottom = rect.row + rect.rows - 1;

    if rect.rows == 1 {
        for col in left..=right {
            surface.set_cell(col, top, border, bg, "─", false);
        }
        return;
    }
    if rect.cols == 1 {
        for row in top..=bottom {
            surface.set_cell(left, row, border, bg, "│", false);
        }
        return;
    }

    for col in (left + 1)..right {
        surface.set_cell(col, top, border, bg, "─", false);
        surface.set_cell(col, bottom, border, bg, "─", false);
    }
    for row in (top + 1)..bottom {
        surface.set_cell(left, row, border, bg, "│", false);
        surface.set_cell(right, row, border, bg, "│", false);
    }
    surface.set_cell(left, top, border, bg, "┌", false);
    surface.set_cell(right, top, border, bg, "┐", false);
    surface.set_cell(left, bottom, border, bg, "└", false);
    surface.set_cell(right, bottom, border, bg, "┘", false);
}

pub(super) fn render_cell_scrollbar(
    surface: &mut CellSurface,
    col: i32,
    row: i32,
    rows: i32,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    fg: Color,
    bg: Color,
    track_glyph: &str,
    thumb_fg: Color,
    thumb_glyph: &str,
) {
    if rows <= 0 || visible_items == 0 || total_items <= visible_items {
        return;
    }

    for offset in 0..rows {
        surface.set_cell(col, row + offset, fg, bg, track_glyph, false);
    }

    let thumb_rows =
        (((rows as i64 * visible_items as i64) / total_items as i64).max(1) as i32).min(rows);
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_row = row
        + (((rows - thumb_rows).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    for offset in 0..thumb_rows {
        surface.set_cell(col, thumb_row + offset, thumb_fg, bg, thumb_glyph, false);
    }
}

pub(super) fn char_cell_width(ch: char) -> i32 {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1) as i32
}

pub(super) fn display_cell_width(value: &str) -> usize {
    value
        .chars()
        .map(|ch| {
            unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1)
        })
        .sum()
}

pub(super) fn truncate_to_cells(value: &str, max_cols: usize) -> String {
    if display_cell_width(value) <= max_cols {
        return value.to_string();
    }
    if max_cols == 0 {
        return String::new();
    }
    let ellipsis = "…";
    let ellipsis_w = display_cell_width(ellipsis);
    if max_cols <= ellipsis_w {
        return ellipsis.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in value.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch)
            .unwrap_or(1)
            .max(1);
        if width + ch_width + ellipsis_w > max_cols {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push_str(ellipsis);
    out
}
