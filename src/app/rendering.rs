use std::{fmt::Write as _, num::NonZeroU32};

use crate::render::{Color, Frame, TextRenderer};
use crate::state::Session;
use crate::terminal::{
    terminal_selection_span, TerminalController, TerminalSelectionRange, TerminalStatus,
};
use crate::util::now_millis;

use super::layout::{self, Layout, Rect};
use super::sidebar::{sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind};
use super::theme::{
    screen_cell_colors, status_color, theme_palette_index, ACCENT, BG, BORDER, MUTED, SURFACE,
    SURFACE_ALT, TERM_BG, TERM_FG, TEXT,
};
use super::App;

struct AppRenderModel {
    topbar_project: String,
    topbar_session: String,
    topbar_status: String,
    topbar_status_fg: Color,
    sidebar_rows: Vec<SidebarRow>,
    sticky_sidebar_anchor: Option<usize>,
    hovered_sidebar_row: Option<usize>,
    sidebar_status_now_ms: u64,
    terminal_selection: Option<TerminalSelectionRange>,
    screen: vt100::Screen,
}

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
                self.sidebar_visible_rows(layout.sidebar, text, layout.spacing.panel_pad);
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
                layout.spacing.panel_pad,
            )
        });
        let model = self.collect_render_model(
            &layout,
            sidebar_rows,
            sticky_sidebar_anchor,
            hovered_sidebar_row,
        );

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
        paint_render_model(&mut frame, text, &layout, self.sidebar_scroll, &model);

        let _ = buffer.present();
        self.needs_redraw = false;
    }

    fn collect_render_model(
        &self,
        layout: &Layout,
        sidebar_rows: Vec<SidebarRow>,
        sticky_sidebar_anchor: Option<usize>,
        hovered_sidebar_row: Option<usize>,
    ) -> AppRenderModel {
        AppRenderModel {
            topbar_project: self
                .current_project()
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "pi-harness".to_string()),
            topbar_session: self
                .current_session()
                .map(|session| session.name.clone())
                .unwrap_or_default(),
            topbar_status: self.status_text(),
            topbar_status_fg: status_color(self.current_session(), self.current_terminal_status()),
            sidebar_rows,
            sticky_sidebar_anchor,
            hovered_sidebar_row,
            sidebar_status_now_ms: now_millis(),
            terminal_selection: self
                .current_terminal()
                .and_then(TerminalController::selection_range),
            screen: self
                .current_terminal()
                .map(|terminal| terminal.screen().clone())
                .unwrap_or_else(|| {
                    vt100::Parser::new(layout.terminal_rows, layout.terminal_cols, 0)
                        .screen()
                        .clone()
                }),
        }
    }

    fn status_text(&self) -> String {
        status_text_for_session(
            self.current_project().is_some(),
            self.current_session(),
            self.current_terminal_status(),
        )
    }
}

fn paint_render_model(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    layout: &Layout,
    sidebar_scroll: usize,
    model: &AppRenderModel,
) {
    frame.clear(BG);
    render_app_chrome(frame, layout);

    render_topbar_frame(
        frame,
        text,
        layout.topbar,
        layout.spacing.panel_pad,
        TopbarFrame {
            project: &model.topbar_project,
            status: &model.topbar_status,
            session: &model.topbar_session,
            status_fg: model.topbar_status_fg,
        },
    );
    render_sidebar_frame(
        frame,
        text,
        layout.sidebar,
        layout.spacing.panel_pad,
        SidebarFrame {
            rows: &model.sidebar_rows,
            scroll: sidebar_scroll,
            sticky_row_index: model.sticky_sidebar_anchor,
            hovered_row_index: model.hovered_sidebar_row,
            now_ms: model.sidebar_status_now_ms,
        },
    );
    render_terminal_frame(
        frame,
        text,
        layout.terminal,
        &model.screen,
        model.terminal_selection,
    );
    render_terminal_scrollback(
        frame,
        layout.terminal,
        layout.spacing.terminal_pad,
        &model.screen,
        (text.metrics.cell_height / 2).max(4),
    );
}

fn render_app_chrome(frame: &mut Frame<'_>, layout: &Layout) {
    render_panel_chrome(frame, layout.topbar, SURFACE);
    render_panel_chrome(frame, layout.sidebar, SURFACE_ALT);
    render_panel_chrome(frame, layout.terminal_card, SURFACE);
    frame.rect(
        layout.terminal.x,
        layout.terminal.y,
        layout.terminal.w,
        layout.terminal.h,
        TERM_BG,
    );
}

fn render_panel_chrome(frame: &mut Frame<'_>, rect: Rect, bg: Color) {
    frame.rect(rect.x, rect.y, rect.w, rect.h, bg);
    frame.stroke_rect(rect.x, rect.y, rect.w, rect.h, BORDER);
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

#[derive(Clone, Copy)]
struct ScreenRenderStyle {
    default_fg: Color,
    default_bg: Color,
    cursor_visible: bool,
}

#[derive(Clone, Copy)]
struct TopbarFrame<'a> {
    project: &'a str,
    status: &'a str,
    session: &'a str,
    status_fg: Color,
}

#[derive(Clone, Copy)]
struct SidebarFrame<'a> {
    rows: &'a [SidebarRow],
    scroll: usize,
    sticky_row_index: Option<usize>,
    hovered_row_index: Option<usize>,
    now_ms: u64,
}

