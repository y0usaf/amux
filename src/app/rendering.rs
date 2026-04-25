use std::num::NonZeroU32;

use crate::render::{Color, Frame, TextRenderer};
use crate::state::Session;
use crate::terminal::{
    terminal_selection_span, TerminalController, TerminalSelectionRange, TerminalStatus,
};
use crate::util::now_millis;

use super::layout::{panel_content_rect, CellGrid, CellRect, Layout};
use super::sidebar::{sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind};
use super::theme::{
    screen_cell_colors, status_color, ACCENT, BG, BORDER, MUTED, SURFACE, SURFACE_ALT, TERM_BG,
    TERM_FG, TEXT,
};
use super::App;

const SCROLLBAR_TRACK_GLYPH: &str = "▕";
const SCROLLBAR_THUMB_GLYPH: &str = "▐";

impl App {
    pub(super) fn render(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        let (layout, sidebar_visible_rows) = {
            let Some(text) = self.text.as_ref() else {
                return;
            };
            let layout = self.compute_layout(size.width as i32, size.height as i32, text);
            let sidebar_visible_rows =
                self.sidebar_visible_rows(layout.sidebar_cells, layout.spacing.panel_pad_cells);
            (layout, sidebar_visible_rows)
        };
        self.sync_terminals();
        self.resize_terminals(layout.terminal_rows, layout.terminal_cols);

        let sidebar_rows = self.sidebar_rows();
        if self.sidebar_sync_to_selection {
            self.ensure_sidebar_selection_visible(&sidebar_rows, sidebar_visible_rows);
            self.sidebar_sync_to_selection = false;
        } else {
            self.clamp_sidebar_scroll(sidebar_rows.len(), sidebar_visible_rows);
        }
        let sticky_sidebar_anchor =
            self.sticky_sidebar_anchor_row_index(&sidebar_rows, sidebar_visible_rows);
        let hovered_sidebar_row = self.text.as_ref().and_then(|text| {
            self.hovered_sidebar_row_index(
                layout.sidebar,
                text,
                &sidebar_rows,
                sidebar_visible_rows,
                layout.spacing.panel_pad_cells,
            )
        });

        let topbar_project = self
            .current_project()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "pi-harness".to_string());
        let topbar_session = self
            .current_session()
            .map(|session| session.name.clone())
            .unwrap_or_default();
        let topbar_status = self.status_text();
        let topbar_status_fg = status_color(self.current_session(), self.current_terminal_status());
        let terminal_selection = self
            .current_terminal()
            .and_then(TerminalController::selection_range);
        let screen = self
            .current_terminal()
            .map(|terminal| terminal.screen().clone())
            .unwrap_or_else(|| {
                vt100::Parser::new(layout.terminal_rows, layout.terminal_cols, 0)
                    .screen()
                    .clone()
            });

        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(text) = self.text.as_mut() else {
            return;
        };

        let _ = surface.resize(
            NonZeroU32::new(size.width).unwrap(),
            NonZeroU32::new(size.height).unwrap(),
        );
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let width = buffer.width().get() as usize;
        let height = buffer.height().get() as usize;

        let pixels: &mut [u32] = &mut buffer;
        let mut frame = Frame::new(pixels, width, height);
        frame.clear(BG);

        let mut cells = CellSurface::new(layout.grid.cols, layout.grid.rows, TEXT, BG);
        render_topbar_frame(
            &mut cells,
            text,
            &layout,
            &topbar_project,
            &topbar_status,
            &topbar_session,
            topbar_status_fg,
        );
        render_sidebar_frame(
            &mut cells,
            text,
            &layout,
            &sidebar_rows,
            self.sidebar_scroll,
            sticky_sidebar_anchor,
            hovered_sidebar_row,
            now_millis(),
        );
        render_terminal_frame(&mut cells, &layout, &screen, terminal_selection);
        cells.render(&mut frame, text, layout.grid);

        let _ = buffer.present();
        self.needs_redraw = false;
    }

    fn status_text(&self) -> String {
        status_text_for_session(
            self.current_project().is_some(),
            self.current_session(),
            self.current_terminal_status(),
        )
    }
}

