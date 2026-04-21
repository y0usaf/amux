use std::path::PathBuf;

use crate::state::{compare_sessions, Session};
use crate::util::{normalize_project_path, project_name_from_path};

#[derive(Clone, Debug)]
pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub sessions: Vec<Session>,
}

impl Project {
    pub fn new(path: PathBuf) -> Self {
        let path = normalize_project_path(&path);
        Self {
            name: project_name_from_path(&path),
            path,
            sessions: Vec::new(),
        }
    }

    pub fn selection_key(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    pub fn sort_sessions(&mut self) {
        self.sessions.sort_by(compare_sessions);
    }
}
