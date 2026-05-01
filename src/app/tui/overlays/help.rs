use crate::app::cell_surface::{
    display_cell_width, draw_box, render_cell_scrollbar, truncate_to_cells, CellSurface,
};
use crate::app::layout::CellRect as Rect;
use crate::app::theme::{self, DerivedTheme};
use crate::config::{AppAction, AppConfig};

use super::super::raw::terminal_size;

#[derive(Clone, Debug, Default)]
pub(in crate::app::tui) struct HelpOverlayState {
    pub(in crate::app::tui) scroll: usize,
}

fn help_overlay_rect(cols: i32, rows: i32) -> Rect {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let width = cols.clamp(1, 88);
    let height = rows.clamp(1, 20);
    Rect::new((cols - width) / 2, (rows - height) / 2, width, height)
}

pub(in crate::app::tui) fn help_overlay_visible_rows_for_terminal() -> usize {
    let (cols, rows) = terminal_size();
    let inner = help_overlay_rect(i32::from(cols), i32::from(rows)).inset_edges(2, 1, 2, 1);
    inner.rows.saturating_sub(2).max(0) as usize
}

pub(in crate::app::tui) fn help_overlay_lines(config: &crate::config::AppConfig) -> Vec<String> {
    let mut lines = vec![
        "Global".to_string(),
        help_line("ctrl+g / :help", "toggle this help"),
        help_line(":open <dir>", "open project"),
        help_line(":archive", "browse archived sessions"),
        help_line(":refresh / :reload", "refresh current / all sessions"),
        help_line(":quit / ctrl+q", "quit"),
        String::new(),
        "Navigation & actions".to_string(),
    ];

    for action in [
        AppAction::PreviousProject,
        AppAction::NextProject,
        AppAction::PreviousSession,
        AppAction::NextSession,
        AppAction::NewSession,
        AppAction::RefreshSession,
        AppAction::RefreshAllSessions,
        AppAction::ArchiveSession,
        AppAction::RemoveProject,
        AppAction::CopySelection,
        AppAction::PasteClipboard,
    ] {
        let bindings = config.action_binding_texts(action).join(" / ");
        if !bindings.is_empty() {
            lines.push(help_line(&bindings, action_help_text(action)));
        }
    }

    lines.extend([
        String::new(),
        "Terminal".to_string(),
        help_line("mouse drag", "select text"),
        help_line("wheel / scroll keys", "scroll terminal"),
        help_line("::", "send literal ':' to terminal"),
        String::new(),
        "Command line".to_string(),
        help_line("ctrl+a/e", "start / end"),
        help_line("ctrl+w/u", "delete word / clear"),
        help_line("backspace", "delete; empty input closes"),
        String::new(),
        "Help / archive dialogs".to_string(),
        help_line("↑/↓ j/k wheel", "scroll / select"),
        help_line("pgup/pgdn", "page up / down"),
        help_line("home/end g/G", "jump top / bottom"),
        help_line("esc q ctrl+c", "close"),
    ]);
    lines
}

fn action_help_text(action: AppAction) -> &'static str {
    match action {
        AppAction::PreviousProject => "previous project",
        AppAction::NextProject => "next project",
        AppAction::PreviousSession => "previous session",
        AppAction::NextSession => "next session",
        AppAction::NewSession => "new session",
        AppAction::RefreshSession => "refresh session",
        AppAction::RefreshAllSessions => "refresh all sessions",
        AppAction::ArchiveSession => "archive selected session",
        AppAction::RemoveProject => "remove project from sidebar",
        AppAction::CopySelection => "copy terminal selection",
        AppAction::PasteClipboard => "paste clipboard / image",
    }
}

fn help_line(keys: &str, description: &str) -> String {
    format!("{keys:<24} {description}")
}

fn render_dialog_title_line(
    surface: &mut CellSurface,
    row: i32,
    col: i32,
    width: i32,
    title: &str,
    info: &str,
    theme: &DerivedTheme,
) {
    if width <= 0 {
        return;
    }
    surface.put_text(col, row, width, theme.text, theme.surface, title);
    let title_width = display_cell_width(title) as i32;
    let info_width = display_cell_width(info) as i32;
    let slash_start = col + title_width + 1;
    let slash_end = col + width - info_width.saturating_add(1);
    for index in 0..(slash_end - slash_start).max(0) {
        let fg = if index % 2 == 0 {
            theme.accent
        } else {
            theme.accent_2
        };
        surface.set_cell(slash_start + index, row, fg, theme.surface, "╱", false);
    }
    if info_width > 0 && width > info_width {
        surface.put_text(
            col + width - info_width,
            row,
            info_width,
            theme.accent_2,
            theme.surface,
            info,
        );
    }
}

pub(in crate::app::tui) fn render_help_overlay(
    surface: &mut CellSurface,
    help: &mut HelpOverlayState,
    config: &AppConfig,
    theme: &DerivedTheme,
) {
    let rect = help_overlay_rect(surface.cols, surface.rows);
    draw_box(
        surface,
        rect,
        theme.text,
        theme.surface_raised,
        theme.accent,
    );
    if rect.cols <= 2 || rect.rows <= 2 {
        return;
    }

    let inner = rect.inset_edges(2, 1, 2, 1);
    surface.fill_rect(inner, theme.text, theme.surface);
    render_dialog_title_line(
        surface,
        inner.row,
        inner.col,
        inner.cols,
        " HELP ",
        " ctrl+g/Esc/q close ",
        theme,
    );

    let lines = help_overlay_lines(config);
    let list_row = inner.row + 2;
    let list_rows = (inner.row + inner.rows - list_row).max(0) as usize;
    if list_rows == 0 {
        return;
    }
    help.scroll = help.scroll.min(lines.len().saturating_sub(list_rows));
    let text_width = (inner.cols - 1).max(0);
    for (row_offset, line) in lines.iter().skip(help.scroll).take(list_rows).enumerate() {
        let row = list_row + row_offset as i32;
        if line.is_empty() {
            continue;
        }
        let is_heading = !line.starts_with(' ')
            && !line.contains("  ")
            && !line.contains('/')
            && !line.contains(':');
        if is_heading {
            render_dialog_title_line(surface, row, inner.col, text_width, line, "", theme);
        } else {
            let split_at = line.len().min(24);
            let (keys, description) = line.split_at(split_at);
            surface.put_text_styled(
                inner.col,
                row,
                split_at as i32,
                theme.accent_2,
                theme.surface,
                keys.trim_end(),
                false,
            );
            surface.put_text_styled(
                inner.col + 25,
                row,
                text_width.saturating_sub(25),
                theme::brighten(theme.muted, 44),
                theme.surface,
                &truncate_to_cells(description.trim(), text_width.saturating_sub(25) as usize),
                false,
            );
        }
    }
    render_cell_scrollbar(
        surface,
        inner.col + inner.cols - 1,
        list_row,
        list_rows as i32,
        list_rows,
        lines.len(),
        help.scroll,
        theme.border,
        theme.surface,
        "╎",
        theme.accent_2,
        "┃",
    );
}
