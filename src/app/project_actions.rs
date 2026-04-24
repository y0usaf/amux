use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::App;

#[cfg(test)]
pub(super) fn normalize_unique_project_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    super::workspace::normalize_unique_project_paths(paths)
}

impl App {
    pub(super) fn reload_projects_from_disk(&mut self) {
        self.workspace.reload_projects_from_disk();
        self.sync_sidebar_to_selection();
    }

    pub(super) fn restore_selection(
        &mut self,
        project_key: Option<String>,
        session_key: Option<String>,
    ) {
        self.workspace.restore_selection(project_key, session_key);
        self.sync_sidebar_to_selection();
    }

    pub(super) fn open_project_picker(&mut self) {
        let start_dir = self
            .current_project()
            .map(|project| project.path.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        if let Ok(Some(path)) = pick_project_directory(&start_dir) {
            self.add_project(&path);
        }
    }

    pub(super) fn add_project(&mut self, path: &Path) {
        match self.workspace.add_project(path) {
            Ok(()) => {
                self.sync_sidebar_to_selection();
                self.sync_terminals();
            }
            Err(error) => self.set_note(error),
        }
    }

    pub(super) fn promote_project_to_front(&mut self, project_index: usize) {
        self.workspace.promote_project_to_front(project_index);
        self.sync_sidebar_to_selection();
    }

    pub(super) fn remove_selected_project(&mut self) {
        if self.workspace.remove_selected_project() {
            self.sync_sidebar_to_selection();
            self.sync_terminals();
        }
    }

    pub(super) fn refresh_project_from_scan(&mut self, project_index: usize) {
        self.workspace.refresh_project_from_scan(project_index);
        self.sync_sidebar_to_selection();
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
