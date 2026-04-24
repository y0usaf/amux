#[path = "state/merge.rs"]
mod merge;
#[path = "state/persisted.rs"]
mod persisted;
#[path = "state/project.rs"]
mod project;
#[path = "state/scanned.rs"]
mod scanned;
#[path = "state/session.rs"]
mod session;
#[path = "state/sort.rs"]
mod sort;

pub use merge::merge_scanned_sessions;
pub use persisted::{default_state_path, PersistedState};
pub use project::Project;
pub use scanned::ScannedSession;
pub use session::{Session, SessionRuntime};
pub use sort::compare_sessions;

#[cfg(test)]
mod tests {
    use super::{
        compare_sessions, merge_scanned_sessions, PersistedState, ScannedSession, Session,
    };

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
    fn running_session_renders_before_session_file_exists() {
        let mut session = Session::new_draft();
        session.draft = false;
        session.runtime.running = true;

        assert!(session.should_render_in_sidebar());
        assert!(session.counts_for_activity_ordering());
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

    #[test]
    fn normalize_selected_project_to_match_normalized_deduped_projects() {
        let current = std::env::current_dir().unwrap();
        let mut state = PersistedState {
            projects: vec!["src/../src".into(), "/tmp/other".into()],
            selected_project: Some("src/../src".into()),
            ..PersistedState::default()
        };

        state.normalize();

        assert_eq!(
            state.selected_project.as_deref(),
            Some(current.join("src").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn drop_selected_project_when_it_is_not_in_normalized_project_list() {
        let mut state = PersistedState {
            projects: vec!["/tmp/project".into()],
            selected_project: Some("/tmp/missing".into()),
            ..PersistedState::default()
        };

        state.normalize();

        assert_eq!(state.selected_project, None);
    }

    #[test]
    fn queued_draft_is_not_ephemeral_and_counts_for_activity() {
        let mut session = Session::new_draft();
        session.runtime.queued = true;

        assert!(!session.is_ephemeral_draft());
        assert!(session.should_render_in_sidebar());
        assert!(session.counts_for_activity_ordering());
    }

    #[test]
    fn persisted_selection_key_prefers_pi_session_id_over_session_file() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some("/tmp/pi-session-1.jsonl".into());

        assert_eq!(
            session.persisted_selection_key().as_deref(),
            Some("pi-session-1")
        );
        assert_eq!(session.selection_key(), "pi-session-1");
    }

    #[test]
    fn selection_key_falls_back_to_local_id_for_unpersisted_draft() {
        let session = Session::new_draft();

        assert!(session.persisted_selection_key().is_none());
        assert_eq!(session.selection_key(), session.local_id);
    }

    #[test]
    fn apply_scan_keeps_earliest_creation_timestamp_and_latest_update() {
        let mut session = Session::new_draft();
        session.created_at_ms = 500;
        session.updated_at_ms = 500;

        session.apply_scan(ScannedSession {
            session_id: "pi-session-1".into(),
            session_file: "/tmp/pi-session-1.jsonl".into(),
            cwd: "/tmp".into(),
            name: "Imported".into(),
            created_at_ms: 100,
            updated_at_ms: 200,
        });

        assert_eq!(session.name, "Imported");
        assert_eq!(session.pi_session_id.as_deref(), Some("pi-session-1"));
        assert_eq!(
            session.session_file.as_deref(),
            Some(std::path::Path::new("/tmp/pi-session-1.jsonl"))
        );
        assert_eq!(session.created_at_ms, 100);
        assert_eq!(session.updated_at_ms, 500);
        assert!(!session.draft);
    }

    #[test]
    fn apply_scan_never_regresses_materialized_creation_timestamp() {
        let mut session = Session::from_scan(ScannedSession {
            session_id: "pi-session-1".into(),
            session_file: "/tmp/pi-session-1.jsonl".into(),
            cwd: "/tmp".into(),
            name: "Imported".into(),
            created_at_ms: 100,
            updated_at_ms: 100,
        });
        session.updated_at_ms = 500;

        session.apply_scan(ScannedSession {
            session_id: "pi-session-1".into(),
            session_file: "/tmp/pi-session-1.jsonl".into(),
            cwd: "/tmp".into(),
            name: "Imported".into(),
            created_at_ms: 300,
            updated_at_ms: 200,
        });

        assert_eq!(session.created_at_ms, 100);
        assert_eq!(session.updated_at_ms, 500);
    }

    #[test]
    fn apply_scan_preserves_custom_name() {
        let mut session = Session::new_draft();
        session.name = "Pinned name".into();
        session.updated_at_ms = 500;

        session.apply_scan(ScannedSession {
            session_id: "pi-session-1".into(),
            session_file: "/tmp/pi-session-1.jsonl".into(),
            cwd: "/tmp".into(),
            name: "Imported".into(),
            created_at_ms: 100,
            updated_at_ms: 200,
        });

        assert_eq!(session.name, "Pinned name");
        assert_eq!(session.pi_session_id.as_deref(), Some("pi-session-1"));
        assert!(!session.draft);
    }

    #[test]
    fn matches_identity_accepts_any_supported_identifier() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some("/tmp/pi-session-1.jsonl".into());

        assert!(session.matches_identity(Some(&session.local_id), "other", None));
        assert!(session.matches_identity(None, "pi-session-1", None));
        assert!(session.matches_identity(
            None,
            "other",
            Some(std::path::Path::new("/tmp/pi-session-1.jsonl"))
        ));
        assert!(!session.matches_identity(
            Some("different-local"),
            "other",
            Some(std::path::Path::new("/tmp/other.jsonl"))
        ));
    }

    #[test]
    fn should_adopt_name_only_for_default_blank_or_same_name() {
        let mut session = Session::new_draft();
        session.name = "Session".into();
        assert!(session.should_adopt_name("Better name"));

        session.name = "   ".into();
        assert!(session.should_adopt_name("Better name"));

        session.name = "Pinned name".into();
        assert!(!session.should_adopt_name("Better name"));
        assert!(session.should_adopt_name("Pinned name"));
        assert!(!session.should_adopt_name("   "));
    }

    #[test]
    fn runtime_is_active_for_running_or_queued_sessions() {
        let mut session = Session::new_draft();
        assert!(!session.runtime.is_active());

        session.runtime.running = true;
        assert!(session.runtime.is_active());

        session.runtime.running = false;
        session.runtime.queued = true;
        assert!(session.runtime.is_active());
    }

    #[test]
    fn merge_scanned_sessions_updates_matching_session_without_dropping_runtime() {
        let mut existing = Session::new_draft();
        existing.pi_session_id = Some("pi-session-1".into());
        existing.session_file = Some("/tmp/session.jsonl".into());
        existing.runtime.running = true;
        existing.updated_at_ms = 500;

        let mut current = vec![existing];
        merge_scanned_sessions(
            &mut current,
            vec![ScannedSession {
                session_id: "pi-session-1".into(),
                session_file: "/tmp/session.jsonl".into(),
                cwd: "/tmp/project".into(),
                created_at_ms: 100,
                updated_at_ms: 200,
                name: "Imported".into(),
            }],
        );

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].name, "Imported");
        assert_eq!(current[0].updated_at_ms, 500);
        assert!(current[0].runtime.running);
        assert!(!current[0].draft);
    }

