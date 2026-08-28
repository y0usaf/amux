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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Project;
    use std::path::PathBuf;

    #[test]
    fn session_index_for_restore_key_matches_persisted_selection_key() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        let sessions = vec![session];

        assert_eq!(
            session_index_for_restore_key(&sessions, "pi-session-1"),
            Some(0)
        );
    }

    #[test]
    fn session_index_for_restore_key_matches_local_id_for_in_memory_restore() {
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();
        session.pi_session_id = Some("pi-session-1".into());
        let sessions = vec![session];

        assert_eq!(
            session_index_for_restore_key(&sessions, "local-session-1"),
            Some(0)
        );
    }

    #[test]
    fn session_index_for_restore_key_returns_none_for_unknown_key() {
        let sessions = vec![Session::new_draft()];

        assert_eq!(session_index_for_restore_key(&sessions, "missing"), None);
    }

    #[test]
    fn preferred_session_index_prefers_visible_session_over_hidden_draft() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let draft = Session::new_draft();
        let mut visible = Session::new_draft();
        visible.pi_session_id = Some("pi-session-1".into());
        visible.draft = false;
        project.sessions = vec![draft, visible];

        assert_eq!(first_selectable_session_index(&project), Some(1));
        assert_eq!(ephemeral_draft_session_index(&project), Some(0));
        assert_eq!(preferred_session_index(&project), Some(1));
    }

    #[test]
    fn next_selectable_session_index_skips_hidden_draft_when_visible_session_remains() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut first = Session::new_draft();
        first.pi_session_id = Some("pi-session-1".into());
        first.draft = false;
        let hidden = Session::new_draft();
        let mut third = Session::new_draft();
        third.pi_session_id = Some("pi-session-2".into());
        third.draft = false;
        project.sessions = vec![first, hidden, third];

        assert_eq!(next_selectable_session_index(&project, 1), Some(2));
        assert_eq!(next_selectable_session_index(&project, 2), Some(2));
        assert_eq!(next_selectable_session_index(&project, 3), Some(2));
    }

    #[test]
    fn sidebar_session_order_nests_subagent_chains_under_their_parent() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut parent = Session::new_draft();
        parent.session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        parent.draft = false;
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        child.draft = false;
        let mut other = Session::new_draft();
        other.session_file = Some(PathBuf::from("/sessions/other.jsonl"));
        other.draft = false;
        project.sessions = vec![parent, child, other];

        let order = sidebar_session_order(&project);
        // Parent (index 0) first, its subagent (index 1) nested under it,
        // then the unrelated top-level session (index 2).
        assert_eq!(order, vec![(0, 0), (1, 1), (2, 0)]);
    }

    #[test]
    fn sidebar_session_order_nests_nested_subagents_under_their_own_parent() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut root = Session::new_draft();
        root.session_file = Some(PathBuf::from("/sessions/root.jsonl"));
        root.draft = false;
        let mut sub = Session::new_draft();
        sub.session_file = Some(PathBuf::from("/sessions/root/sub.jsonl"));
        sub.parent_session_file = Some(PathBuf::from("/sessions/root.jsonl"));
        sub.draft = false;
        let mut leaf = Session::new_draft();
        leaf.parent_session_file = Some(PathBuf::from("/sessions/root/sub.jsonl"));
        leaf.draft = false;
        project.sessions = vec![root, sub, leaf];

        assert_eq!(
            sidebar_session_order(&project),
            vec![(0, 0), (1, 1), (2, 2)]
        );
    }

    #[test]
    fn first_selectable_session_index_skips_children_when_a_top_level_row_exists() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        child.draft = false;
        let mut top = Session::new_draft();
        top.session_file = Some(PathBuf::from("/sessions/top.jsonl"));
        top.draft = false;
        project.sessions = vec![child, top];

        assert_eq!(first_selectable_session_index(&project), Some(1));
    }

    #[test]
    fn first_selectable_session_index_returns_none_when_only_children_exist() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        child.draft = false;
        project.sessions = vec![child];

        assert_eq!(first_selectable_session_index(&project), None);
        assert_eq!(preferred_session_index(&project), None);
    }

    #[test]
    fn next_selectable_session_index_skips_subagent_children() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut parent = Session::new_draft();
        parent.session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        parent.draft = false;
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        child.draft = false;
        let mut other = Session::new_draft();
        other.session_file = Some(PathBuf::from("/sessions/other.jsonl"));
        other.draft = false;
        project.sessions = vec![parent, child, other];

        assert_eq!(next_selectable_session_index(&project, 0), Some(0));
        assert_eq!(next_selectable_session_index(&project, 1), Some(2));
        assert_eq!(next_selectable_session_index(&project, 2), Some(2));
        assert_eq!(next_selectable_session_index(&project, 3), Some(2));
    }

    #[test]
    fn selectable_session_index_resolves_subagent_to_parent() {
        let mut parent = Session::new_draft();
        parent.session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        parent.draft = false;
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/parent.jsonl"));
        child.draft = false;
        let sessions = vec![parent, child];

        assert_eq!(selectable_session_index(&sessions, 0), Some(0));
        assert_eq!(selectable_session_index(&sessions, 1), Some(0));
    }

    #[test]
    fn selectable_session_index_rejects_orphaned_subagent() {
        let mut child = Session::new_draft();
        child.parent_session_file = Some(PathBuf::from("/sessions/gone.jsonl"));
        child.draft = false;
        let sessions = vec![child];

        assert_eq!(selectable_session_index(&sessions, 0), None);
    }
}
