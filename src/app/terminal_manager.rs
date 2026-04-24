use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use winit::event_loop::EventLoopProxy;

use crate::state::{Project, Session};
use crate::terminal::{TerminalController, TerminalStatus, TerminalTarget};

pub(super) struct TerminalManager {
    proxy: EventLoopProxy<()>,
    controllers: HashMap<String, TerminalController>,
    last_selected_session_id: Option<String>,
    sidecar_extension_path: Option<PathBuf>,
    sidecar_socket_path: PathBuf,
}

impl TerminalManager {
    pub(super) fn new(
        proxy: EventLoopProxy<()>,
        sidecar_extension_path: Option<PathBuf>,
        sidecar_socket_path: PathBuf,
    ) -> Self {
        Self {
            proxy,
            controllers: HashMap::new(),
            last_selected_session_id: None,
            sidecar_extension_path,
            sidecar_socket_path,
        }
    }

    pub(super) fn has_sidecar_extension(&self) -> bool {
        self.sidecar_extension_path.is_some()
    }

    pub(super) fn current(&self, session_id: Option<&str>) -> Option<&TerminalController> {
        self.controllers.get(session_id?)
    }

    pub(super) fn current_mut(
        &mut self,
        session_id: Option<&str>,
    ) -> Option<&mut TerminalController> {
        self.controllers.get_mut(session_id?)
    }

    pub(super) fn status(&self, session_id: Option<&str>) -> Option<&TerminalStatus> {
        Some(self.current(session_id)?.status())
    }

    pub(super) fn remove(&mut self, session_id: &str) {
        self.controllers.remove(session_id);
    }

    pub(super) fn resize_all(&mut self, rows: u16, cols: u16) {
        for terminal in self.controllers.values_mut() {
            terminal.resize(rows, cols);
        }
    }

    pub(super) fn drain_events(&mut self, selected_session_id: Option<&str>) -> bool {
        let mut changed = false;
        for (session_id, terminal) in self.controllers.iter_mut() {
            let terminal_changed = terminal.drain_events();
            if terminal_changed && selected_session_id == Some(session_id.as_str()) {
                changed = true;
            }
        }
        changed
    }

    pub(super) fn restart_terminal_for_session(
        &mut self,
        project: &Project,
        session: &Session,
    ) -> Option<String> {
        if session.runtime.is_active() {
            return None;
        }

        let (session_id, target) = self.terminal_target_for_session(project, session);
        let proxy = self.proxy.clone();
        let restart_result: anyhow::Result<()> = (|| {
            let terminal = self
                .controllers
                .entry(session_id)
                .or_insert_with(|| TerminalController::new(proxy));
            let _ = terminal.attach(None)?;
            let _ = terminal.attach(Some(target))?;
            Ok(())
        })();
        restart_result
            .err()
            .map(|error| format!("terminal: {error}"))
    }

    pub(super) fn restart_idle_terminals_for_project(&mut self, project: &Project) -> Vec<String> {
        let session_ids = restartable_terminal_session_ids(project);
        let mut errors = Vec::new();

        for session_id in session_ids {
            let Some(terminal) = self.controllers.get_mut(&session_id) else {
                continue;
            };
            if let Err(error) = terminal.restart() {
                errors.push(format!("terminal: {error}"));
            }
        }

        errors
    }

    pub(super) fn sync(
        &mut self,
        projects: &[Project],
        selected_session_id: Option<&str>,
    ) -> Vec<String> {
        let mut active_ids = HashSet::new();
        let mut errors = Vec::new();
        let proxy = self.proxy.clone();

        for project in projects {
            for session in &project.sessions {
                let (session_id, target) = self.terminal_target_for_session(project, session);
                active_ids.insert(session_id.clone());

                let attach_result = {
                    let terminal = self
                        .controllers
                        .entry(session_id)
                        .or_insert_with(|| TerminalController::new(proxy.clone()));
                    terminal.attach(Some(target))
                };
                if let Err(error) = attach_result {
                    errors.push(format!("terminal: {error}"));
                }
            }
        }

        self.controllers
            .retain(|session_id, _| active_ids.contains(session_id));

        self.sync_selected_terminal_scroll(selected_session_id);
        errors
    }

    fn terminal_target_for_session(
        &self,
        project: &Project,
        session: &Session,
    ) -> (String, TerminalTarget) {
        (
            session.local_id.clone(),
            TerminalTarget {
                pi_binary: None,
                sidecar_extension_path: self.sidecar_extension_path.clone(),
                sidecar_socket_path: self.sidecar_socket_path.clone(),
                harness_session_id: session.local_id.clone(),
                cwd: project.path.clone(),
                session_file: session.session_file.clone(),
            },
        )
    }

    fn sync_selected_terminal_scroll(&mut self, selected_session_id: Option<&str>) {
        if self.last_selected_session_id.as_deref() == selected_session_id {
            return;
        }

        self.last_selected_session_id = selected_session_id.map(ToOwned::to_owned);
        if let Some(session_id) = selected_session_id {
            if let Some(terminal) = self.controllers.get_mut(session_id) {
                terminal.scroll_to_bottom();
            }
        }
    }
}

fn restartable_terminal_session_ids(project: &Project) -> Vec<String> {
    project
        .sessions
        .iter()
        .filter(|session| !session.runtime.is_active())
        .map(|session| session.local_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::restartable_terminal_session_ids;
    use crate::state::{Project, Session};

    #[test]
    fn queued_sessions_are_not_restartable() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));

        let idle = Session::new_draft();

        let mut queued = Session::new_draft();
        queued.runtime.queued = true;

        let mut running = Session::new_draft();
        running.runtime.running = true;

        let idle_id = idle.local_id.clone();
        let queued_id = queued.local_id.clone();
        let running_id = running.local_id.clone();
        project.sessions = vec![idle, queued, running];

        let restartable = restartable_terminal_session_ids(&project);
        assert_eq!(restartable, vec![idle_id]);
        assert!(!restartable.contains(&queued_id));
        assert!(!restartable.contains(&running_id));
    }
}