fn render_topbar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    panel_pad: i32,
    model: TopbarFrame<'_>,
) {
    let content_rect = Rect {
        x: rect.x + panel_pad,
        y: rect.y + panel_pad,
        w: (rect.w - panel_pad * 2).max(0),
        h: layout::TOPBAR_ROWS * text.metrics.cell_height,
    };
    let cols = screen_cols(content_rect, text);
    let rows = layout::TOPBAR_ROWS.max(0) as usize;
    let Some(screen) = build_synthetic_screen(rows, cols, |ansi| {
        let project_text = text.truncate_with_ellipsis(model.project, cols);
        let status_text = text.truncate_with_ellipsis(model.status, cols);
        let session_text = text.truncate_with_ellipsis(model.session, cols);
        push_positioned_text(
            ansi,
            1,
            centered_screen_col(cols, &project_text),
            MUTED,
            None,
            &project_text,
        );
        push_positioned_text(
            ansi,
            2,
            centered_screen_col(cols, &status_text),
            model.status_fg,
            None,
            &status_text,
        );
        push_positioned_text(
            ansi,
            3,
            centered_screen_col(cols, &session_text),
            ACCENT,
            None,
            &session_text,
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
        ScreenRenderStyle {
            default_fg: TEXT,
            default_bg: SURFACE,
            cursor_visible: false,
        },
    );
}

fn render_sidebar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    panel_pad: i32,
    model: SidebarFrame<'_>,
) {
    let cell_h = text.metrics.cell_height.max(1);
    let start_y = rect.y + panel_pad;
    let visible_rows = ((rect.h - panel_pad * 2).max(0) / cell_h) as usize;
    let shows_scrollbar = visible_rows > 0 && model.rows.len() > visible_rows;
    let scrollbar_reserve_px = if shows_scrollbar { 8 } else { 0 };
    let content_rect = Rect {
        x: rect.x + panel_pad,
        y: start_y,
        w: (rect.w - panel_pad * 2 - scrollbar_reserve_px).max(0),
        h: (visible_rows as i32 * cell_h).max(0),
    };
    let cols = screen_cols(content_rect, text);

    if let Some(screen) = build_synthetic_screen(visible_rows, cols, |ansi| {
        let sticky_rows = usize::from(model.sticky_row_index.is_some());
        let sticky_row = model
            .sticky_row_index
            .and_then(|row_index| model.rows.get(row_index).map(|row| (row_index, row)))
            .into_iter();
        let body_rows = model
            .rows
            .iter()
            .enumerate()
            .skip(model.scroll)
            .take(visible_rows.saturating_sub(sticky_rows));
        for (screen_row, (row_index, row)) in sticky_row.chain(body_rows).enumerate() {
            let screen_row = screen_row + 1;
            let inverted = row.inverted || model.hovered_row_index == Some(row_index);
            let (row_fg, row_bg) = sidebar_row_colors(row, inverted);
            if let Some(bg) = row_bg {
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
                        row_fg,
                        row_bg,
                        &value,
                    );
                }
                SidebarRowKind::Session { .. } => {
                    let status = row.status.map(|status| {
                        (
                            sidebar_status_glyph(status, model.now_ms),
                            sidebar_status_color(status),
                        )
                    });
                    let reserved_cells = status
                        .map(|(glyph, _)| display_cell_width(glyph) + 1)
                        .unwrap_or(0);
                    let value =
                        text.truncate_with_ellipsis(&row.text, cols.saturating_sub(reserved_cells));
                    push_positioned_text(ansi, screen_row, 1, row_fg, row_bg, &value);

                    if let Some((glyph, color)) = status {
                        push_positioned_text(
                            ansi,
                            screen_row,
                            cols.saturating_sub(display_cell_width(glyph)) + 1,
                            if inverted { row_fg } else { color },
                            row_bg,
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
            ScreenRenderStyle {
                default_fg: TEXT,
                default_bg: SURFACE_ALT,
                cursor_visible: false,
            },
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
        model.rows.len(),
        model.scroll,
        (cell_h / 2).max(4),
    );
}

fn sidebar_row_colors(row: &SidebarRow, inverted: bool) -> (Color, Option<Color>) {
    if inverted {
        return (row.bg.unwrap_or(SURFACE_ALT), Some(row.fg));
    }

    (row.fg, row.bg)
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

fn screen_cols(rect: Rect, text: &TextRenderer) -> usize {
    (rect.w.max(0) / text.metrics.cell_width.max(1)) as usize
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
    if let Some(idx) = theme_palette_index(color) {
        let _ = write!(out, "\x1b[38;5;{idx}m");
        return;
    }

    let (r, g, b) = color.rgb_components();
    let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
}

fn push_ansi_bg(out: &mut String, color: Color) {
    if let Some(idx) = theme_palette_index(color) {
        let _ = write!(out, "\x1b[48;5;{idx}m");
        return;
    }

    let (r, g, b) = color.rgb_components();
    let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
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

    let thumb_h = (((track.h as i64 * visible_items as i64) / total_items as i64)
        .max(i64::from(min_thumb_h.max(1))) as i32)
        .min(track.h);
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_y = track.y
        + (((track.h - thumb_h).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    frame.rect(track.x, thumb_y, track.w.max(1), thumb_h, MUTED);
}

fn terminal_max_scrollback(screen: &vt100::Screen) -> usize {
    let mut snapshot = screen.clone();
    snapshot.set_scrollback(usize::MAX);
    snapshot.scrollback()
}

fn render_terminal_scrollback(
    frame: &mut Frame<'_>,
    rect: Rect,
    terminal_pad: i32,
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
            x: rect.x + rect.w + terminal_pad - 5,
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
        ScreenRenderStyle {
            default_fg: TERM_FG,
            default_bg: TERM_BG,
            cursor_visible,
        },
    );
}

fn render_screen_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
    style: ScreenRenderStyle,
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
            let cursor_here = style.cursor_visible && row == cursor_row && col == cursor_col;
            let (fg, bg) = screen_cell_colors(
                cell,
                cursor_here,
                selected,
                style.default_fg,
                style.default_bg,
            );
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
