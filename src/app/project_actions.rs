use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pi;
use crate::state::{merge_scanned_sessions, Project};
use crate::util::{normalize_project_path, project_name_from_path};

use super::App;

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

impl App {
    pub(super) fn reload_projects_from_disk(&mut self) {
        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());

        let mut project_paths: Vec<PathBuf> = if !self.persisted.projects.is_empty() {
            self.persisted.projects.iter().map(PathBuf::from).collect()
        } else if !self.initial_project_paths.is_empty() {
            self.initial_project_paths.clone()
        } else {
            vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
        };

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

        self.restore_selection(selected_project_key, selected_session_key);
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
            self.sync_sidebar_to_selection();
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
            self.projects[self.selected_project]
                .sessions
                .iter()
                .position(|session| session.selection_key() == key || session.local_id == key)
        });

        if self.selected_session.is_none() {
            self.selected_session = if self.projects[self.selected_project].sessions.is_empty() {
                self.ensure_default_session_for_project(self.selected_project)
            } else {
                Some(0)
            };
        }
        self.sync_sidebar_to_selection();
    }

    pub(super) fn open_project_picker(&mut self) {
        let start_dir = self
            .current_project()
            .map(|project| project.path.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        match pick_project_directory(&start_dir) {
            Ok(Some(path)) => self.add_project(&path),
            Ok(None) | Err(_) => {}
        }
    }

    pub(super) fn add_project(&mut self, path: &Path) {
        let path = normalize_project_path(path);
        if !path.exists() || !path.is_dir() {
            self.set_note(format!("invalid project path: {}", path.display()));
            return;
        }
        if self.projects.iter().any(|project| project.path == path) {
            self.set_note("project already added");
            return;
        }

        let _ = self.prepare_selection_change();
        let mut project = Project::new(path.clone());
        merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&path));
        project.sort_sessions();
        self.projects.push(project);
        self.selected_project = self.projects.len().saturating_sub(1);
        self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
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

    pub(super) fn remove_selected_project(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        self.projects.remove(self.selected_project);
        if self.projects.is_empty() {
            self.selected_project = 0;
            self.selected_session = None;
        } else {
            self.selected_project = self.selected_project.min(self.projects.len() - 1);
            self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        }
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
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
}

fn pick_project_directory(start_dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut filename_arg = OsString::from("--filename=");
    filename_arg.push(start_dir.as_os_str());
    if !start_dir.as_os_str().is_empty() && !start_dir.to_string_lossy().ends_with('/') {
        filename_arg.push("/");
    }

    let output = Command::new("zenity")
        .arg("--file-selection")
        .arg("--directory")
        .arg("--title=Open project")
        .arg(filename_arg)
        .output()
        .map_err(|error| format!("spawn zenity: {error}"))?;

    match output.status.code() {
        Some(0) => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(path)))
            }
        }
        Some(1) => Ok(None),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                Err(format!("zenity exited with {}", output.status))
            } else {
                Err(stderr)
            }
        }
    }
}
