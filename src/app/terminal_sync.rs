use std::collections::HashSet;

use crate::state::Project;
use crate::terminal::{TerminalController, TerminalStatus, TerminalTarget};

use super::App;

fn restartable_terminal_session_ids(project: &Project) -> Vec<String> {
    project
        .sessions
        .iter()
        .filter(|session| !session.runtime.is_active())
        .map(|session| session.local_id.clone())
        .collect()
}

impl App {
    fn terminal_target_for_session(
        &self,
        project_index: usize,
        session_index: usize,
    ) -> Option<(String, TerminalTarget)> {
        let project = self.projects.get(project_index)?;
        let session = project.sessions.get(session_index)?;
        Some((
            session.local_id.clone(),
            TerminalTarget {
                pi_binary: None,
                sidecar_extension_path: self.sidecar_extension_path.clone(),
                sidecar_socket_path: self.sidecar_socket_path.clone(),
                harness_session_id: session.local_id.clone(),
                cwd: project.path.clone(),
                session_file: session.session_file.clone(),
            },
        ))
    }

    pub(super) fn current_terminal(&self) -> Option<&TerminalController> {
        let session = self.current_session()?;
        self.terminals.get(&session.local_id)
    }

    pub(super) fn current_terminal_mut(&mut self) -> Option<&mut TerminalController> {
        let session_id = self.current_session()?.local_id.clone();
        self.terminals.get_mut(&session_id)
    }

    pub(super) fn current_terminal_status(&self) -> Option<&TerminalStatus> {
        Some(self.current_terminal()?.status())
    }

    pub(super) fn resize_terminals(&mut self, rows: u16, cols: u16) {
        for terminal in self.terminals.values_mut() {
            terminal.resize(rows, cols);
        }
    }

    pub(super) fn restart_terminal_for_session(
        &mut self,
        project_index: usize,
        session_index: usize,
    ) {
        if self
            .projects
            .get(project_index)
            .and_then(|project| project.sessions.get(session_index))
            .is_some_and(|session| session.runtime.is_active())
        {
            return;
        }

        let Some((session_id, target)) =
            self.terminal_target_for_session(project_index, session_index)
        else {
            return;
        };

        let proxy = self.proxy.clone();
        let restart_result: anyhow::Result<()> = (|| {
            let terminal = self
                .terminals
                .entry(session_id)
                .or_insert_with(|| TerminalController::new(proxy.clone()));
            let _ = terminal.attach(None)?;
            let _ = terminal.attach(Some(target))?;
            Ok(())
        })();
        if let Err(error) = restart_result {
            self.set_note(format!("terminal: {error}"));
        }
    }

    pub(super) fn restart_idle_terminals_for_project(&mut self, project_index: usize) {
        let Some(project) = self.projects.get(project_index) else {
            return;
        };

        let session_ids = restartable_terminal_session_ids(project);

        for session_id in session_ids {
            let Some(terminal) = self.terminals.get_mut(&session_id) else {
                continue;
            };
            if let Err(error) = terminal.restart() {
                self.set_note(format!("terminal: {error}"));
            }
        }
    }

    pub(super) fn sync_terminals(&mut self) {
        let mut active_ids = HashSet::new();
        let proxy = self.proxy.clone();

        for project_index in 0..self.projects.len() {
            let session_count = self.projects[project_index].sessions.len();
            for session_index in 0..session_count {
                let Some((session_id, target)) =
                    self.terminal_target_for_session(project_index, session_index)
                else {
                    continue;
                };
                active_ids.insert(session_id.clone());

                let attach_result = {
                    let terminal = self
                        .terminals
                        .entry(session_id)
                        .or_insert_with(|| TerminalController::new(proxy.clone()));
                    terminal.attach(Some(target))
                };
                if let Err(error) = attach_result {
                    self.set_note(format!("terminal: {error}"));
                }
            }
        }

        let stale_ids: Vec<_> = self
            .terminals
            .keys()
            .filter(|session_id| !active_ids.contains(*session_id))
            .cloned()
            .collect();
        for session_id in stale_ids {
            self.terminals.remove(&session_id);
        }

        self.update_window_title();
    }
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
