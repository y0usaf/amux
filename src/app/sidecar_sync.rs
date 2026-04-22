use crate::pi::PiSidecarSnapshot;
use crate::state::Session;
use crate::terminal::TerminalStatus;
use crate::util::now_millis;

use super::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidecarOrderUpdate {
    None,
    Touch,
    Promote,
}

pub(super) fn should_bind_sidecar_session(session: &Session, snapshot: &PiSidecarSnapshot) -> bool {
    session.pi_session_id.is_some()
        || session.session_file.is_some()
        || session.runtime.running
        || session.runtime.queued
        || snapshot.stage.is_active()
        || snapshot.queued
}

pub(super) fn sidecar_order_update(
    prev_running: bool,
    running: bool,
    prev_trackable: bool,
    trackable: bool,
) -> SidecarOrderUpdate {
    if running {
        if !trackable {
            SidecarOrderUpdate::None
        } else if !prev_running || !prev_trackable {
            SidecarOrderUpdate::Promote
        } else {
            SidecarOrderUpdate::None
        }
    } else if prev_running {
        if !trackable {
            SidecarOrderUpdate::None
        } else if !prev_trackable {
            SidecarOrderUpdate::Promote
        } else {
            SidecarOrderUpdate::Touch
        }
    } else {
        SidecarOrderUpdate::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SidecarApplyResult {
    reordered: bool,
    promote_project: bool,
}

fn terminal_status_note(status: Option<&TerminalStatus>) -> Option<String> {
    match status {
        Some(TerminalStatus::Error(error)) => Some(format!("terminal: {error}")),
        Some(TerminalStatus::Exited(status)) => Some(format!("terminal exited: {status}")),
        Some(TerminalStatus::Empty | TerminalStatus::Launching | TerminalStatus::Running)
        | None => None,
    }
}

fn is_terminal_status_note(note: &str) -> bool {
    note.starts_with("terminal: ") || note.starts_with("terminal exited: ")
}

fn reconcile_terminal_note(
    current_note: Option<&str>,
    status: Option<&TerminalStatus>,
) -> Option<String> {
    match terminal_status_note(status) {
        Some(note) => Some(note),
        None => current_note
            .filter(|note| !is_terminal_status_note(note))
            .map(ToOwned::to_owned),
    }
}

fn apply_snapshot_to_session(
    session: &mut Session,
    snapshot: &PiSidecarSnapshot,
    selected: bool,
    now_ms: u64,
) -> SidecarApplyResult {
    let prev_running = session.runtime.running;
    let prev_queued = session.runtime.queued;
    let prev_unread = session.runtime.unread;
    if snapshot.ts_ms == 0 {
        let clears_known_activity = session.runtime.last_sidecar_ts_ms > 0
            && (prev_running || prev_queued)
            && !snapshot.stage.is_active()
            && !snapshot.queued;
        if session.runtime.last_sidecar_ts_ms > 0 && !clears_known_activity {
            return SidecarApplyResult::default();
        }
    } else if snapshot.ts_ms < session.runtime.last_sidecar_ts_ms {
        return SidecarApplyResult::default();
    }

    let timestamp = snapshot.ts_ms.max(now_ms);
    let prev_trackable = session.counts_for_activity_ordering();
    let should_bind = should_bind_sidecar_session(session, snapshot);

    if should_bind {
        session.pi_session_id = Some(snapshot.session_id.clone());
        if let Some(path) = snapshot.session_file.clone() {
            session.session_file = Some(path);
        }
        if let Some(name) = snapshot
            .session_name
            .as_ref()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
        {
            if session.should_adopt_name(name) && session.name != name {
                session.name = name.to_string();
            }
        }
        session.draft = false;
    }

    session.runtime.running = snapshot.stage.is_active();
    session.runtime.status = snapshot.stage.as_runtime_status().map(ToOwned::to_owned);
    session.runtime.queued = snapshot.queued;
    session.runtime.tool_name = snapshot.tool_name.clone();
    if snapshot.ts_ms > 0 {
        session.runtime.last_sidecar_ts_ms = session.runtime.last_sidecar_ts_ms.max(snapshot.ts_ms);
    }

    let mut result = SidecarApplyResult::default();
    let trackable = session.counts_for_activity_ordering();
    match sidecar_order_update(
        prev_running,
        session.runtime.running,
        prev_trackable,
        trackable,
    ) {
        SidecarOrderUpdate::Promote => {
            session.promote_at(timestamp);
            result.reordered = true;
            result.promote_project = true;
        }
        SidecarOrderUpdate::Touch => {
            session.touch_at(timestamp);
            result.reordered = true;
        }
        SidecarOrderUpdate::None => {}
    }

    if session.runtime.is_active() && !(prev_running || prev_queued) {
        session.runtime.unread = false;
    } else if prev_running && !session.runtime.running && trackable {
        session.runtime.unread = if selected { prev_unread } else { true };
    }

    result
}

impl App {
    fn apply_sidecar_snapshot(&mut self, snapshot: PiSidecarSnapshot) {
        if !snapshot.is_valid() {
            return;
        }

        let mut matched = None;
        for (project_index, project) in self.projects.iter().enumerate() {
            for (session_index, session) in project.sessions.iter().enumerate() {
                if session.matches_identity(
                    snapshot.harness_session_id.as_deref(),
                    &snapshot.session_id,
                    snapshot.session_file.as_deref(),
                ) {
                    matched = Some((project_index, session_index));
                    break;
                }
            }
            if matched.is_some() {
                break;
            }
        }

        let Some((project_index, session_index)) = matched else {
            return;
        };
        let selected =
            self.selected_project == project_index && self.selected_session == Some(session_index);
        let selected_session_key = self
            .current_session()
            .map(|session| session.local_id.clone());
        let update = {
            let session = &mut self.projects[project_index].sessions[session_index];
            apply_snapshot_to_session(session, &snapshot, selected, now_millis())
        };

        if update.reordered {
            self.projects[project_index].sort_sessions();
            self.restore_selection(None, selected_session_key);
        }
        if update.promote_project {
            self.promote_project_to_front(project_index);
        }
        self.persist_selection();
        self.update_window_title();
    }

    pub(super) fn process_background_events(&mut self) {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        let mut changed = false;
        for (session_id, terminal) in self.terminals.iter_mut() {
            let terminal_changed = terminal.drain_events();
            if terminal_changed && selected_session_id.as_deref() == Some(session_id.as_str()) {
                changed = true;
            }
        }
        while let Some(snapshot) = self.sidecar.try_recv() {
            self.apply_sidecar_snapshot(snapshot);
            changed = true;
        }

        self.note = reconcile_terminal_note(self.note.as_deref(), self.current_terminal_status());

        if changed {
            self.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn snapshot(stage: crate::pi::PiSessionStage, ts_ms: u64) -> PiSidecarSnapshot {
        PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-1".into(),
            harness_session_id: Some("local-session-1".into()),
            session_file: Some(PathBuf::from("/tmp/pi-session-1.jsonl")),
            session_name: Some("Imported session".into()),
            stage,
            queued: false,
            tool_name: None,
            ts_ms,
        }
    }

    #[test]
    fn bind_sidecar_session_when_session_is_already_bound_by_pi_id() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        let snapshot = PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-2".into(),
            harness_session_id: None,
            session_file: None,
            session_name: None,
            stage: crate::pi::PiSessionStage::Idle,
            queued: false,
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
            stage: crate::pi::PiSessionStage::Tool,
            queued: false,
            tool_name: Some("grep".into()),
            ts_ms: 0,
        };

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
            stage: crate::pi::PiSessionStage::Idle,
            queued: false,
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
    fn stale_snapshot_does_not_regress_runtime_state() {
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();
        session.created_at_ms = 10;
        session.updated_at_ms = 10;

        let active = snapshot(crate::pi::PiSessionStage::Thinking, 200);
        let applied = apply_snapshot_to_session(&mut session, &active, false, 300);

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: true,
            }
        );
        assert!(session.runtime.running);
        assert_eq!(session.runtime.status.as_deref(), Some("thinking"));
        assert_eq!(session.runtime.last_sidecar_ts_ms, 200);
        let updated_at_ms = session.updated_at_ms;

        let stale_idle = snapshot(crate::pi::PiSessionStage::Idle, 150);
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
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 150),
            false,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
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

        let active = snapshot(crate::pi::PiSessionStage::Thinking, 200);
        apply_snapshot_to_session(&mut session, &active, false, 300);

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 0),
            false,
            400,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
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
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.updated_at_ms = 10;
        session.runtime.last_sidecar_ts_ms = 100;
        session.draft = false;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 150),
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
        session.pi_session_id = Some("stale-session-id".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Thinking, 150),
            false,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: true,
            }
        );
        assert_eq!(session.pi_session_id.as_deref(), Some("pi-session-1"));
        assert!(session.runtime.running);
    }

    #[test]
    fn zero_timestamp_snapshot_does_not_reactivate_completed_session() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.last_sidecar_ts_ms = 200;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Thinking, 0),
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
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.unread = true;
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 150),
            true,
            250,
        );

        assert_eq!(
            applied,
            SidecarApplyResult {
                reordered: true,
                promote_project: false,
            }
        );
        assert!(!session.runtime.running);
        assert_eq!(session.runtime.status, None);
        assert!(session.runtime.unread);
    }

    #[test]
    fn selected_completion_without_existing_unread_stays_read() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.running = true;
        session.runtime.status = Some("thinking".into());
        session.runtime.last_sidecar_ts_ms = 100;

        apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 150),
            true,
            250,
        );

        assert!(!session.runtime.unread);
    }

    #[test]
    fn selected_idle_snapshot_keeps_stale_unread_notification() {
        let mut session = Session::new_draft();
        session.pi_session_id = Some("pi-session-1".into());
        session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));
        session.draft = false;
        session.runtime.unread = true;
        session.runtime.last_sidecar_ts_ms = 100;

        let applied = apply_snapshot_to_session(
            &mut session,
            &snapshot(crate::pi::PiSessionStage::Idle, 150),
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
        session.pi_session_id = Some("pi-session-1".into());
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
                stage: crate::pi::PiSessionStage::Idle,
                queued: true,
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
