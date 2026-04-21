#[path = "state/merge.rs"]
mod merge;
#[path = "state/persisted.rs"]
mod persisted;
#[path = "state/project.rs"]
mod project;
#[path = "state/session.rs"]
mod session;
#[path = "state/sort.rs"]
mod sort;

pub use merge::merge_scanned_sessions;
pub use persisted::{default_state_path, PersistedState};
pub use project::Project;
pub use session::{Session, SessionRuntime};
pub use sort::compare_sessions;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{compare_sessions, PersistedState, Session};

    #[test]
    fn promoted_session_sorts_above_newer_idle_session() {
        let mut promoted = Session::new_draft();
        promoted.name = "promoted".into();
        promoted.updated_at_ms = 100;
        promoted.promoted_at_ms = 200;

        let mut idle = Session::new_draft();
        idle.name = "idle".into();
        idle.updated_at_ms = 500;

        assert_eq!(compare_sessions(&promoted, &idle), std::cmp::Ordering::Less);
    }

    #[test]
    fn session_promotion_never_regresses() {
        let mut session = Session::new_draft();
        session.updated_at_ms = 100;
        session.promoted_at_ms = 250;

        session.promote_at(200);

        assert_eq!(session.updated_at_ms, 200);
        assert_eq!(session.promoted_at_ms, 250);
    }

    #[test]
    fn ephemeral_draft_does_not_render_in_sidebar() {
        let session = Session::new_draft();

        assert!(session.is_ephemeral_draft());
        assert!(!session.should_render_in_sidebar());
    }

    #[test]
    fn running_session_stays_hidden_until_session_file_exists() {
        let mut session = Session::new_draft();
        session.draft = false;
        session.runtime.running = true;

        assert!(!session.should_render_in_sidebar());

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pi-harness-sidebar-materialized-{}-{unique}.jsonl",
            std::process::id()
        ));
        fs::write(&path, "{}\n").unwrap();
        session.session_file = Some(path.clone());

        assert!(session.should_render_in_sidebar());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn finished_unlinked_session_renders_in_sidebar() {
        let mut session = Session::new_draft();
        session.draft = false;
        session.pi_session_id = Some("pi-session-1".into());

        assert!(session.should_render_in_sidebar());
    }

    #[test]
    fn persisted_project_order_is_preserved_while_deduping() {
        let mut state = PersistedState {
            projects: vec![
                "/tmp/project-b".into(),
                "/tmp/project-a".into(),
                "/tmp/project-b".into(),
            ],
            ..PersistedState::default()
        };

        state.normalize();

        assert_eq!(
            state.projects,
            vec!["/tmp/project-b".to_string(), "/tmp/project-a".to_string()]
        );
    }
}
