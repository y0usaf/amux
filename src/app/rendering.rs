use std::num::NonZeroU32;

use crate::render::{Color, Frame, TextRenderer};
use crate::terminal::{terminal_selection_span, TerminalSelectionRange, TerminalStatus};
use crate::util::now_millis;

use super::layout::{self, Rect, TERMINAL_PAD};
use super::sidebar::{sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind};
use super::theme::{
    status_color, terminal_cell_colors, BG, BORDER, MUTED, SURFACE, SURFACE_ALT, TERM_BG, TEXT,
    WARNING,
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
    let max_cells = ((rect.w - layout::PANEL_PAD * 2).max(0) / text.metrics.cell_width) as usize;
    let title_text = text.truncate_with_ellipsis(title, max_cells.max(8));
    let status_text = text.truncate_with_ellipsis(status, max_cells.max(8));
    let title_x = centered_text_x(rect, text, &title_text);
    let status_x = centered_text_x(rect, text, &status_text);
    let y = rect.y + layout::PANEL_PAD;

    frame.text(text, title_x, y, TEXT, &title_text);
    frame.text(
        text,
        status_x,
        y + text.metrics.cell_height,
        status_fg,
        &status_text,
    );
}

fn render_sidebar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    rows: &[SidebarRow],
    scroll: usize,
    now_ms: u64,
) {
    let cell_w = text.metrics.cell_width;
    let cell_h = text.metrics.cell_height;
    let start_x = rect.x + layout::SIDEBAR_PAD_X * cell_w;
    let start_y = rect.y + layout::SIDEBAR_PAD_Y * cell_h;
    let visible_rows = ((rect.h - layout::SIDEBAR_PAD_Y * 2 * cell_h).max(0) / cell_h) as usize;
    let shows_scrollbar = visible_rows > 0 && rows.len() > visible_rows;
    let scrollbar_reserve_px = if shows_scrollbar { 8 } else { 0 };

    for (index, row) in rows.iter().skip(scroll).take(visible_rows).enumerate() {
        let y = start_y + index as i32 * cell_h;
        if let Some(bg) = row.bg {
            frame.rect(rect.x + 6, y, rect.w - 12, cell_h, bg);
        }

        let status = row.status.map(|status| {
            (
                sidebar_status_glyph(status, now_ms),
                sidebar_status_color(status),
            )
        });
        let reserved_px = if status.is_some() { cell_w * 2 } else { 0 };
        let line = text.truncate_with_ellipsis(
            &row.text,
            (((rect.w - layout::SIDEBAR_PAD_X * 2 * cell_w - scrollbar_reserve_px - reserved_px)
                .max(0)
                / cell_w) as usize)
                .max(match row.kind {
                    SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => 8,
                    SidebarRowKind::Label | SidebarRowKind::Session { .. } => 0,
                }),
        );
        let x = match row.kind {
            SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => {
                centered_text_x(rect, text, &line)
            }
            SidebarRowKind::Label | SidebarRowKind::Session { .. } => start_x,
        };
        frame.text(text, x, y, row.fg, &line);

        if let Some((glyph, color)) = status {
            let glyph_x = rect.x + rect.w
                - layout::SIDEBAR_PAD_X * cell_w
                - scrollbar_reserve_px
                - text.measure_text(glyph);
            frame.text(text, glyph_x, y, color, glyph);
        }
    }

    render_vertical_scrollbar(
        frame,
        rect.x + rect.w - 5,
        start_y,
        (visible_rows as i32 * cell_h).max(0),
        visible_rows,
        rows.len(),
        scroll,
        (cell_h / 2).max(4),
    );
}

fn render_vertical_scrollbar(
    frame: &mut Frame<'_>,
    track_x: i32,
    track_y: i32,
    track_h: i32,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    min_thumb_h: i32,
) {
    if track_h <= 0 || visible_items == 0 || total_items <= visible_items {
        return;
    }

    frame.rect(track_x, track_y, 1, track_h, BORDER);

    let thumb_h = ((track_h as i64 * visible_items as i64) / total_items as i64)
        .max(i64::from(min_thumb_h.max(1))) as i32;
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_y = track_y
        + (((track_h - thumb_h).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    frame.rect(track_x, thumb_y, 1, thumb_h.min(track_h), MUTED);
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
        rect.x + rect.w + TERMINAL_PAD - 5,
        rect.y,
        rect.h,
        visible_rows,
        visible_rows.saturating_add(max_scroll),
        max_scroll.saturating_sub(screen.scrollback()),
        min_thumb_h,
    );
}

fn centered_text_x(rect: Rect, text: &TextRenderer, value: &str) -> i32 {
    rect.x + ((rect.w - text.measure_text(value)).max(0) / 2)
}

fn render_terminal_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = screen.scrollback() == 0 && !screen.hide_cursor();

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
            let (fg, bg) = terminal_cell_colors(cell, cursor_here, selected);
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
