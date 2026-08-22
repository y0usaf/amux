use crate::omp;
use crate::state::ScannedSession;

use super::super::raw::terminal_size;
use super::dialog::render_dialog_title_line;
use crate::app::cell_surface::{draw_box, render_cell_scrollbar, truncate_to_cells, CellSurface};
use crate::app::glyphs::GlyphSet;
use crate::app::layout::CellRect as Rect;
use crate::app::theme::{self, DerivedTheme};

#[derive(Clone, Debug)]
pub(in crate::app::tui) struct ArchiveViewerState {
    pub(in crate::app::tui) sessions: Vec<ScannedSession>,
    pub(in crate::app::tui) selected: usize,
    pub(in crate::app::tui) scroll: usize,
    pub(in crate::app::tui) note: Option<String>,
}

impl ArchiveViewerState {
    pub(in crate::app::tui) fn load() -> Self {
        let mut viewer = Self {
            sessions: omp::scan_archived_sessions(),
            selected: 0,
            scroll: 0,
            note: None,
        };
        viewer.clamp_selection();
        viewer
    }

    pub(in crate::app::tui) fn reload_sessions(&mut self) {
        let selected_id = self
            .selected_session()
            .map(|session| session.session_id.clone());
        self.sessions = omp::scan_archived_sessions();
        self.selected = selected_id
            .as_deref()
            .and_then(|id| {
                self.sessions
                    .iter()
                    .position(|session| session.session_id == id)
            })
            .unwrap_or(0);
        self.clamp_selection();
        self.scroll = self.scroll.min(self.sessions.len().saturating_sub(1));
    }

    pub(in crate::app::tui) fn selected_session(&self) -> Option<&ScannedSession> {
        self.sessions.get(self.selected)
    }

    pub(in crate::app::tui) fn move_selection(&mut self, delta: i32) {
        if self.sessions.is_empty() {
            return;
        }
        let max = self.sessions.len().saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + delta).clamp(0, max) as usize;
    }

    pub(in crate::app::tui) fn page_selection(&mut self, delta_pages: i32, visible_rows: usize) {
        let step = visible_rows.max(1) as i32;
        self.move_selection(delta_pages.saturating_mul(step));
    }

    pub(in crate::app::tui) fn select_first(&mut self) {
        self.selected = 0;
    }

    pub(in crate::app::tui) fn select_last(&mut self) {
        self.selected = self.sessions.len().saturating_sub(1);
    }

    pub(in crate::app::tui) fn ensure_selection_visible(&mut self, visible_rows: usize) {
        if self.sessions.is_empty() || visible_rows == 0 {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected.saturating_sub(visible_rows.saturating_sub(1));
        }
        self.scroll = self
            .scroll
            .min(self.sessions.len().saturating_sub(visible_rows));
    }

    pub(in crate::app::tui) fn clamp_selection(&mut self) {
        if self.sessions.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.sessions.len() - 1);
        }
    }
}

pub(in crate::app::tui) fn archive_viewer_visible_rows_for_terminal() -> usize {
    let (cols, rows) = terminal_size();
    archive_viewer_list_rows(archive_viewer_rect(i32::from(cols), i32::from(rows)))
}

fn archive_viewer_rect(cols: i32, rows: i32) -> Rect {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let width = cols.clamp(1, 110);
    let height = rows.clamp(1, 32);
    Rect::new((cols - width) / 2, (rows - height) / 2, width, height)
}

fn archive_viewer_list_rows(rect: Rect) -> usize {
    rect.rows.saturating_sub(7) as usize
}

