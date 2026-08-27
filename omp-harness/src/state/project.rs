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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn new_project_normalizes_path_and_derives_name() {
        let project = Project::new(PathBuf::from("src/../src"));

        assert_eq!(
            project.path,
            normalize_project_path(Path::new("src/../src"))
        );
        assert_eq!(project.name, project_name_from_path(&project.path));
        assert!(project.sessions.is_empty());
    }

    #[test]
    fn selection_key_matches_normalized_project_path() {
        let project = Project::new(PathBuf::from("src/../src"));
        assert_eq!(project.selection_key(), project.path.to_string_lossy());
    }

    #[test]
    fn sort_sessions_orders_promoted_then_recent_then_name() {
        let mut project = Project::new(PathBuf::from("/tmp/project"));

        let mut alpha = Session::new_draft();
        alpha.name = "alpha".into();
        alpha.updated_at_ms = 10;

        let mut zeta = Session::new_draft();
        zeta.name = "zeta".into();
        zeta.updated_at_ms = 10;

        let mut recent = Session::new_draft();
        recent.name = "recent".into();
        recent.updated_at_ms = 20;

        let mut promoted = Session::new_draft();
        promoted.name = "promoted".into();
        promoted.updated_at_ms = 5;
        promoted.promoted_at_ms = 30;

        project.sessions = vec![zeta, promoted, alpha, recent];
        project.sort_sessions();

        let names: Vec<_> = project
            .sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect();
        assert_eq!(names, vec!["promoted", "recent", "alpha", "zeta"]);
    }
}
