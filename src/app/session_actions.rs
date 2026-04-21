use crate::pi;
use crate::state::Session;

use super::App;

impl App {
    pub(super) fn archive_selected_session(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };
        let Some(session_index) = self.selected_session else {
            self.set_note("no session selected");
            return;
        };
        let Some(session) = project.sessions.get(session_index) else {
            return;
        };

        if session.runtime.running {
            self.set_note("cannot archive running session");
            return;
        }

        if let Some(path) = session.session_file.as_ref() {
            if let Err(error) = pi::archive_session_file(path) {
                self.set_note(error);
                return;
            }
        }

        let project_index = self.selected_project;
        let session_id = session.local_id.clone();
        let next_selected_session = {
            let project = &mut self.projects[project_index];
            project.sessions.remove(session_index);
            if project.sessions.is_empty() {
                None
            } else {
                Some(session_index.min(project.sessions.len() - 1))
            }
        };
        self.terminals.remove(&session_id);
        self.selected_session = next_selected_session
            .or_else(|| self.ensure_default_session_for_project(project_index));
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn new_session(&mut self) {
        let _ = self.prepare_selection_change();
        let Some(project) = self.current_project_mut() else {
            return;
        };
        project.sessions.insert(0, Session::new_draft());
        self.selected_session = Some(0);
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn select_project(&mut self, index: usize) {
        if index >= self.projects.len() {
            return;
        }
        let _ = self.prepare_selection_change();
        self.selected_project = index;
        self.selected_session = if self.projects[index].sessions.is_empty() {
            self.ensure_default_session_for_project(index)
        } else {
            Some(0)
        };
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn select_session(&mut self, index: usize) {
        let project_index = self.selected_project;
        self.select_session_in_project(project_index, index);
    }

    pub(super) fn select_session_in_project(&mut self, project_index: usize, session_index: usize) {
        let Some(project) = self.projects.get(project_index) else {
            return;
        };
        if session_index >= project.sessions.len() {
            return;
        }
        let removed = self.prepare_selection_change();
        let session_index =
            Self::adjust_session_index_after_removal(project_index, session_index, removed);
        let Some(project) = self.projects.get(project_index) else {
            return;
        };
        if session_index >= project.sessions.len() {
            return;
        }
        self.selected_project = project_index;
        self.selected_session = Some(session_index);
        if let Some(session) = self.current_session_mut() {
            session.runtime.unread = false;
        }
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) {
        if self.projects.is_empty() {
            return;
        }
        let len = self.projects.len() as i32;
        let next = (self.selected_project as i32 + delta).rem_euclid(len) as usize;
        self.select_project(next);
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) {
        let Some(project) = self.current_project() else {
            return;
        };
        if project.sessions.is_empty() {
            return;
        }
        let current = self.selected_session.unwrap_or(0) as i32;
        let len = project.sessions.len() as i32;
        let next = (current + delta).rem_euclid(len) as usize;
        self.select_session(next);
    }

    pub(super) fn refresh_current_session(&mut self) {
        if self.selected_session.is_none() {
            self.set_note("no session selected");
            return;
        }

        if self
            .current_session()
            .is_some_and(|session| session.runtime.running)
        {
            self.set_note("cannot refresh running session");
            return;
        }

        let project_index = self.selected_project;
        self.refresh_project_from_scan(project_index);
        self.sync_terminals();

        if let Some(session_index) = self.selected_session {
            self.restart_terminal_for_session(self.selected_project, session_index);
        }
    }

    pub(super) fn refresh_all_sessions(&mut self) {
        self.reload_projects_from_disk();
        self.sync_terminals();
        for project_index in 0..self.projects.len() {
            self.restart_idle_terminals_for_project(project_index);
        }
    }
}
