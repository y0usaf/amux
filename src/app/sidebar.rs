use super::theme::{ACCENT, RUNNING, SELECTION, TEXT, WARNING};
use super::*;

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
    pub(super) status: Option<SidebarStatusKind>,
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
        self.projects
            .iter()
            .flat_map(|project| project.sessions.iter())
            .any(|session| {
                matches!(
                    session_sidebar_status(session),
                    Some(SidebarStatusKind::Active | SidebarStatusKind::Queued)
                )
            })
    }

    pub(super) fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut rows = vec![SidebarRow {
            kind: SidebarRowKind::ActionOpenProject,
            text: "+ NEW PROJECT".to_string(),
            fg: ACCENT,
            bg: None,
            status: None,
        }];
        let selected_session_visible = self.current_session_visible_in_sidebar();

        if !self.projects.is_empty() {
            rows.push(SidebarRow {
                kind: SidebarRowKind::Label,
                text: String::new(),
                fg: MUTED,
                bg: None,
                status: None,
            });
        }

        for (project_index, project) in self.projects.iter().enumerate() {
            let project_selected = project_index == self.selected_project;
            rows.push(SidebarRow {
                kind: SidebarRowKind::Project(project_index),
                text: project.name.to_uppercase(),
                fg: if project_selected { ACCENT } else { TEXT },
                bg: (project_selected && !selected_session_visible).then_some(SELECTION),
                status: None,
            });

            for (session_index, session) in project.sessions.iter().enumerate() {
                if !session.should_render_in_sidebar() {
                    continue;
                }
                let selected = project_selected
                    && selected_session_visible
                    && Some(session_index) == self.selected_session;
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
                    bg: selected.then_some(SELECTION),
                    status,
                });
            }

            if project_index + 1 < self.projects.len() {
                rows.push(SidebarRow {
                    kind: SidebarRowKind::Label,
                    text: String::new(),
                    fg: MUTED,
                    bg: None,
                    status: None,
                });
            }
        }

        rows
    }

    pub(super) fn sidebar_visible_rows(&self, rect: Rect, text: &TextRenderer) -> usize {
        ((rect.h - SIDEBAR_PAD_Y * 2 * text.metrics.cell_height).max(0) / text.metrics.cell_height)
            as usize
    }

    pub(super) fn selected_sidebar_row_index(&self, rows: &[SidebarRow]) -> Option<usize> {
        let selected_session_visible = self.current_session_visible_in_sidebar();
        rows.iter().position(|row| match row.kind {
            SidebarRowKind::ActionOpenProject => self.projects.is_empty(),
            SidebarRowKind::Project(project_index) => {
                project_index == self.selected_project && !selected_session_visible
            }
            SidebarRowKind::Session {
                project_index,
                session_index,
            } => {
                selected_session_visible
                    && project_index == self.selected_project
                    && Some(session_index) == self.selected_session
            }
            SidebarRowKind::Label => false,
        })
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
        if visible_rows == 0 {
            return;
        }

        let Some(selected_index) = self.selected_sidebar_row_index(rows) else {
            return;
        };
        if selected_index < self.sidebar_scroll {
            self.sidebar_scroll = selected_index;
            return;
        }

        let last_visible = self.sidebar_scroll + visible_rows.saturating_sub(1);
        if selected_index > last_visible {
            self.sidebar_scroll = selected_index + 1 - visible_rows;
        }
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
