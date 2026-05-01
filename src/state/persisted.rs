use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::{app_state_dir, normalize_project_path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub projects: Vec<String>,
    #[serde(default)]
    pub project_cache: Vec<PersistedProject>,
    pub selected_project: Option<String>,
    pub selected_session: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersistedProject {
    pub path: String,
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersistedSession {
    pub local_id: String,
    pub name: String,
    pub pi_session_id: Option<String>,
    pub session_file: Option<PathBuf>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub promoted_at_ms: u64,
    #[serde(default)]
    pub draft: bool,
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
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .or_else(|_| {
                fs::copy(&tmp, &path)?;
                fs::remove_file(&tmp)
            })
            .with_context(|| format!("writing {}", path.display()))?;
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

        if self.projects.is_empty() && !self.project_cache.is_empty() {
            self.projects = self
                .project_cache
                .iter()
                .map(|project| project.path.clone())
                .collect();
        }

        self.project_cache = normalized_project_cache(&self.projects, &self.project_cache);

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

fn normalized_project_cache(
    projects: &[String],
    cache: &[PersistedProject],
) -> Vec<PersistedProject> {
    projects
        .iter()
        .map(|path| {
            let normalized = normalize_project_path(&PathBuf::from(path))
                .to_string_lossy()
                .into_owned();
            let sessions = cache
                .iter()
                .find(|project| {
                    normalize_project_path(&PathBuf::from(&project.path)).to_string_lossy()
                        == normalized
                })
                .map(|project| project.sessions.clone())
                .unwrap_or_default();
            PersistedProject {
                path: normalized,
                sessions,
            }
        })
        .collect()
}

pub fn default_state_path() -> PathBuf {
    app_state_dir().join("state.json")
}
