use crate::render::Color;
use crate::state::{Project, Session};

use super::glyphs::GlyphSet;
use super::selection::sidebar_session_order;
use super::theme::Role;

pub(super) const SIDEBAR_ANIMATION_FRAME_MS: u64 = 10;
pub(super) const SIDEBAR_SPINNER_FRAME_TICKS: u64 = 6;
pub(super) const SIDEBAR_SPINNER_FRAME_MS: u64 =
    SIDEBAR_ANIMATION_FRAME_MS * SIDEBAR_SPINNER_FRAME_TICKS;
pub(super) const SIDEBAR_SPINNER_FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⢸", "⢰", "⣰", "⣠", "⣤", "⣄", "⣆", "⡆", "⡇", "⠇", "⠏",
];
pub(super) const SIDEBAR_NOTIFICATION_GLYPH: &str = "⣿";
pub(super) const SIDEBAR_CROWN_BLINK_MS: u64 = 400;
pub(super) const SIDEBAR_CROWN_JEWEL: &str = "✦";
pub(super) const SIDEBAR_CROWN_JEWEL_OPEN: &str = "✧";

#[derive(Clone, Debug)]
pub(super) enum SidebarRowKind {
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
    Interrupted,
    Notification,
    Input,
}

#[derive(Clone, Debug)]
pub(super) struct SidebarRow {
    pub(super) kind: SidebarRowKind,
    /// Nesting level: 0 for projects and top-level sessions, 1+ for
    /// subagent rows rendered under their parent.
    pub(super) depth: usize,
    pub(super) text: String,
    pub(super) fg: Role,
    pub(super) bg: Option<Color>,
    pub(super) inverted: bool,
    pub(super) selector: bool,
    pub(super) current: bool,
    pub(super) status: Option<SidebarStatusKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// anchor_row = selected project's header when one exists; selected_row = active row.
pub(super) struct SidebarSelectionSpan {
    pub(super) anchor_row: usize,
    pub(super) selected_row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SidebarViewportItem {
    pub(super) row_index: usize,
    pub(super) visible_row: usize,
    pub(super) sticky: bool,
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

pub(super) fn sidebar_viewport_items(
    rows: &[SidebarRow],
    scroll: usize,
    visible_rows: usize,
    sticky_row_index: Option<usize>,
) -> Vec<SidebarViewportItem> {
    if visible_rows == 0 {
        return Vec::new();
    }

    let mut items = Vec::new();
    let body_start = usize::from(sticky_row_index.is_some());
    if let Some(row_index) = sticky_row_index.filter(|row_index| rows.get(*row_index).is_some()) {
        items.push(SidebarViewportItem {
            row_index,
            visible_row: 0,
            sticky: true,
        });
    }

    let body_rows = visible_rows.saturating_sub(body_start);
    items.extend(
        rows.iter()
            .enumerate()
            .skip(scroll)
            .take(body_rows)
            .enumerate()
            .map(|(offset, (row_index, _))| SidebarViewportItem {
                row_index,
                visible_row: body_start + offset,
                sticky: false,
            }),
    );
    items
}

pub(super) fn session_sidebar_status(session: &Session) -> Option<SidebarStatusKind> {
    if session.runtime.awaiting_interview() {
        Some(SidebarStatusKind::Input)
    } else if session.runtime.running {
        Some(SidebarStatusKind::Active)
    } else if session.runtime.queued {
        Some(SidebarStatusKind::Queued)
    } else if session.runtime.interrupted {
        Some(SidebarStatusKind::Interrupted)
    } else if session.runtime.unread {
        Some(SidebarStatusKind::Notification)
    } else {
        None
    }
}

pub(super) fn sidebar_status_color(status: SidebarStatusKind) -> Role {
    match status {
        SidebarStatusKind::Active => Role::Running,
        SidebarStatusKind::Queued => Role::Warning,
        SidebarStatusKind::Interrupted => Role::Error,
        SidebarStatusKind::Notification => Role::Success,
        SidebarStatusKind::Input => Role::Accent2,
    }
}

pub(super) fn sidebar_status_glyph(
    glyphs: &GlyphSet,
    status: SidebarStatusKind,
    now_ms: u64,
) -> &str {
    match status {
        SidebarStatusKind::Active | SidebarStatusKind::Queued => {
            let frame = ((now_ms / SIDEBAR_SPINNER_FRAME_MS) as usize
                + spinner_phase_offset(status, glyphs))
                % glyphs.spinner.len();
            glyphs.spinner[frame]
        }
        SidebarStatusKind::Interrupted
        | SidebarStatusKind::Notification
        | SidebarStatusKind::Input => glyphs.notification,
    }
}
pub(super) fn crown_jewel_glyph(
    glyphs: &GlyphSet,
    status: Option<SidebarStatusKind>,
    now_ms: u64,
) -> &str {
    match status {
        Some(SidebarStatusKind::Active) if (now_ms / SIDEBAR_CROWN_BLINK_MS) % 2 == 1 => {
            glyphs.crown_open
        }
        _ => glyphs.crown_closed,
    }
}

fn spinner_phase_offset(status: SidebarStatusKind, glyphs: &GlyphSet) -> usize {
    match status {
        SidebarStatusKind::Active => 0,
        SidebarStatusKind::Queued => glyphs.spinner.len() / 2,
        SidebarStatusKind::Interrupted
        | SidebarStatusKind::Notification
        | SidebarStatusKind::Input => 0,
    }
}

fn project_sidebar_status(project: &Project) -> Option<SidebarStatusKind> {
    let mut active = false;
    let mut queued = false;

    for session in &project.sessions {
        match session_sidebar_status(session) {
            Some(SidebarStatusKind::Input) => return Some(SidebarStatusKind::Input),
            Some(SidebarStatusKind::Interrupted) => return Some(SidebarStatusKind::Interrupted),
            Some(SidebarStatusKind::Notification) => return Some(SidebarStatusKind::Notification),
            Some(SidebarStatusKind::Active) => active = true,
            Some(SidebarStatusKind::Queued) => queued = true,
            None => {}
        }
    }

    if active {
        Some(SidebarStatusKind::Active)
    } else if queued {
        Some(SidebarStatusKind::Queued)
    } else {
        None
    }
}

pub(super) fn sidebar_has_spinner(projects: &[Project]) -> bool {
    projects
        .iter()
        .flat_map(|project| project.sessions.iter())
        .any(|session| session.runtime.running || session.runtime.queued)
}

pub(super) fn build_sidebar_rows(
    projects: &[Project],
    selected_project: usize,
    selected_session: Option<usize>,
    selected_session_visible: bool,
) -> Vec<SidebarRow> {
    let mut rows = Vec::new();

    for (project_index, project) in projects.iter().enumerate() {
        let project_selected = project_index == selected_project;
        let project_focused = project_selected && !selected_session_visible;
        let status = project_sidebar_status(project);
        rows.push(SidebarRow {
            kind: SidebarRowKind::Project(project_index),
            depth: 0,
            text: project.name.clone(),
            fg: if project_focused {
                Role::Text
            } else {
                Role::Heading
            },
            bg: None,
            inverted: project_focused,
            selector: project_focused,
            current: project_selected,
            status,
        });

        for (session_index, depth) in sidebar_session_order(project) {
            let session = &project.sessions[session_index];
            let selected = project_selected
                && selected_session_visible
                && Some(session_index) == selected_session;
            let status = session_sidebar_status(session);
            rows.push(SidebarRow {
                kind: SidebarRowKind::Session {
                    project_index,
                    session_index,
                },
                depth,
                text: session.name.clone(),
                fg: if selected { Role::Text } else { Role::Muted },
                bg: None,
                inverted: selected,
                selector: selected,
                current: selected,
                status,
            });
        }

        rows.push(SidebarRow {
            kind: SidebarRowKind::Label,
            depth: 0,
            text: String::new(),
            fg: Role::Muted,
            bg: None,
            inverted: false,
            selector: false,
            current: false,
            status: None,
        });
    }

    rows
}

pub(super) fn selected_sidebar_row_index_for_state(
    _projects_empty: bool,
    selected_project: usize,
    selected_session: Option<usize>,
    selected_session_visible: bool,
    rows: &[SidebarRow],
) -> Option<usize> {
    rows.iter().position(|row| match row.kind {
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

pub(super) fn selected_sidebar_selection_span_for_state(
    _projects_empty: bool,
    selected_project: usize,
    selected_session: Option<usize>,
    selected_session_visible: bool,
    rows: &[SidebarRow],
) -> Option<SidebarSelectionSpan> {
    let selected_row = selected_sidebar_row_index_for_state(
        _projects_empty,
        selected_project,
        selected_session,
        selected_session_visible,
        rows,
    )?;
    let anchor_row = rows
        .iter()
        .position(
            |row| matches!(row.kind, SidebarRowKind::Project(index) if index == selected_project),
        )
        .unwrap_or(selected_row);
    Some(SidebarSelectionSpan {
        anchor_row,
        selected_row,
    })
}

pub(super) fn clamp_sidebar_scroll_value(
    current_scroll: usize,
    row_count: usize,
    visible_rows: usize,
) -> usize {
    current_scroll.min(row_count.saturating_sub(visible_rows))
}

pub(super) fn ensure_sidebar_selection_visible_for_state(
    current_scroll: usize,
    _projects_empty: bool,
    selected_project: usize,
    selected_session: Option<usize>,
    selected_session_visible: bool,
    rows: &[SidebarRow],
    visible_rows: usize,
) -> usize {
    let current_scroll = clamp_sidebar_scroll_value(current_scroll, rows.len(), visible_rows);
    let Some(span) = selected_sidebar_selection_span_for_state(
        _projects_empty,
        selected_project,
        selected_session,
        selected_session_visible,
        rows,
    ) else {
        return current_scroll;
    };
    sync_sidebar_scroll_to_selection_span(current_scroll, rows.len(), visible_rows, span)
}

pub(super) fn scroll_sidebar_by_rows_value(
    current_scroll: usize,
    delta_rows: i32,
    visible_rows: usize,
    row_count: usize,
) -> (usize, bool) {
    if delta_rows == 0 || row_count == 0 {
        return (current_scroll, false);
    }

    let current_scroll = clamp_sidebar_scroll_value(current_scroll, row_count, visible_rows);
    let next = if delta_rows > 0 {
        current_scroll.saturating_add(delta_rows as usize)
    } else {
        current_scroll.saturating_sub(delta_rows.unsigned_abs() as usize)
    };
    let next = next.min(row_count.saturating_sub(visible_rows));
    (next, next != current_scroll)
}
