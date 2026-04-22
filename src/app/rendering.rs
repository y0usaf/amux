use std::{fmt::Write as _, num::NonZeroU32};

use crate::render::{Color, Frame, TextRenderer};
use crate::terminal::{terminal_selection_span, TerminalSelectionRange, TerminalStatus};
use crate::util::now_millis;

use super::layout::{self, Rect, TERMINAL_PAD};
use super::sidebar::{sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind};
use super::theme::{
    screen_cell_colors, status_color, BG, BORDER, MUTED, SURFACE, SURFACE_ALT, TERM_BG, TERM_FG,
    TEXT, WARNING,
};
use super::App;

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
            let sidebar_visible_rows = self.sidebar_visible_rows(layout.sidebar, text);
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
        let sticky_sidebar_anchor = self
            .sticky_sidebar_anchor_row_index(&sidebar_rows, sidebar_visible_rows)
            .and_then(|row_index| sidebar_rows.get(row_index));

        let topbar_title = match (self.current_project(), self.current_session()) {
            (Some(project), Some(session)) => format!("{} / {}", project.name, session.name),
            (Some(project), None) => project.name.clone(),
            (None, _) => "pi-harness".to_string(),
        };
        let status = self.status_text();
        let topbar_status = self.note.clone().unwrap_or(status);
        let topbar_status_fg = if self.note.is_some() {
            WARNING
        } else {
            status_color(self.current_session(), self.current_terminal_status())
        };
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

        frame.rect(
            layout.topbar.x,
            layout.topbar.y,
            layout.topbar.w,
            layout.topbar.h,
            SURFACE,
        );
        frame.stroke_rect(
            layout.topbar.x,
            layout.topbar.y,
            layout.topbar.w,
            layout.topbar.h,
            BORDER,
        );

        frame.rect(
            layout.sidebar.x,
            layout.sidebar.y,
            layout.sidebar.w,
            layout.sidebar.h,
            SURFACE_ALT,
        );
        frame.stroke_rect(
            layout.sidebar.x,
            layout.sidebar.y,
            layout.sidebar.w,
            layout.sidebar.h,
            BORDER,
        );

        frame.rect(
            layout.terminal_card.x,
            layout.terminal_card.y,
            layout.terminal_card.w,
            layout.terminal_card.h,
            SURFACE,
        );
        frame.stroke_rect(
            layout.terminal_card.x,
            layout.terminal_card.y,
            layout.terminal_card.w,
            layout.terminal_card.h,
            BORDER,
        );
        frame.rect(
            layout.terminal.x,
            layout.terminal.y,
            layout.terminal.w,
            layout.terminal.h,
            TERM_BG,
        );

        let sidebar_status_now_ms = now_millis();

        render_topbar_frame(
            &mut frame,
            text,
            layout.topbar,
            &topbar_title,
            &topbar_status,
            topbar_status_fg,
        );
        render_sidebar_frame(
            &mut frame,
            text,
            layout.sidebar,
            &sidebar_rows,
            self.sidebar_scroll,
            sticky_sidebar_anchor,
            sidebar_status_now_ms,
        );
        render_terminal_frame(
            &mut frame,
            text,
            layout.terminal,
            &screen,
            terminal_selection,
        );
        render_terminal_scrollback(
            &mut frame,
            layout.terminal,
            &screen,
            (text.metrics.cell_height / 2).max(4),
        );

        let _ = buffer.present();
        self.needs_redraw = false;
    }

    fn status_text(&self) -> String {
        if let Some(session) = self.current_session() {
            if let Some(tool) = session.runtime.tool_name.as_deref() {
                return format!("tool: {}", tool);
            }
            if let Some(status) = session.runtime.status.as_deref() {
                if session.runtime.queued {
                    return format!("{} · queued", status);
                }
                return status.to_string();
            }
            if session.draft {
                return "new session".to_string();
            }
        } else if self.current_project().is_some() {
            return "select a session".to_string();
        } else {
            return "open a project".to_string();
        }

        match self.current_terminal_status() {
            Some(TerminalStatus::Launching) => "launching".to_string(),
            Some(TerminalStatus::Running) => "running".to_string(),
            Some(TerminalStatus::Exited(_)) => "exited".to_string(),
            Some(TerminalStatus::Error(_)) => "error".to_string(),
            Some(TerminalStatus::Empty) | None => "idle".to_string(),
        }
    }
}

use crate::terminal::TerminalController;