fn status_text_for_session(
    has_project: bool,
    session: Option<&Session>,
    terminal_status: Option<&TerminalStatus>,
) -> String {
    if let Some(session) = session {
        if let Some(status) = session.runtime.status.as_deref() {
            if session.runtime.queued {
                return format!("{} · queued", status);
            }
            return status.to_string();
        }
        if session.draft {
            return "new session".to_string();
        }

        return terminal_status_label(terminal_status).to_string();
    }

    if has_project {
        return "select a session".to_string();
    }

    "open a project".to_string()
}

fn terminal_status_label(status: Option<&TerminalStatus>) -> &'static str {
    match status {
        Some(TerminalStatus::Launching) => "launching",
        Some(TerminalStatus::Running) => "running",
        Some(TerminalStatus::Exited(_)) => "exited",
        Some(TerminalStatus::Error(_)) => "error",
        Some(TerminalStatus::Empty) | None => "idle",
    }
}

#[derive(Clone, Debug)]
struct Cell {
    text: String,
    fg: Color,
    bg: Color,
    underline: bool,
    continuation: bool,
}

impl Cell {
    fn blank(fg: Color, bg: Color) -> Self {
        Self {
            text: " ".to_string(),
            fg,
            bg,
            underline: false,
            continuation: false,
        }
    }
}

struct CellSurface {
    cols: i32,
    rows: i32,
    cells: Vec<Cell>,
}

impl CellSurface {
    fn new(cols: i32, rows: i32, fg: Color, bg: Color) -> Self {
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

    fn fill_rect(&mut self, rect: CellRect, fg: Color, bg: Color) {
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

    fn set_cell(
        &mut self,
        col: i32,
        row: i32,
        fg: Color,
        bg: Color,
        text: impl Into<String>,
        underline: bool,
    ) {
        if let Some(cell) = self.cell_mut(col, row) {
            *cell = Cell {
                text: text.into(),
                fg,
                bg,
                underline,
                continuation: false,
            };
        }
    }

    fn put_cell_span(
        &mut self,
        col: i32,
        row: i32,
        span: i32,
        text: &str,
        fg: Color,
        bg: Color,
        underline: bool,
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
                    underline,
                    continuation: offset > 0,
                };
            }
        }
    }

    fn put_text(&mut self, col: i32, row: i32, max_cols: i32, fg: Color, bg: Color, value: &str) {
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
            self.put_cell_span(cursor, row, width, ch.encode_utf8(&mut buf), fg, bg, false);
            cursor += width;
        }
    }

    fn render(&self, frame: &mut Frame<'_>, text: &mut TextRenderer, grid: CellGrid) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[(row * self.cols + col) as usize];
                let x = grid.origin_x + col * grid.cell_w;
                let y = grid.origin_y + row * grid.cell_h;
                frame.rect(x, y, grid.cell_w, grid.cell_h, cell.bg);
            }
        }

        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[(row * self.cols + col) as usize];
                if cell.underline {
                    let x = grid.origin_x + col * grid.cell_w;
                    let y = grid.origin_y + row * grid.cell_h;
                    frame.hline(x, x + grid.cell_w - 1, y + grid.cell_h - 2, cell.fg);
                }
            }
        }

        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[(row * self.cols + col) as usize];
                if cell.continuation || cell.text.trim().is_empty() {
                    continue;
                }
                let x = grid.origin_x + col * grid.cell_w;
                let y = grid.origin_y + row * grid.cell_h;
                frame.text(text, x, y, cell.fg, &cell.text);
            }
        }
    }
}

fn char_cell_width(ch: char) -> i32 {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1) as i32
}

fn render_topbar_frame(
    surface: &mut CellSurface,
    text: &TextRenderer,
    layout: &Layout,
    project: &str,
    status: &str,
    session: &str,
    status_fg: Color,
) {
    draw_box(surface, layout.topbar_cells, SURFACE, BORDER);
    let content = panel_content_rect(layout.topbar_cells, layout.spacing.panel_pad_cells);
    let rows = [(project, MUTED), (status, status_fg), (session, ACCENT)];
    for (offset, (value, fg)) in rows.into_iter().enumerate() {
        if offset as i32 >= content.rows {
            break;
        }
        let value = text.truncate_with_ellipsis(value, content.cols.max(0) as usize);
        let col = content.col + centered_cell_offset(content.cols, &value);
        surface.put_text(
            col,
            content.row + offset as i32,
            content.cols,
            fg,
            SURFACE,
            &value,
        );
    }
}

