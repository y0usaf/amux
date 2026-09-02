use crate::state::{Project, Session};

const SIDEBAR_MAX_DEPTH: usize = 8;

pub(super) fn session_index_for_restore_key(sessions: &[Session], key: &str) -> Option<usize> {
    sessions
        .iter()
        .position(|session| session.selection_key() == key || session.local_id == key)
}

/// Rows a user can land on: rendered in the sidebar and never a subagent
/// child (children own no PTY, so their session cannot be opened directly).
pub(super) fn is_selectable_session(session: &Session) -> bool {
    session.should_render_in_sidebar() && !session.is_child()
}

/// The first selectable row: a fresh project should never land selection on
/// a subagent child.
pub(super) fn first_selectable_session_index(project: &Project) -> Option<usize> {
    project.sessions.iter().position(is_selectable_session)
}

pub(super) fn ephemeral_draft_session_index(project: &Project) -> Option<usize> {
    project
        .sessions
        .iter()
        .position(Session::is_ephemeral_draft)
}

pub(super) fn preferred_session_index(project: &Project) -> Option<usize> {
    first_selectable_session_index(project).or_else(|| ephemeral_draft_session_index(project))
}

pub(super) fn next_selectable_session_index(
    project: &Project,
    anchor_index: usize,
) -> Option<usize> {
    let selectable: Vec<usize> = project
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| is_selectable_session(session))
        .map(|(index, _)| index)
        .collect();
    if selectable.is_empty() {
        return preferred_session_index(project);
    }

    selectable
        .iter()
        .copied()
        .find(|index| *index >= anchor_index)
        .or_else(|| selectable.last().copied())
}

/// The row selection actually lands on. Subagent children own no PTY, so
/// pointing at one resolves to its parent row; an orphaned child (parent row
/// missing) resolves to nothing and the caller falls back.
pub(super) fn selectable_session_index(sessions: &[Session], index: usize) -> Option<usize> {
    let session = sessions.get(index)?;
    if !session.is_child() {
        return Some(index);
    }
    let parent_file = session.parent_session_file.as_ref()?;
    sessions.iter().position(|candidate| {
        !candidate.is_child() && candidate.session_file.as_ref() == Some(parent_file)
    })
}

/// Sidebar display order: top-level sessions in stored order, each followed
/// by its subagent chain (nested children nest under their own parent).
/// Returns `(session_index, depth)` pairs.
pub(super) fn sidebar_session_order(project: &Project) -> Vec<(usize, usize)> {
    let sessions = &project.sessions;
    let mut ordered: Vec<(usize, usize)> = Vec::with_capacity(sessions.len());
    let mut emitted = vec![false; sessions.len()];

    for (index, session) in sessions.iter().enumerate() {
        if !session.should_render_in_sidebar() || session.is_child() {
            continue;
        }
        ordered.push((index, 0));
        emitted[index] = true;
        append_child_chain(sessions, index, 1, &mut ordered, &mut emitted);
    }

    // Children whose parent row is missing still render, one level in, so
    // runtime-bound subagents never vanish mid-run.
    for (index, session) in sessions.iter().enumerate() {
        if session.should_render_in_sidebar() && !emitted[index] {
            ordered.push((index, 1));
        }
    }
    ordered
}

fn append_child_chain(
    sessions: &[Session],
    parent_index: usize,
    depth: usize,
    ordered: &mut Vec<(usize, usize)>,
    emitted: &mut [bool],
) {
    if depth > SIDEBAR_MAX_DEPTH {
        return;
    }
    let Some(parent_file) = sessions[parent_index].session_file.as_ref() else {
        return;
    };
    for (index, session) in sessions.iter().enumerate() {
        if emitted[index] || !session.should_render_in_sidebar() {
            continue;
        }
        if session.parent_session_file.as_ref() != Some(parent_file) {
            continue;
        }
        ordered.push((index, depth));
        emitted[index] = true;
        append_child_chain(sessions, index, depth + 1, ordered, emitted);
    }
}
