use crate::terminal::{TerminalController, TerminalStatus};

use super::App;

impl App {
    pub(super) fn current_terminal(&self) -> Option<&TerminalController> {
        self.terminal_manager.current(
            self.current_session()
                .map(|session| session.local_id.as_str()),
        )
    }

    pub(super) fn current_terminal_mut(&mut self) -> Option<&mut TerminalController> {
        let session_id = self.current_session()?.local_id.clone();
        self.terminal_manager.current_mut(Some(&session_id))
    }

    pub(super) fn current_terminal_status(&self) -> Option<&TerminalStatus> {
        self.terminal_manager.status(
            self.current_session()
                .map(|session| session.local_id.as_str()),
        )
    }

    pub(super) fn resize_terminals(&mut self, rows: u16, cols: u16) {
        self.terminal_manager.resize_all(rows, cols);
    }

    pub(super) fn remove_terminal_for_session_id(&mut self, session_id: &str) {
        self.terminal_manager.remove(session_id);
    }

    pub(super) fn restart_terminal_for_session(
        &mut self,
        project_index: usize,
        session_index: usize,
    ) {
        let Some(project) = self.workspace.projects().get(project_index).cloned() else {
            return;
        };
        let Some(session) = project.sessions.get(session_index).cloned() else {
            return;
        };

        if let Some(error) = self
            .terminal_manager
            .restart_terminal_for_session(&project, &session)
        {
            self.set_note(error);
        }
    }

    pub(super) fn restart_idle_terminals_for_project(&mut self, project_index: usize) {
        let Some(project) = self.workspace.projects().get(project_index).cloned() else {
            return;
        };

        for error in self
            .terminal_manager
            .restart_idle_terminals_for_project(&project)
        {
            self.set_note(error);
        }
    }

    pub(super) fn sync_terminals(&mut self) {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        let errors = self
            .terminal_manager
            .sync(self.workspace.projects(), selected_session_id.as_deref());
        for error in errors {
            self.set_note(error);
        }
        self.update_window_title();
    }

    pub(super) fn drain_terminal_events(&mut self) -> bool {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        self.terminal_manager
            .drain_events(selected_session_id.as_deref())
    }
}
