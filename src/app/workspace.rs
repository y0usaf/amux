use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::pi;
use crate::state::{merge_scanned_sessions, PersistedState, Project, Session};
use crate::util::{normalize_project_path, project_name_from_path};

use super::selection::{
    ephemeral_draft_session_index, next_selectable_session_index, preferred_session_index,
    session_index_for_restore_key, visible_session_indices,
};

#[derive(Clone, Debug)]
pub(super) struct SessionArchiveTarget {
    pub(super) project_index: usize,
    pub(super) session_index: usize,
    pub(super) session_id: String,
    pub(super) session_file: Option<PathBuf>,
}

pub(super) struct Workspace {
    initial_project_paths: Vec<PathBuf>,
    persisted: PersistedState,
    projects: Vec<Project>,
    selected_project: usize,
    selected_session: Option<usize>,
}

impl Workspace {
    pub(super) fn new(initial_project_paths: Vec<PathBuf>, persisted: PersistedState) -> Self {
        Self {
            initial_project_paths,
            persisted,
            projects: Vec::new(),
            selected_project: 0,
            selected_session: None,
        }
    }

    pub(super) fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub(super) fn projects_mut(&mut self) -> &mut [Project] {
        &mut self.projects
    }

    pub(super) fn selected_project_index(&self) -> usize {
        self.selected_project
    }

    pub(super) fn selected_session_index(&self) -> Option<usize> {
        self.selected_session
    }

    pub(super) fn current_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    pub(super) fn current_session(&self) -> Option<&Session> {
        let index = self.selected_session?;
        self.current_project()?.sessions.get(index)
    }

    pub(super) fn current_session_visible_in_sidebar(&self) -> bool {
        self.current_session()
            .is_some_and(Session::should_render_in_sidebar)
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

    pub(super) fn reload_projects_from_disk(&mut self) {
        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());

        let opened_project_paths =
            normalize_unique_project_paths(std::mem::take(&mut self.initial_project_paths));
        let opened_project_key = opened_project_paths
            .first()
            .map(|path| path.to_string_lossy().into_owned());

        let mut project_paths: Vec<PathBuf> = if !self.persisted.projects.is_empty() {
            self.persisted.projects.iter().map(PathBuf::from).collect()
        } else if opened_project_paths.is_empty() {
            vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
        } else {
            Vec::new()
        };
        project_paths.extend(opened_project_paths);
        project_paths = normalize_unique_project_paths(project_paths);

        let mut next_projects = Vec::with_capacity(project_paths.len());
        for path in project_paths {
            let mut project = self
                .projects
                .iter()
                .find(|project| project.path == path)
                .cloned()
                .unwrap_or_else(|| Project::new(path.clone()));
            project.path = path.clone();
            project.name = project_name_from_path(&path);
            merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&path));
            project.sort_sessions();
            next_projects.push(project);
        }
        self.projects = next_projects;

        self.restore_selection(
            opened_project_key.or(selected_project_key),
            selected_session_key,
        );
        self.persist_selection();
    }

    pub(super) fn restore_selection(
        &mut self,
        project_key: Option<String>,
        session_key: Option<String>,
    ) {
        if self.projects.is_empty() {
            self.selected_project = 0;
            self.selected_session = None;
            return;
        }

        let persisted_project = project_key.or_else(|| self.persisted.selected_project.clone());
        self.selected_project = persisted_project
            .as_deref()
            .and_then(|key| {
                self.projects
                    .iter()
                    .position(|project| project.selection_key() == key)
            })
            .unwrap_or(0);

        let desired_session = session_key.or_else(|| self.persisted.selected_session.clone());
        self.selected_session = desired_session.as_deref().and_then(|key| {
            session_index_for_restore_key(&self.projects[self.selected_project].sessions, key)
        });

        if self.selected_session.is_none() {
            self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        }
    }

    pub(super) fn add_project(&mut self, path: &Path) -> Result<(), String> {
        let path = normalize_project_path(path);
        if !path.exists() || !path.is_dir() {
            return Err(format!("invalid project path: {}", path.display()));
        }
        if let Some(project_index) = self
            .projects
            .iter()
            .position(|project| project.path == path)
        {
            self.select_project(project_index);
            return Ok(());
        }

        let mut project = Project::new(path.clone());
        merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&path));
        project.sort_sessions();
        self.projects.push(project);
        self.selected_project = self.projects.len() - 1;
        self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        self.persist_selection();
        Ok(())
    }

    pub(super) fn promote_project_to_front(&mut self, project_index: usize) {
        if project_index == 0 || project_index >= self.projects.len() {
            return;
        }

        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());
        let project = self.projects.remove(project_index);
        self.projects.insert(0, project);
        self.restore_selection(selected_project_key, selected_session_key);
    }

    pub(super) fn remove_selected_project(&mut self) -> bool {
        if self.projects.is_empty() {
            return false;
        }
        self.projects.remove(self.selected_project);
        if self.projects.is_empty() {
            self.selected_project = 0;
            self.selected_session = None;
        } else {
            self.selected_project = self.selected_project.min(self.projects.len() - 1);
            self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        }
        self.persist_selection();
        true
    }

    pub(super) fn refresh_project_from_scan(&mut self, project_index: usize) {
        let Some(project_path) = self
            .projects
            .get(project_index)
            .map(|project| project.path.clone())
        else {
            return;
        };

        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());

        if let Some(project) = self.projects.get_mut(project_index) {
            merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&project_path));
            project.sort_sessions();
        }

        self.restore_selection(selected_project_key, selected_session_key);
        self.persist_selection();
    }

    pub(super) fn archive_target(&self) -> Result<SessionArchiveTarget, &'static str> {
        let Some(project) = self.current_project() else {
            return Err("no session selected");
        };
        let Some(session_index) = self.selected_session else {
            return Err("no session selected");
        };
        let Some(session) = project.sessions.get(session_index) else {
            return Err("no session selected");
        };

        Ok(SessionArchiveTarget {
            project_index: self.selected_project,
            session_index,
            session_id: session.local_id.clone(),
            session_file: session.session_file.clone(),
        })
    }

    pub(super) fn remove_archived_session(&mut self, target: &SessionArchiveTarget) {
        let Some(project) = self.projects.get_mut(target.project_index) else {
            return;
        };
        if target.session_index >= project.sessions.len() {
            return;
        }

        project.sessions.remove(target.session_index);
        self.selected_project = target
            .project_index
            .min(self.projects.len().saturating_sub(1));
        self.selected_session = self
            .projects
            .get(target.project_index)
            .and_then(|project| next_selectable_session_index(project, target.session_index))
            .or_else(|| self.ensure_default_session_for_project(target.project_index));
        self.persist_selection();
    }

    pub(super) fn new_session(&mut self) {
        let project_index = self.selected_project;
        let session_index = match self
            .projects
            .get(project_index)
            .and_then(ephemeral_draft_session_index)
        {
            Some(index) => index,
            None => {
                let Some(project) = self.projects.get_mut(project_index) else {
                    return;
                };
                project.sessions.push(Session::new_draft());
                project.sessions.len() - 1
            }
        };

        self.selected_session = Some(session_index);
        self.persist_selection();
    }

    pub(super) fn select_project(&mut self, index: usize) -> bool {
        if index >= self.projects.len() {
            return false;
        }
        self.selected_project = index;
        self.selected_session = self.ensure_default_session_for_project(index);
        self.persist_selection();
        true
    }

    pub(super) fn select_session(&mut self, index: usize) -> bool {
        let project_index = self.selected_project;
        self.select_session_in_project(project_index, index)
    }

    pub(super) fn select_session_in_project(
        &mut self,
        project_index: usize,
        session_index: usize,
    ) -> bool {
        let Some(project) = self.projects.get(project_index) else {
            return false;
        };
        if session_index >= project.sessions.len() {
            return false;
        }
        self.selected_project = project_index;
        self.selected_session = Some(session_index);
        self.persist_selection();
        true
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) -> bool {
        if self.projects.is_empty() {
            return false;
        }
        let len = self.projects.len() as i32;
        let next = (self.selected_project as i32 + delta).rem_euclid(len) as usize;
        self.select_project(next)
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) -> bool {
        let Some(project) = self.current_project() else {
            return false;
        };
        let indices = cycle_session_indices(project);
        if indices.is_empty() {
            return false;
        }

        let next = match self
            .selected_session
            .and_then(|current| indices.iter().position(|index| *index == current))
        {
            Some(position) => {
                let len = indices.len() as i32;
                let next = (position as i32 + delta).rem_euclid(len) as usize;
                indices[next]
            }
            None if delta < 0 => indices[indices.len() - 1],
            None => indices[0],
        };
        self.select_session(next)
    }

    pub(super) fn refresh_current_session_project_index(&self) -> Result<usize, &'static str> {
        let Some(session) = self.current_session() else {
            return Err("no session selected");
        };
        if !session_is_idle(session) {
            return Err("cannot refresh active session");
        }
        Ok(self.selected_project)
    }

    pub(super) fn selected_terminal_restart_target(&self) -> Option<(usize, usize)> {
        Some((self.selected_project, self.selected_session?))
    }

    fn ensure_default_session_for_project(&mut self, project_index: usize) -> Option<usize> {
        let project = self.projects.get_mut(project_index)?;
        if let Some(index) = preferred_session_index(project) {
            return Some(index);
        }

        project.sessions.push(Session::new_draft());
        Some(project.sessions.len() - 1)
    }
}