fn render_sidebar_frame(
    surface: &mut CellSurface,
    text: &TextRenderer,
    layout: &Layout,
    rows: &[SidebarRow],
    scroll: usize,
    sticky_row_index: Option<usize>,
    hovered_row_index: Option<usize>,
    now_ms: u64,
) {
    draw_box(surface, layout.sidebar_cells, SURFACE_ALT, BORDER);
    let mut content = panel_content_rect(layout.sidebar_cells, layout.spacing.panel_pad_cells);
    let visible_rows = content.rows.max(0) as usize;
    let shows_scrollbar = visible_rows > 0 && rows.len() > visible_rows;
    if shows_scrollbar && content.cols > 1 {
        let scrollbar_col = content.col + content.cols - 1;
        render_cell_scrollbar(
            surface,
            scrollbar_col,
            content.row,
            content.rows,
            visible_rows,
            rows.len(),
            scroll,
            SURFACE_ALT,
        );
        content.cols -= 1;
    }

    let sticky_rows = usize::from(sticky_row_index.is_some());
    let sticky_row = sticky_row_index
        .and_then(|row_index| rows.get(row_index).map(|row| (row_index, row)))
        .into_iter();
    let body_rows = rows
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows.saturating_sub(sticky_rows));
    for (screen_row, (row_index, row)) in sticky_row.chain(body_rows).enumerate() {
        if screen_row >= visible_rows || content.cols <= 0 {
            break;
        }
        let row_y = content.row + screen_row as i32;
        let inverted = row.inverted || hovered_row_index == Some(row_index);
        let (row_fg, row_bg) = sidebar_row_colors(row, inverted);
        let bg = row_bg.unwrap_or(SURFACE_ALT);
        surface.fill_rect(
            CellRect {
                col: content.col,
                row: row_y,
                cols: content.cols,
                rows: 1,
            },
            row_fg,
            bg,
        );

        match row.kind {
            SidebarRowKind::Label => {}
            SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => {
                let value = text.truncate_with_ellipsis(&row.text, content.cols as usize);
                let col = content.col + centered_cell_offset(content.cols, &value);
                surface.put_text(col, row_y, content.cols, row_fg, bg, &value);
            }
            SidebarRowKind::Session { .. } => {
                let status = row.status.map(|status| {
                    (
                        sidebar_status_glyph(status, now_ms),
                        sidebar_status_color(status),
                    )
                });
                let reserved_cells = status
                    .map(|(glyph, _)| display_cell_width(glyph) as i32 + 1)
                    .unwrap_or(0);
                let text_cols = content.cols.saturating_sub(reserved_cells);
                let value = text.truncate_with_ellipsis(&row.text, text_cols as usize);
                surface.put_text(content.col, row_y, text_cols, row_fg, bg, &value);

                if let Some((glyph, color)) = status {
                    let glyph_cols = display_cell_width(glyph) as i32;
                    let glyph_col = content.col + content.cols.saturating_sub(glyph_cols);
                    surface.put_text(
                        glyph_col,
                        row_y,
                        glyph_cols,
                        if inverted { row_fg } else { color },
                        bg,
                        glyph,
                    );
                }
            }
        }
    }
}

fn sidebar_row_colors(row: &SidebarRow, inverted: bool) -> (Color, Option<Color>) {
    if inverted {
        return (row.bg.unwrap_or(SURFACE_ALT), Some(row.fg));
    }

    (row.fg, row.bg)
}

fn render_terminal_frame(
    surface: &mut CellSurface,
    layout: &Layout,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
) {
    draw_box(surface, layout.terminal_card_cells, SURFACE, BORDER);
    surface.fill_rect(layout.terminal_cells, TERM_FG, TERM_BG);
    blit_terminal_screen(surface, layout.terminal_cells, screen, selection);
    render_terminal_scrollback(surface, layout, screen);
}

