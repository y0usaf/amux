use crate::state::{Project, Session};

use super::App;

impl App {
    pub(super) fn current_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    pub(super) fn current_project_mut(&mut self) -> Option<&mut Project> {
        self.projects.get_mut(self.selected_project)
    }

    pub(super) fn current_session(&self) -> Option<&Session> {
        let index = self.selected_session?;
        self.current_project()?.sessions.get(index)
    }

    pub(super) fn current_session_mut(&mut self) -> Option<&mut Session> {
        let index = self.selected_session?;
        self.current_project_mut()?.sessions.get_mut(index)
    }

    pub(super) fn current_session_visible_in_sidebar(&self) -> bool {
        self.current_session()
            .is_some_and(Session::should_render_in_sidebar)
    }

    pub(super) fn ensure_default_session_for_project(
        &mut self,
        project_index: usize,
    ) -> Option<usize> {
        let project = self.projects.get_mut(project_index)?;
        if project.sessions.is_empty() {
            project.sessions.push(Session::new_draft());
        }
        Some(0)
    }

    pub(super) fn prepare_selection_change(&mut self) -> Option<(usize, usize)> {
        self.discard_selected_ephemeral_session()
    }

    fn discard_selected_ephemeral_session(&mut self) -> Option<(usize, usize)> {
        let project_index = self.selected_project;
        let session_index = self.selected_session?;
        let session_id = {
            let session = self
                .projects
                .get(project_index)?
                .sessions
                .get(session_index)?;
            if !session.is_ephemeral_draft() {
                return None;
            }
            session.local_id.clone()
        };

        self.projects
            .get_mut(project_index)?
            .sessions
            .remove(session_index);
        self.selected_session = None;
        self.terminals.remove(&session_id);
        Some((project_index, session_index))
    }

    pub(super) fn adjust_session_index_after_removal(
        project_index: usize,
        session_index: usize,
        removed: Option<(usize, usize)>,
    ) -> usize {
        match removed {
            Some((removed_project, removed_session))
                if removed_project == project_index && removed_session < session_index =>
            {
                session_index - 1
            }
            _ => session_index,
        }
    }

    pub(super) fn persist_selection(&mut self) {
        self.persisted.projects = self
            .projects
            .iter()
            .map(|project| project.selection_key())
            .collect();
        self.persisted.selected_project = self
            .current_project()
            .map(|project| project.selection_key());
        self.persisted.selected_session = self
            .current_session()
            .and_then(Session::persisted_selection_key);
        let _ = self.persisted.save_default();
    }
}