    #[test]
    fn merge_scanned_sessions_dedupes_duplicate_scan_entries() {
        let mut existing = Session::new_draft();
        existing.pi_session_id = Some("pi-session-1".into());
        existing.session_file = Some("/tmp/session.jsonl".into());
        existing.runtime.running = true;
        existing.updated_at_ms = 50;

        let mut current = vec![existing];
        merge_scanned_sessions(
            &mut current,
            vec![
                ScannedSession {
                    session_id: "pi-session-1".into(),
                    session_file: "/tmp/session.jsonl".into(),
                    cwd: "/tmp/project".into(),
                    created_at_ms: 100,
                    updated_at_ms: 200,
                    name: "Imported".into(),
                },
                ScannedSession {
                    session_id: "pi-session-1".into(),
                    session_file: "/tmp/session.jsonl".into(),
                    cwd: "/tmp/project".into(),
                    created_at_ms: 100,
                    updated_at_ms: 800,
                    name: "Imported again".into(),
                },
            ],
        );

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].pi_session_id.as_deref(), Some("pi-session-1"));
        assert_eq!(current[0].updated_at_ms, 800);
        assert!(current[0].runtime.running);
    }

    #[test]
    fn merge_scanned_sessions_keeps_only_draft_or_active_unmatched_sessions() {
        let draft = Session::new_draft();

        let mut running = Session::new_draft();
        running.draft = false;
        running.runtime.running = true;
        running.name = "running".into();

        let mut queued = Session::new_draft();
        queued.draft = false;
        queued.runtime.queued = true;
        queued.name = "queued".into();

        let mut stale = Session::new_draft();
        stale.draft = false;
        stale.pi_session_id = Some("pi-session-stale".into());
        stale.name = "stale".into();

        let mut current = vec![draft.clone(), running.clone(), queued.clone(), stale];
        merge_scanned_sessions(&mut current, vec![]);

        assert_eq!(current.len(), 3);
        assert!(current
            .iter()
            .any(|session| session.local_id == draft.local_id));
        assert!(current
            .iter()
            .any(|session| session.local_id == running.local_id));
        assert!(current
            .iter()
            .any(|session| session.local_id == queued.local_id));
        assert!(!current.iter().any(|session| session.name == "stale"));
    }

    #[test]
    fn merge_scanned_sessions_keeps_finished_sidecar_bound_session_until_scan_catches_up() {
        let mut finished = Session::new_draft();
        finished.draft = false;
        finished.name = "finished".into();
        finished.pi_session_id = Some("pi-session-1".into());
        finished.session_file = Some("/tmp/pi-session-1.jsonl".into());
        finished.runtime.last_sidecar_ts_ms = 123;

        let local_id = finished.local_id.clone();
        let mut current = vec![finished];
        merge_scanned_sessions(&mut current, vec![]);

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].local_id, local_id);
        assert_eq!(current[0].name, "finished");
        assert!(!current[0].draft);
    }

    #[test]
    fn apply_scan_preserves_earlier_draft_creation_timestamp() {
        let mut session = Session::new_draft();
        session.created_at_ms = 100;
        session.updated_at_ms = 500;

        session.apply_scan(ScannedSession {
            session_id: "pi-session-1".into(),
            session_file: "/tmp/pi-session-1.jsonl".into(),
            cwd: "/tmp".into(),
            name: "Imported".into(),
            created_at_ms: 700,
            updated_at_ms: 200,
        });

        assert_eq!(session.created_at_ms, 100);
        assert_eq!(session.updated_at_ms, 500);
    }
}