pub(super) fn normalize_unique_project_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::with_capacity(paths.len());

    for path in paths.into_iter().map(|path| normalize_project_path(&path)) {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }

    unique
}

pub(super) fn cycle_session_indices(project: &Project) -> Vec<usize> {
    let visible = visible_session_indices(project);
    if visible.is_empty() {
        (0..project.sessions.len()).collect()
    } else {
        visible
    }
}

pub(super) fn session_is_idle(session: &Session) -> bool {
    !session.runtime.is_active()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{cycle_session_indices, session_is_idle, Workspace};
    use crate::state::{PersistedState, Project, Session};

    #[test]
    fn queued_sessions_are_not_idle() {
        let mut session = Session::new_draft();
        assert!(session_is_idle(&session));

        session.runtime.queued = true;
        assert!(!session_is_idle(&session));
    }

    #[test]
    fn cycle_session_indices_skip_hidden_drafts_when_visible_sessions_exist() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let hidden = Session::new_draft();
        let mut visible = Session::new_draft();
        visible.pi_session_id = Some("pi-session-1".into());
        visible.draft = false;
        project.sessions = vec![hidden, visible];

        assert_eq!(cycle_session_indices(&project), vec![1]);
    }

    #[test]
    fn cycle_session_indices_fall_back_to_hidden_draft_when_it_is_all_we_have() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        project.sessions = vec![Session::new_draft()];

        assert_eq!(cycle_session_indices(&project), vec![0]);
    }

    #[test]
    fn archive_target_allows_active_sessions() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));
        let mut session = Session::new_draft();
        session.local_id = "local-session-1".into();
        session.runtime.running = true;
        project.sessions = vec![session];

        let mut workspace = Workspace::new(Vec::new(), PersistedState::default());
        workspace.projects = vec![project];
        workspace.selected_project = 0;
        workspace.selected_session = Some(0);

        let target = workspace.archive_target().unwrap();

        assert_eq!(target.session_id, "local-session-1");
    }
}