fn blit_terminal_screen(
    surface: &mut CellSurface,
    rect: CellRect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = screen.scrollback() == 0 && !screen.hide_cursor();
    for row in 0..rows.min(rect.rows.max(0) as u16) {
        let row_selection = terminal_selection_span(selection, row, cols);
        for col in 0..cols.min(rect.cols.max(0) as u16) {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }

            let col_span = if col + 1 < cols
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
                let cell_end = col + col_span;
                end > col && start < cell_end
            });
            let cursor_here = cursor_visible && row == cursor_row && col == cursor_col;
            let (fg, bg) = screen_cell_colors(cell, cursor_here, selected, TERM_FG, TERM_BG);
            let contents = if cell.contents().is_empty() {
                " "
            } else {
                cell.contents()
            };
            surface.put_cell_span(
                rect.col + i32::from(col),
                rect.row + i32::from(row),
                i32::from(col_span),
                contents,
                fg,
                bg,
                cell.underline(),
            );
        }
    }
}

fn draw_box(surface: &mut CellSurface, rect: CellRect, bg: Color, border: Color) {
    surface.fill_rect(rect, TEXT, bg);
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

fn render_cell_scrollbar(
    surface: &mut CellSurface,
    col: i32,
    row: i32,
    rows: i32,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    bg: Color,
) {
    if rows <= 0 || visible_items == 0 || total_items <= visible_items {
        return;
    }

    for offset in 0..rows {
        surface.set_cell(col, row + offset, BORDER, bg, SCROLLBAR_TRACK_GLYPH, false);
    }

    let thumb_rows =
        (((rows as i64 * visible_items as i64) / total_items as i64).max(1) as i32).min(rows);
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_row = row
        + (((rows - thumb_rows).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    for offset in 0..thumb_rows {
        surface.set_cell(
            col,
            thumb_row + offset,
            MUTED,
            bg,
            SCROLLBAR_THUMB_GLYPH,
            false,
        );
    }
}

fn terminal_max_scrollback(screen: &vt100::Screen) -> usize {
    let mut snapshot = screen.clone();
    snapshot.set_scrollback(usize::MAX);
    snapshot.scrollback()
}

fn render_terminal_scrollback(surface: &mut CellSurface, layout: &Layout, screen: &vt100::Screen) {
    let visible_rows = usize::from(screen.size().0);
    let max_scroll = terminal_max_scrollback(screen);
    if visible_rows == 0 || max_scroll == 0 || layout.terminal_cells.rows <= 0 {
        return;
    }

    let right_pad_col = layout.terminal_cells.col
        + layout.terminal_cells.cols
        + layout.spacing.terminal_pad_cols.saturating_sub(1);
    let max_col = layout.terminal_card_cells.col + layout.terminal_card_cells.cols - 2;
    let col = right_pad_col.min(max_col).max(layout.terminal_cells.col);
    render_cell_scrollbar(
        surface,
        col,
        layout.terminal_cells.row,
        layout.terminal_cells.rows,
        visible_rows,
        visible_rows.saturating_add(max_scroll),
        max_scroll.saturating_sub(screen.scrollback()),
        SURFACE,
    );
}

fn display_cell_width(value: &str) -> usize {
    value
        .chars()
        .map(|ch| {
            unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1)
        })
        .sum()
}

fn centered_cell_offset(cols: i32, value: &str) -> i32 {
    let cols = cols.max(0) as usize;
    (cols.saturating_sub(display_cell_width(value)) / 2) as i32
}

#[cfg(test)]
mod tests {
    use super::status_text_for_session;
    use crate::state::Session;

    #[test]
    fn status_text_ignores_tool_name_when_runtime_status_exists() {
        let mut session = Session::new_draft();
        session.runtime.status = Some("thinking".into());
        session.runtime.tool_name = Some("Clipboard".into());

        assert_eq!(
            status_text_for_session(true, Some(&session), None),
            "thinking"
        );
    }

    #[test]
    fn status_text_ignores_stale_tool_name_without_runtime_status() {
        let mut session = Session::new_draft();
        session.draft = false;
        session.runtime.tool_name = Some("Clipboard".into());

        assert_eq!(status_text_for_session(true, Some(&session), None), "idle");
    }
}
