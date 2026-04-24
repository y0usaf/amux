use crate::render::{Color, TextRenderer};
use crate::state::Session;

use super::layout::Rect;
use super::theme::{ACCENT, MUTED, RUNNING, TEXT, WARNING};
use super::App;

pub(super) const SIDEBAR_SPINNER_FRAMES: &[&str] =
    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(super) const SIDEBAR_SPINNER_FRAME_MS: u64 = 80;
pub(super) const SIDEBAR_NOTIFICATION_GLYPH: &str = "⣿";

#[derive(Clone, Debug)]
pub(super) enum SidebarRowKind {
    ActionOpenProject,
    Label,
    Project(usize),
    Session {
        project_index: usize,
        session_index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarStatusKind {
    Active,
    Queued,
    Notification,
}

#[derive(Clone, Debug)]
pub(super) struct SidebarRow {
    pub(super) kind: SidebarRowKind,
    pub(super) text: String,
    pub(super) fg: Color,
    pub(super) bg: Option<Color>,
    pub(super) inverted: bool,
    pub(super) status: Option<SidebarStatusKind>,
}

impl SidebarRow {
    pub(super) fn is_hoverable(&self) -> bool {
        !matches!(self.kind, SidebarRowKind::Label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// anchor_row = selected project's header when one exists; selected_row = active row.
pub(super) struct SidebarSelectionSpan {
    pub(super) anchor_row: usize,
    pub(super) selected_row: usize,
}

fn clamp_scroll_into_range(current_scroll: usize, min_scroll: usize, max_scroll: usize) -> usize {
    if min_scroll <= max_scroll {
        current_scroll.clamp(min_scroll, max_scroll)
    } else {
        max_scroll
    }
}

fn sync_sidebar_scroll_to_row(
    current_scroll: usize,
    row_count: usize,
    visible_rows: usize,
    row_index: usize,
) -> usize {
    let max_scroll = row_count.saturating_sub(visible_rows);
    if visible_rows == 0 {
        return current_scroll.min(max_scroll);
    }

    let min_scroll = row_index.saturating_add(1).saturating_sub(visible_rows);
    clamp_scroll_into_range(
        current_scroll.min(max_scroll),
        min_scroll,
        row_index.min(max_scroll),
    )
}

fn sync_sidebar_scroll_to_selection_span(
    current_scroll: usize,
    row_count: usize,
    visible_rows: usize,
    span: SidebarSelectionSpan,
) -> usize {
    // Selection sync rule: keep the selected project's header visible alongside the
    // selected row when possible; otherwise reserve a sticky header row for it.
    let max_scroll = row_count.saturating_sub(visible_rows);
    let current_scroll = current_scroll.min(max_scroll);
    if visible_rows == 0 {
        return current_scroll;
    }

    if span.anchor_row >= span.selected_row {
        return sync_sidebar_scroll_to_row(
            current_scroll,
            row_count,
            visible_rows,
            span.selected_row,
        );
    }

    let min_scroll = span
        .selected_row
        .saturating_add(1)
        .saturating_sub(visible_rows);
    let max_scroll_with_anchor = span.anchor_row.min(max_scroll);
    if min_scroll <= max_scroll_with_anchor {
        return clamp_scroll_into_range(current_scroll, min_scroll, max_scroll_with_anchor);
    }

    if visible_rows < 2 {
        return sync_sidebar_scroll_to_row(
            current_scroll,
            row_count,
            visible_rows,
            span.selected_row,
        );
    }

    let sticky_min_scroll = span
        .selected_row
        .saturating_add(2)
        .saturating_sub(visible_rows)
        .max(span.anchor_row.saturating_add(1))
        .min(max_scroll);
    clamp_scroll_into_range(
        current_scroll,
        sticky_min_scroll,
        span.selected_row.min(max_scroll),
    )
}

pub(super) fn sticky_sidebar_anchor_row(
    scroll: usize,
    visible_rows: usize,
    span: SidebarSelectionSpan,
) -> Option<usize> {
    if visible_rows < 2 || span.anchor_row >= span.selected_row || span.anchor_row >= scroll {
        return None;
    }

    let body_visible_rows = visible_rows - 1;
    let body_last_visible = scroll.saturating_add(body_visible_rows.saturating_sub(1));
    (span.selected_row >= scroll && span.selected_row <= body_last_visible)
        .then_some(span.anchor_row)
}

pub(super) fn session_sidebar_status(session: &Session) -> Option<SidebarStatusKind> {
    if session.runtime.running {
        Some(SidebarStatusKind::Active)
    } else if session.runtime.queued {
        Some(SidebarStatusKind::Queued)
    } else if session.runtime.unread {
        Some(SidebarStatusKind::Notification)
    } else {
        None
    }
}

pub(super) fn sidebar_status_color(status: SidebarStatusKind) -> Color {
    match status {
        SidebarStatusKind::Active => RUNNING,
        SidebarStatusKind::Queued => WARNING,
        SidebarStatusKind::Notification => ACCENT,
    }
}

pub(super) fn sidebar_status_glyph(status: SidebarStatusKind, now_ms: u64) -> &'static str {
    match status {
        SidebarStatusKind::Active | SidebarStatusKind::Queued => {
            let frame =
                ((now_ms / SIDEBAR_SPINNER_FRAME_MS) as usize) % SIDEBAR_SPINNER_FRAMES.len();
            SIDEBAR_SPINNER_FRAMES[frame]
        }
        SidebarStatusKind::Notification => SIDEBAR_NOTIFICATION_GLYPH,
    }
}

impl App {
    pub(super) fn has_sidebar_spinner(&self) -> bool {
        self.workspace
            .projects()
            .iter()
            .flat_map(|project| project.sessions.iter())
            .any(|session| session.runtime.running || session.runtime.queued)
    }

    pub(super) fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let projects = self.workspace.projects();
        let selected_project = self.workspace.selected_project_index();
        let selected_session = self.workspace.selected_session_index();
        let mut rows = vec![SidebarRow {
            kind: SidebarRowKind::ActionOpenProject,
            text: "+ NEW PROJECT".to_string(),
            fg: ACCENT,
            bg: None,
            inverted: projects.is_empty(),
            status: None,
        }];
        let selected_session_visible = self.current_session_visible_in_sidebar();

        if !projects.is_empty() {
            rows.push(SidebarRow {
                kind: SidebarRowKind::Label,
                text: String::new(),
                fg: MUTED,
                bg: None,
                inverted: false,
                status: None,
            });
        }

        for (project_index, project) in projects.iter().enumerate() {
            let project_selected = project_index == selected_project;
            rows.push(SidebarRow {
                kind: SidebarRowKind::Project(project_index),
                text: project.name.to_uppercase(),
                fg: if project_selected { ACCENT } else { TEXT },
                bg: None,
                inverted: project_selected,
                status: None,
            });

            for (session_index, session) in project.sessions.iter().enumerate() {
                if !session.should_render_in_sidebar() {
                    continue;
                }
                let selected = project_selected
                    && selected_session_visible
                    && Some(session_index) == selected_session;
                let status = session_sidebar_status(session);
                rows.push(SidebarRow {
                    kind: SidebarRowKind::Session {
                        project_index,
                        session_index,
                    },
                    text: session.name.clone(),
                    fg: status.map(sidebar_status_color).unwrap_or(if selected {
                        TEXT
                    } else {
                        MUTED
                    }),
                    bg: None,
                    inverted: selected,
                    status,
                });
            }

            if project_index + 1 < projects.len() {
                rows.push(SidebarRow {
                    kind: SidebarRowKind::Label,
                    text: String::new(),
                    fg: MUTED,
                    bg: None,
                    inverted: false,
                    status: None,
                });
            }
        }

        rows
    }

    pub(super) fn sidebar_visible_rows(
        &self,
        rect: Rect,
        text: &TextRenderer,
        panel_pad: i32,
    ) -> usize {
        let cell_h = text.metrics.cell_height.max(1);
        ((rect.h - panel_pad * 2).max(0) / cell_h) as usize
    }

    pub(super) fn selected_sidebar_row_index(&self, rows: &[SidebarRow]) -> Option<usize> {
        let selected_session_visible = self.current_session_visible_in_sidebar();
        let selected_project = self.workspace.selected_project_index();
        let selected_session = self.workspace.selected_session_index();
        rows.iter().position(|row| match row.kind {
            SidebarRowKind::ActionOpenProject => self.workspace.projects().is_empty(),
            SidebarRowKind::Project(project_index) => {
                project_index == selected_project && !selected_session_visible
            }
            SidebarRowKind::Session {
                project_index,
                session_index,
            } => {
                selected_session_visible
                    && project_index == selected_project
                    && Some(session_index) == selected_session
            }
            SidebarRowKind::Label => false,
        })
    }

    pub(super) fn selected_sidebar_selection_span(
        &self,
        rows: &[SidebarRow],
    ) -> Option<SidebarSelectionSpan> {
        let selected_row = self.selected_sidebar_row_index(rows)?;
        let selected_project = self.workspace.selected_project_index();
        let anchor_row = rows
            .iter()
            .position(|row| matches!(row.kind, SidebarRowKind::Project(index) if index == selected_project))
            .unwrap_or(selected_row);
        Some(SidebarSelectionSpan {
            anchor_row,
            selected_row,
        })
    }

    pub(super) fn sticky_sidebar_anchor_row_index(
        &self,
        rows: &[SidebarRow],
        visible_rows: usize,
    ) -> Option<usize> {
        self.selected_sidebar_selection_span(rows)
            .and_then(|span| sticky_sidebar_anchor_row(self.sidebar_scroll, visible_rows, span))
    }

    pub(super) fn sidebar_row_index_at_visible_row(
        &self,
        rows: &[SidebarRow],
        visible_rows: usize,
        visible_row: usize,
    ) -> Option<usize> {
        if visible_row >= visible_rows {
            return None;
        }

        let sticky_row = self.sticky_sidebar_anchor_row_index(rows, visible_rows);
        let row_index = match sticky_row {
            Some(anchor_row) if visible_row == 0 => anchor_row,
            Some(_) => self.sidebar_scroll + visible_row.saturating_sub(1),
            None => self.sidebar_scroll + visible_row,
        };
        rows.get(row_index).map(|_| row_index)
    }

    pub(super) fn hovered_sidebar_row_index(
        &self,
        rect: Rect,
        text: &TextRenderer,
        rows: &[SidebarRow],
        panel_pad: i32,
    ) -> Option<usize> {
        if !rect.contains(self.cursor_pos.0, self.cursor_pos.1) {
            return None;
        }

        let cell_h = text.metrics.cell_height.max(1);
        let local_y = self.cursor_pos.1 as i32 - rect.y - panel_pad;
        if local_y < 0 {
            return None;
        }

        let visible_row = (local_y / cell_h) as usize;
        let row_index = self.sidebar_row_index_at_visible_row(
            rows,
            self.sidebar_visible_rows(rect, text, panel_pad),
            visible_row,
        )?;
        rows.get(row_index)
            .is_some_and(SidebarRow::is_hoverable)
            .then_some(row_index)
    }

    pub(super) fn hovered_sidebar_row_index_for_cursor(&self) -> Option<usize> {
        let (Some(window), Some(text)) = (&self.window, &self.text) else {
            return None;
        };
        let size = window.inner_size();
        let layout = self.compute_layout(size.width as i32, size.height as i32, text);
        let rows = self.sidebar_rows();
        self.hovered_sidebar_row_index(layout.sidebar, text, &rows, layout.spacing.panel_pad)
    }

    pub(super) fn clamp_sidebar_scroll(&mut self, row_count: usize, visible_rows: usize) {
        let max_scroll = row_count.saturating_sub(visible_rows);
        self.sidebar_scroll = self.sidebar_scroll.min(max_scroll);
    }

    pub(super) fn ensure_sidebar_selection_visible(
        &mut self,
        rows: &[SidebarRow],
        visible_rows: usize,
    ) {
        self.clamp_sidebar_scroll(rows.len(), visible_rows);
        let Some(span) = self.selected_sidebar_selection_span(rows) else {
            return;
        };
        self.sidebar_scroll = sync_sidebar_scroll_to_selection_span(
            self.sidebar_scroll,
            rows.len(),
            visible_rows,
            span,
        );
    }

    pub(super) fn scroll_sidebar_by_rows(
        &mut self,
        delta_rows: i32,
        visible_rows: usize,
        row_count: usize,
    ) -> bool {
        if delta_rows == 0 || row_count == 0 {
            return false;
        }

        self.clamp_sidebar_scroll(row_count, visible_rows);
        let next = if delta_rows > 0 {
            self.sidebar_scroll.saturating_add(delta_rows as usize)
        } else {
            self.sidebar_scroll
                .saturating_sub(delta_rows.unsigned_abs() as usize)
        };
        let next = next.min(row_count.saturating_sub(visible_rows));
        if next == self.sidebar_scroll {
            return false;
        }

        self.sidebar_scroll = next;
        true
    }

    pub(super) fn sync_sidebar_to_selection(&mut self) {
        self.sidebar_sync_to_selection = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Session;

    #[test]
    fn status_color_maps_each_kind() {
        assert_eq!(sidebar_status_color(SidebarStatusKind::Active), RUNNING);
        assert_eq!(sidebar_status_color(SidebarStatusKind::Queued), WARNING);
        assert_eq!(
            sidebar_status_color(SidebarStatusKind::Notification),
            ACCENT
        );
    }

    #[test]
    fn session_sidebar_status_returns_none_without_flags() {
        let session = Session::new_draft();
        assert_eq!(session_sidebar_status(&session), None);
    }

    #[test]
    fn spinner_glyph_wraps_after_last_frame() {
        let now_ms = SIDEBAR_SPINNER_FRAME_MS * SIDEBAR_SPINNER_FRAMES.len() as u64;
        assert_eq!(
            sidebar_status_glyph(SidebarStatusKind::Active, now_ms),
            SIDEBAR_SPINNER_FRAMES[0]
        );
        assert_eq!(
            sidebar_status_glyph(SidebarStatusKind::Queued, now_ms + SIDEBAR_SPINNER_FRAME_MS),
            SIDEBAR_SPINNER_FRAMES[1]
        );
    }

    #[test]
    fn selection_sync_keeps_project_header_visible_with_selected_session() {
        assert_eq!(
            sync_sidebar_scroll_to_selection_span(
                0,
                32,
                5,
                SidebarSelectionSpan {
                    anchor_row: 10,
                    selected_row: 12,
                },
            ),
            8
        );
    }

    #[test]
    fn selection_sync_uses_sticky_header_space_for_deep_session() {
        let span = SidebarSelectionSpan {
            anchor_row: 10,
            selected_row: 15,
        };
        let scroll = sync_sidebar_scroll_to_selection_span(0, 32, 5, span);

        assert_eq!(scroll, 12);
        assert_eq!(sticky_sidebar_anchor_row(scroll, 5, span), Some(10));
    }

    #[test]
    fn sticky_header_hides_when_selected_session_is_not_in_body() {
        let span = SidebarSelectionSpan {
            anchor_row: 10,
            selected_row: 15,
        };

        assert_eq!(sticky_sidebar_anchor_row(16, 5, span), None);
    }
}
