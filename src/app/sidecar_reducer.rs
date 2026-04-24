use crate::pi::PiSidecarSnapshot;
use crate::state::Session;
use crate::terminal::TerminalStatus;

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
pub(super) struct SidecarApplyResult {
    pub(super) reordered: bool,
    pub(super) promote_project: bool,
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

pub(super) fn reconcile_terminal_note(
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

pub(super) fn apply_snapshot_to_session(
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
    session.runtime.tool_name = if matches!(snapshot.stage, crate::pi::PiSessionStage::Tool) {
        snapshot.tool_name.clone()
    } else {
        None
    };
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
