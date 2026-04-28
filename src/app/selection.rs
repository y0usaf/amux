use crate::state::{Project, Session};

pub(super) fn session_index_for_restore_key(sessions: &[Session], key: &str) -> Option<usize> {
    sessions
        .iter()
        .position(|session| session.selection_key() == key || session.local_id == key)
}

pub(super) fn first_visible_session_index(project: &Project) -> Option<usize> {
    project
        .sessions
        .iter()
        .position(Session::should_render_in_sidebar)
}

pub(super) fn ephemeral_draft_session_index(project: &Project) -> Option<usize> {
    project
        .sessions
        .iter()
        .position(Session::is_ephemeral_draft)
}

pub(super) fn preferred_session_index(project: &Project) -> Option<usize> {
    first_visible_session_index(project)
        .or_else(|| ephemeral_draft_session_index(project))
        .or_else(|| (!project.sessions.is_empty()).then_some(0))
}

pub(super) fn visible_session_indices(project: &Project) -> Vec<usize> {
    project
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| session.should_render_in_sidebar())
        .map(|(index, _)| index)
        .collect()
}

pub(super) fn next_selectable_session_index(
    project: &Project,
    anchor_index: usize,
) -> Option<usize> {
    let visible = visible_session_indices(project);
    if visible.is_empty() {
        return preferred_session_index(project);
    }

    visible
        .iter()
        .copied()
        .find(|index| *index >= anchor_index)
        .or_else(|| visible.last().copied())
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

        assert_eq!(first_visible_session_index(&project), Some(1));
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
}