fn render_topbar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    title: &str,
    status: &str,
    status_fg: Color,
) {
    let content_rect = Rect {
        x: rect.x + layout::PANEL_PAD,
        y: rect.y + layout::PANEL_PAD,
        w: (rect.w - layout::PANEL_PAD * 2).max(0),
        h: layout::TOPBAR_ROWS * text.metrics.cell_height,
    };
    let cols = screen_cols(content_rect, text);
    let rows = layout::TOPBAR_ROWS.max(0) as usize;
    let Some(screen) = build_synthetic_screen(rows, cols, |ansi| {
        let title_text = text.truncate_with_ellipsis(title, cols);
        let status_text = text.truncate_with_ellipsis(status, cols);
        push_positioned_text(
            ansi,
            1,
            centered_screen_col(cols, &title_text),
            TEXT,
            None,
            &title_text,
        );
        push_positioned_text(
            ansi,
            2,
            centered_screen_col(cols, &status_text),
            status_fg,
            None,
            &status_text,
        );
    }) else {
        return;
    };

    render_screen_frame(
        frame,
        text,
        content_rect,
        &screen,
        None,
        TEXT,
        SURFACE,
        false,
    );
}

fn render_sidebar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    rows: &[SidebarRow],
    scroll: usize,
    sticky_row: Option<&SidebarRow>,
    now_ms: u64,
) {
    let cell_w = text.metrics.cell_width.max(1);
    let cell_h = text.metrics.cell_height.max(1);
    let start_y = rect.y + layout::SIDEBAR_PAD_Y * cell_h;
    let visible_rows = ((rect.h - layout::SIDEBAR_PAD_Y * 2 * cell_h).max(0) / cell_h) as usize;
    let shows_scrollbar = visible_rows > 0 && rows.len() > visible_rows;
    let scrollbar_reserve_px = if shows_scrollbar { 8 } else { 0 };
    let content_rect = Rect {
        x: rect.x + layout::SIDEBAR_PAD_X * cell_w,
        y: start_y,
        w: (rect.w - layout::SIDEBAR_PAD_X * 2 * cell_w - scrollbar_reserve_px).max(0),
        h: (visible_rows as i32 * cell_h).max(0),
    };
    let cols = screen_cols(content_rect, text);

    if let Some(screen) = build_synthetic_screen(visible_rows, cols, |ansi| {
        for (screen_row, row) in visible_sidebar_rows(rows, scroll, sticky_row, visible_rows)
            .into_iter()
            .enumerate()
        {
            let screen_row = screen_row + 1;
            if let Some(bg) = row.bg {
                fill_screen_row(ansi, screen_row, cols, bg);
            }

            match row.kind {
                SidebarRowKind::Label => {}
                SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => {
                    let value = text.truncate_with_ellipsis(&row.text, cols);
                    push_positioned_text(
                        ansi,
                        screen_row,
                        centered_screen_col(cols, &value),
                        row.fg,
                        row.bg,
                        &value,
                    );
                }
                SidebarRowKind::Session { .. } => {
                    let status = row.status.map(|status| {
                        (
                            sidebar_status_glyph(status, now_ms),
                            sidebar_status_color(status),
                        )
                    });
                    let reserved_cells = status
                        .map(|(glyph, _)| display_cell_width(glyph) + 1)
                        .unwrap_or(0);
                    let value =
                        text.truncate_with_ellipsis(&row.text, cols.saturating_sub(reserved_cells));
                    push_positioned_text(ansi, screen_row, 1, row.fg, row.bg, &value);

                    if let Some((glyph, color)) = status {
                        push_positioned_text(
                            ansi,
                            screen_row,
                            cols.saturating_sub(display_cell_width(glyph)) + 1,
                            color,
                            row.bg,
                            glyph,
                        );
                    }
                }
            }
        }
    }) {
        render_screen_frame(
            frame,
            text,
            content_rect,
            &screen,
            None,
            TEXT,
            SURFACE_ALT,
            false,
        );
    }

    render_vertical_scrollbar(
        frame,
        Rect {
            x: rect.x + rect.w - 5,
            y: start_y,
            w: 1,
            h: (visible_rows as i32 * cell_h).max(0),
        },
        visible_rows,
        rows.len(),
        scroll,
        (cell_h / 2).max(4),
    );
}

fn build_synthetic_screen(
    rows: usize,
    cols: usize,
    build: impl FnOnce(&mut String),
) -> Option<vt100::Screen> {
    if rows == 0 || cols == 0 {
        return None;
    }

    let mut parser = vt100::Parser::new(rows as u16, cols as u16, 0);
    let mut input = String::from("\x1b[?25l");
    build(&mut input);
    parser.process(input.as_bytes());
    Some(parser.screen().clone())
}

fn visible_sidebar_rows<'a>(
    rows: &'a [SidebarRow],
    scroll: usize,
    sticky_row: Option<&'a SidebarRow>,
    visible_rows: usize,
) -> Vec<&'a SidebarRow> {
    let sticky_rows = usize::from(sticky_row.is_some());
    let mut visible = Vec::with_capacity(visible_rows);
    if let Some(row) = sticky_row {
        visible.push(row);
    }
    visible.extend(
        rows.iter()
            .skip(scroll)
            .take(visible_rows.saturating_sub(sticky_rows)),
    );
    visible
}

