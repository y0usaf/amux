use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::{app_state_dir, normalize_project_path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub projects: Vec<String>,
    pub selected_project: Option<String>,
    pub selected_session: Option<String>,
}

impl PersistedState {
    pub fn load_default() -> anyhow::Result<Self> {
        let path = default_state_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut state: Self = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        state.normalize();
        Ok(state)
    }

    pub fn save_default(&self) -> anyhow::Result<()> {
        let path = default_state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut copy = self.clone();
        copy.normalize();
        let content = serde_json::to_string_pretty(&copy)?;
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn normalize(&mut self) {
        let mut seen = HashSet::new();
        self.projects = self
            .projects
            .iter()
            .map(PathBuf::from)
            .map(|path| normalize_project_path(&path).to_string_lossy().into_owned())
            .filter(|path| seen.insert(path.clone()))
            .collect();

        if let Some(selected_project) = self.selected_project.as_ref() {
            let normalized = normalize_project_path(&PathBuf::from(selected_project))
                .to_string_lossy()
                .into_owned();
            self.selected_project = self
                .projects
                .iter()
                .any(|project| project == &normalized)
                .then_some(normalized);
        }
    }
}

pub fn default_state_path() -> PathBuf {
    app_state_dir().join("state.json")
}
