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
        let timestamp = snapshot.ts_ms.max(now_millis());

        let mut reordered = false;
        let mut promote_project = false;
        let session = &mut self.projects[project_index].sessions[session_index];
        let prev_running = session.runtime.running;
        let prev_trackable = session.counts_for_activity_ordering();
        let should_bind = should_bind_sidecar_session(session, &snapshot);
        let mut title_changed = false;

        if should_bind {
            session
                .pi_session_id
                .get_or_insert(snapshot.session_id.clone());
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
                    title_changed = true;
                }
            }
            session.draft = false;
        }

        session.runtime.running = snapshot.stage.is_active();
        session.runtime.status = snapshot.stage.as_runtime_status().map(ToOwned::to_owned);
        session.runtime.queued = snapshot.queued;
        session.runtime.tool_name = snapshot.tool_name.clone();

        let trackable = session.counts_for_activity_ordering();
        match sidecar_order_update(
            prev_running,
            session.runtime.running,
            prev_trackable,
            trackable,
        ) {
            SidecarOrderUpdate::Promote => {
                session.promote_at(timestamp);
                reordered = true;
                promote_project = true;
            }
            SidecarOrderUpdate::Touch => {
                session.touch_at(timestamp);
                reordered = true;
            }
            SidecarOrderUpdate::None => {
                if title_changed && snapshot.ts_ms > session.updated_at_ms {
                    session.updated_at_ms = snapshot.ts_ms;
                    reordered = true;
                }
            }
        }

        if prev_running && !session.runtime.running && trackable && !selected {
            session.runtime.unread = true;
        }

        if reordered {
            self.projects[project_index].sort_sessions();
            self.restore_selection(None, selected_session_key);
        }
        if promote_project {
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

        match self.current_terminal_status() {
            Some(TerminalStatus::Error(error)) => self.note = Some(error.clone()),
            Some(TerminalStatus::Exited(status)) => {
                self.note = Some(format!("terminal exited: {status}"))
            }
            Some(TerminalStatus::Empty | TerminalStatus::Launching | TerminalStatus::Running)
            | None => {}
        }

        if changed {
            self.request_redraw();
        }
    }
}