fn screen_cols(rect: Rect, text: &TextRenderer) -> usize {
    ((rect.w.max(0) / text.metrics.cell_width.max(1)).max(0)) as usize
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

fn centered_screen_col(cols: usize, value: &str) -> usize {
    cols.saturating_sub(display_cell_width(value)) / 2 + 1
}

fn fill_screen_row(out: &mut String, row: usize, cols: usize, bg: Color) {
    if cols == 0 {
        return;
    }
    push_cursor_move(out, row, 1);
    push_ansi_bg(out, bg);
    out.push_str(&" ".repeat(cols));
    out.push_str("\x1b[0m");
}

fn push_positioned_text(
    out: &mut String,
    row: usize,
    col: usize,
    fg: Color,
    bg: Option<Color>,
    value: &str,
) {
    if value.is_empty() || col == 0 {
        return;
    }

    push_cursor_move(out, row, col);
    if let Some(bg) = bg {
        push_ansi_bg(out, bg);
    }
    push_ansi_fg(out, fg);
    out.push_str(value);
    out.push_str("\x1b[0m");
}

fn push_cursor_move(out: &mut String, row: usize, col: usize) {
    let _ = write!(out, "\x1b[{row};{col}H");
}

fn push_ansi_fg(out: &mut String, color: Color) {
    let (r, g, b) = color_rgb(color);
    let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
}

fn push_ansi_bg(out: &mut String, color: Color) {
    let (r, g, b) = color_rgb(color);
    let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
}

fn color_rgb(color: Color) -> (u8, u8, u8) {
    let value = color.argb();
    (
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn render_vertical_scrollbar(
    frame: &mut Frame<'_>,
    track: Rect,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    min_thumb_h: i32,
) {
    if track.h <= 0 || visible_items == 0 || total_items <= visible_items {
        return;
    }

    frame.rect(track.x, track.y, track.w.max(1), track.h, BORDER);

    let thumb_h = ((track.h as i64 * visible_items as i64) / total_items as i64)
        .max(i64::from(min_thumb_h.max(1))) as i32;
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_y = track.y
        + (((track.h - thumb_h).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    frame.rect(
        track.x,
        thumb_y,
        track.w.max(1),
        thumb_h.min(track.h),
        MUTED,
    );
}

fn terminal_max_scrollback(screen: &vt100::Screen) -> usize {
    let mut snapshot = screen.clone();
    snapshot.set_scrollback(usize::MAX);
    snapshot.scrollback()
}

fn render_terminal_scrollback(
    frame: &mut Frame<'_>,
    rect: Rect,
    screen: &vt100::Screen,
    min_thumb_h: i32,
) {
    let visible_rows = usize::from(screen.size().0);
    let max_scroll = terminal_max_scrollback(screen);
    if visible_rows == 0 || max_scroll == 0 {
        return;
    }

    render_vertical_scrollbar(
        frame,
        Rect {
            x: rect.x + rect.w + TERMINAL_PAD - 5,
            y: rect.y,
            w: 1,
            h: rect.h,
        },
        visible_rows,
        visible_rows.saturating_add(max_scroll),
        max_scroll.saturating_sub(screen.scrollback()),
        min_thumb_h,
    );
}

fn render_terminal_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
) {
    let cursor_visible = screen.scrollback() == 0 && !screen.hide_cursor();
    render_screen_frame(
        frame,
        text,
        rect,
        screen,
        selection,
        TERM_FG,
        TERM_BG,
        cursor_visible,
    );
}

fn render_screen_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
    default_fg: Color,
    default_bg: Color,
    cursor_visible: bool,
) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();

    for row in 0..rows {
        let y = rect.y + i32::from(row) * text.metrics.cell_height;
        if y >= rect.y + rect.h {
            break;
        }
        let row_selection = terminal_selection_span(selection, row, cols);
        for col in 0..cols {
            let x = rect.x + i32::from(col) * text.metrics.cell_width;
            if x >= rect.x + rect.w {
                break;
            }
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
            let (fg, bg) = screen_cell_colors(cell, cursor_here, selected, default_fg, default_bg);
            frame.rect(
                x,
                y,
                text.metrics.cell_width * i32::from(col_span),
                text.metrics.cell_height,
                bg,
            );
            if cell.underline() {
                frame.hline(
                    x,
                    x + text.metrics.cell_width * i32::from(col_span) - 1,
                    y + text.metrics.cell_height - 2,
                    fg,
                );
            }
            let contents = if cell.contents().is_empty() {
                " "
            } else {
                cell.contents()
            };
            frame.text(text, x, y, fg, contents);
        }
    }
}
