#[cfg(test)]
use super::sidecar_reducer::{apply_snapshot_to_session, reconcile_terminal_note};

#[cfg(test)]
pub(super) use super::sidecar_reducer::{
    should_bind_sidecar_session, sidecar_order_update, SidecarApplyResult, SidecarOrderUpdate,
};
#[cfg(test)]
use crate::omp::PiSidecarSnapshot;
#[cfg(test)]
use crate::state::Session;
#[cfg(test)]
use crate::terminal::TerminalStatus;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn snapshot(stage: crate::omp::PiSessionStage, ts_ms: u64) -> PiSidecarSnapshot {
        PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-1".into(),
            harness_session_id: Some("local-session-1".into()),
            session_file: Some(PathBuf::from("/tmp/pi-session-1.jsonl")),
            session_name: Some("Imported session".into()),
            stage,
            queued: false,
            interrupted: false,
            tool_name: None,
            ts_ms,
        }
    }

    #[test]
    fn bind_sidecar_session_when_session_is_already_bound_by_pi_id() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        let snapshot = PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-2".into(),
            harness_session_id: None,
            session_file: None,
            session_name: None,
            stage: crate::omp::PiSessionStage::Idle,
            queued: false,
            interrupted: false,
            tool_name: None,
            ts_ms: 0,
        };

        assert!(should_bind_sidecar_session(&session, &snapshot));
    }

    #[test]
    fn bind_sidecar_session_when_snapshot_is_active_even_for_unbound_draft() {
        let session = Session::new_draft();
        let snapshot = PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-1".into(),
            harness_session_id: None,
            session_file: None,
            session_name: None,
            stage: crate::omp::PiSessionStage::Tool,
            queued: false,
            interrupted: false,
            tool_name: Some("grep".into()),
            ts_ms: 0,
        };

        assert!(should_bind_sidecar_session(&session, &snapshot));
    }

    #[test]
    fn interrupted_snapshot_binds_new_session() {
        let session = Session::new_draft();
        let mut snapshot = snapshot(crate::omp::PiSessionStage::Idle, 150);
        snapshot.interrupted = true;

        assert!(should_bind_sidecar_session(&session, &snapshot));
    }

    #[test]
    fn bind_sidecar_session_when_session_is_already_queued() {
        let mut session = Session::new_draft();
        session.runtime.queued = true;
        let snapshot = PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-1".into(),
            harness_session_id: None,
            session_file: None,
            session_name: None,
            stage: crate::omp::PiSessionStage::Idle,
            queued: false,
            interrupted: false,
            tool_name: None,
            ts_ms: 0,
        };

        assert!(should_bind_sidecar_session(&session, &snapshot));
    }

    #[test]
    fn sidecar_order_update_is_none_for_steady_running_trackable_session() {
        assert_eq!(
            sidecar_order_update(true, true, true, true),
            SidecarOrderUpdate::None
        );
    }

    #[test]
    fn sidecar_order_update_is_none_when_session_never_started_running() {
        assert_eq!(
            sidecar_order_update(false, false, false, true),
            SidecarOrderUpdate::None
        );
        assert_eq!(
            sidecar_order_update(false, false, true, true),
            SidecarOrderUpdate::None
        );
    }

    #[test]
    fn reconcile_terminal_note_clears_stale_terminal_messages_after_recovery() {
        assert_eq!(
            reconcile_terminal_note(Some("terminal exited: 0"), Some(&TerminalStatus::Running)),
            None
        );
        assert_eq!(
            reconcile_terminal_note(Some("terminal: boom"), Some(&TerminalStatus::Empty)),
            None
        );
    }

    #[test]
    fn reconcile_terminal_note_preserves_unrelated_messages() {
        assert_eq!(
            reconcile_terminal_note(
                Some("sidecar extension not found"),
                Some(&TerminalStatus::Running)
            ),
            Some("sidecar extension not found".into())
        );
    }

    #[test]
    fn reconcile_terminal_note_formats_current_terminal_status() {
        assert_eq!(
            reconcile_terminal_note(None, Some(&TerminalStatus::Error("boom".into()))),
            Some("terminal: boom".into())
        );
        assert_eq!(
            reconcile_terminal_note(None, Some(&TerminalStatus::Exited("1".into()))),
            Some("terminal exited: 1".into())
        );
    }

    #[test]
    fn non_tool_snapshot_clears_stale_tool_name() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.tool_name = Some("Clipboard".into());

        let mut next = snapshot(crate::omp::PiSessionStage::Thinking, 150);
        next.tool_name = Some("Clipboard".into());
        apply_snapshot_to_session(&mut session, &next, false, 250);

        assert_eq!(session.runtime.status.as_deref(), Some("thinking"));
        assert_eq!(session.runtime.tool_name, None);
    }

    #[test]
    fn interrupted_snapshot_sets_runtime_flag() {
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();

        let mut next = snapshot(crate::omp::PiSessionStage::Idle, 150);
        next.interrupted = true;
        apply_snapshot_to_session(&mut session, &next, false, 250);

        assert!(session.runtime.interrupted);
        assert_eq!(session.runtime.status, None);
    }

    #[test]
    fn stale_snapshot_does_not_regress_runtime_state() {
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();
        session.created_at_ms = 10;
        session.updated_at_ms = 10;

        let active = snapshot(crate::omp::PiSessionStage::Thinking, 200);
        let applied = apply_snapshot_to_session(&mut session, &active, false, 300);

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: true,
                identity_changed: true,
            }
        );
        assert!(session.runtime.running);
        assert_eq!(session.runtime.status.as_deref(), Some("thinking"));
        assert_eq!(session.runtime.last_sidecar_ts_ms, 200);
        let updated_at_ms = session.updated_at_ms;

        let stale_idle = snapshot(crate::omp::PiSessionStage::Idle, 150);
        let applied_stale = apply_snapshot_to_session(&mut session, &stale_idle, false, 400);

        assert_eq!(applied_stale, SidecarApplyResult::default());
        assert!(session.runtime.running);
        assert_eq!(session.runtime.status.as_deref(), Some("thinking"));
        assert!(!session.runtime.unread);
        assert_eq!(session.updated_at_ms, updated_at_ms);
        assert_eq!(session.runtime.last_sidecar_ts_ms, 200);
    }

    #[test]
    fn fresh_completion_marks_unread_and_touches_session() {
        let mut session = Session::new_draft();
        session.created_at_ms = 10;
        session.updated_at_ms = 10;
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 150),
            false,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
                identity_changed: false,
            }
        );
        assert!(!session.runtime.running);
        assert_eq!(session.runtime.status, None);
        assert!(session.runtime.unread);
        assert_eq!(session.updated_at_ms, 250);
        assert_eq!(session.runtime.last_sidecar_ts_ms, 150);
    }

    #[test]
    fn zero_timestamp_snapshot_can_still_complete_running_session() {
        let mut session = Session::new_draft();
        session.created_at_ms = 10;
        session.updated_at_ms = 10;

        let active = snapshot(crate::omp::PiSessionStage::Thinking, 200);
        apply_snapshot_to_session(&mut session, &active, false, 300);

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 0),
            false,
            400,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
                identity_changed: false,
            }
        );
        assert!(!session.runtime.running);
        assert_eq!(session.runtime.status, None);
        assert!(session.runtime.unread);
        assert_eq!(session.updated_at_ms, 400);
        assert_eq!(session.runtime.last_sidecar_ts_ms, 200);
    }

    #[test]
    fn idle_name_adoption_does_not_reorder_session() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.updated_at_ms = 10;
        session.runtime.last_sidecar_ts_ms = 100;
        session.draft = false;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 150),
            false,
            250,
        );

        assert_eq!(applied, SidecarApplyResult::default());
        assert_eq!(session.name, "Imported session");
        assert_eq!(session.updated_at_ms, 10);
        assert_eq!(session.runtime.last_sidecar_ts_ms, 150);
    }

    #[test]
    fn binding_snapshot_refreshes_stale_pi_session_id() {
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();
        session.omp_session_id = Some("stale-session-id".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Thinking, 150),
            false,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: true,
                identity_changed: true,
            }
        );
        assert_eq!(session.omp_session_id.as_deref(), Some("pi-session-1"));
        assert!(session.runtime.running);
    }

    #[test]
    fn zero_timestamp_snapshot_does_not_reactivate_completed_session() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.last_sidecar_ts_ms = 200;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Thinking, 0),
            false,
            400,
        );

        assert_eq!(applied, SidecarApplyResult::default());
        assert!(!session.runtime.running);
        assert_eq!(session.runtime.status, None);
        assert_eq!(session.runtime.last_sidecar_ts_ms, 200);
    }

    #[test]
    fn selected_completion_keeps_existing_unread_notification() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.unread = true;
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 150),
            true,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
                identity_changed: false,
            }
        );
        assert!(!session.runtime.running);
        assert_eq!(session.runtime.status, None);
        assert!(session.runtime.unread);
    }

    #[test]
    fn selected_completion_without_existing_unread_stays_read() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.last_sidecar_ts_ms = 100;

        apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 150),
            true,
            250,
        );

        assert!(!session.runtime.unread);
    }

    #[test]
    fn selected_idle_snapshot_keeps_stale_unread_notification() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.unread = true;
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::omp::PiSessionStage::Idle, 150),
            true,
            250,
        );

        assert_eq!(applied, SidecarApplyResult::default());
        assert!(session.runtime.unread);
        assert_eq!(session.runtime.status, None);
    }

    #[test]
    fn queued_snapshot_clears_unread_notification() {
        let mut session = Session::new_draft();
        session.omp_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.unread = true;
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &PiSidecarSnapshot {
                kind: Some("snapshot".into()),
                session_id: "pi-session-1".into(),
                harness_session_id: Some("local-session-1".into()),
                session_file: Some(PathBuf::from("/tmp/pi-session-1.jsonl")),
                session_name: Some("Imported session".into()),
                stage: crate::omp::PiSessionStage::Idle,
                queued: true,
                interrupted: false,
                tool_name: None,
                ts_ms: 150,
            },
            true,
            250,
        );

        assert_eq!(applied, SidecarApplyResult::default());
        assert!(session.runtime.queued);
        assert!(!session.runtime.unread);
    }
}