pub(in crate::app::tui) fn render_archive_viewer(
    surface: &mut CellSurface,
    viewer: &mut ArchiveViewerState,
    theme: &DerivedTheme,
    glyphs: &GlyphSet,
) {
    let rect = archive_viewer_rect(surface.cols, surface.rows);
    draw_box(
        surface,
        rect,
        theme.text,
        theme.surface_raised,
        theme.status_bg,
        glyphs,
    );
    if rect.cols <= 2 || rect.rows <= 2 {
        return;
    }

    let inner = rect.inset_edges(2, 1, 2, 1);
    surface.fill_rect(inner, theme.text, theme.surface);
    let count = format!(" {} archived ", viewer.sessions.len());
    render_dialog_title_line(
        surface,
        inner.row,
        inner.col,
        inner.cols,
        " ARCHIVE ",
        &count,
        theme,
        glyphs,
    );

    let hint = "↑/↓/j/k select  Enter restore  r reload  q/Esc close";
    surface.put_text(
        inner.col,
        inner.row + 2,
        inner.cols,
        theme.muted,
        theme.surface,
        hint,
    );
    let header_row = inner.row + 3;
    surface.put_text(
        inner.col,
        header_row,
        inner.cols,
        theme::brighten(theme.muted, 24),
        theme.surface,
        "UPDATED  SESSION / PROJECT",
    );
    let list_row = inner.row + 4;
    let footer_row = rect.row + rect.rows - 2;
    let list_rows = (footer_row - list_row).max(0) as usize;
    let list_width = (inner.cols - 1).max(0);
    viewer.ensure_selection_visible(list_rows);

    if viewer.sessions.is_empty() {
        surface.put_text(
            inner.col,
            list_row,
            list_width,
            theme.muted,
            theme.surface,
            "No archived sessions. Ctrl+Delete archives the selected session.",
        );
    } else {
        let now_ms = crate::util::now_millis();
        let end = (viewer.scroll + list_rows).min(viewer.sessions.len());
        for (row_offset, index) in (viewer.scroll..end).enumerate() {
            let row = list_row + row_offset as i32;
            let selected = index == viewer.selected;
            let line = archive_viewer_row_text(&viewer.sessions[index], now_ms);
            let row_rect = Rect::new(inner.col, row, list_width, 1);
            let row_bg = if selected {
                theme.surface_raised
            } else {
                theme.surface
            };
            let row_fg = if selected {
                theme.text
            } else {
                theme::brighten(theme.muted, 30)
            };
            surface.fill_rect(row_rect, row_fg, row_bg);
            surface.put_text_styled(
                inner.col,
                row,
                list_width,
                row_fg,
                row_bg,
                &truncate_to_cells(&line, list_width as usize),
                false,
            );
        }
        render_cell_scrollbar(
            surface,
            inner.col + inner.cols - 1,
            list_row,
            list_rows as i32,
            list_rows,
            viewer.sessions.len(),
            viewer.scroll,
            theme.border,
            theme.surface,
            "╎",
            theme.accent_2,
            glyphs.scrollbar_thumb,
        );
    }

    let footer = viewer
        .note
        .as_deref()
        .unwrap_or("Restores selected archive to its original project cwd.");
    surface.put_text(
        inner.col,
        footer_row,
        inner.cols,
        theme.muted,
        theme.surface,
        &truncate_to_cells(footer, inner.cols.max(0) as usize),
    );
}

fn archive_viewer_row_text(session: &ScannedSession, now_ms: u64) -> String {
    format!(
        "{:<7} {}  {}",
        archive_age_label(now_ms, session.updated_at_ms),
        session.name,
        session.cwd.display()
    )
}

fn archive_age_label(now_ms: u64, updated_ms: u64) -> String {
    if updated_ms == 0 {
        return "unknown".to_string();
    }
    let age_secs = now_ms.saturating_sub(updated_ms) / 1000;
    if age_secs < 60 {
        "now".to_string()
    } else if age_secs < 60 * 60 {
        format!("{}m ago", age_secs / 60)
    } else if age_secs < 60 * 60 * 24 {
        format!("{}h ago", age_secs / (60 * 60))
    } else if age_secs < 60 * 60 * 24 * 30 {
        format!("{}d ago", age_secs / (60 * 60 * 24))
    } else if age_secs < 60 * 60 * 24 * 365 {
        format!("{}mo ago", age_secs / (60 * 60 * 24 * 30))
    } else {
        format!("{}y ago", age_secs / (60 * 60 * 24 * 365))
    }
}
