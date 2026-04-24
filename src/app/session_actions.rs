use crate::pi;

use super::App;

impl App {
    pub(super) fn archive_selected_session(&mut self) {
        let target = match self.workspace.archive_target() {
            Ok(target) => target,
            Err(note) => {
                self.set_note(note);
                return;
            }
        };

        if let Some(path) = target.session_file.as_ref() {
            if let Err(error) = pi::archive_session_file(path) {
                self.set_note(error);
                return;
            }
        }

        self.workspace.remove_archived_session(&target);
        self.remove_terminal_for_session_id(&target.session_id);
        self.sync_sidebar_to_selection();
        self.sync_terminals();
    }

    pub(super) fn new_session(&mut self) {
        self.workspace.new_session();
        self.sync_sidebar_to_selection();
        self.sync_terminals();
    }

    pub(super) fn select_project(&mut self, index: usize) {
        if self.workspace.select_project(index) {
            self.sync_sidebar_to_selection();
            self.sync_terminals();
        }
    }

    pub(super) fn select_session_in_project(&mut self, project_index: usize, session_index: usize) {
        if self
            .workspace
            .select_session_in_project(project_index, session_index)
        {
            self.sync_sidebar_to_selection();
            self.sync_terminals();
        }
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) {
        if self.workspace.cycle_projects(delta) {
            self.sync_sidebar_to_selection();
            self.sync_terminals();
        }
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) {
        if self.workspace.cycle_sessions(delta) {
            self.sync_sidebar_to_selection();
            self.sync_terminals();
        }
    }

    pub(super) fn refresh_current_session(&mut self) {
        let project_index = match self.workspace.refresh_current_session_project_index() {
            Ok(project_index) => project_index,
            Err(note) => {
                self.set_note(note);
                return;
            }
        };

        self.refresh_project_from_scan(project_index);
        self.sync_terminals();

        if let Some((project_index, session_index)) =
            self.workspace.selected_terminal_restart_target()
        {
            self.restart_terminal_for_session(project_index, session_index);
        }
    }

    pub(super) fn refresh_all_sessions(&mut self) {
        self.reload_projects_from_disk();
        self.sync_terminals();
        for project_index in 0..self.workspace.projects().len() {
            self.restart_idle_terminals_for_project(project_index);
        }
    }
}
