use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::{app_state_dir, normalize_project_path};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedState {
    pub projects: Vec<String>,
    #[serde(default)]
    pub project_cache: Vec<PersistedProject>,
    pub selected_project: Option<String>,
    pub selected_session: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedProject {
    pub path: String,
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSession {
    pub local_id: String,
    pub name: String,
    pub pi_session_id: Option<String>,
    pub session_file: Option<PathBuf>,
    #[serde(default)]
    pub parent_session_file: Option<PathBuf>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub promoted_at_ms: u64,
    #[serde(default)]
    pub draft: bool,
}

enum SaveCommand {
    Save {
        path: PathBuf,
        state: PersistedState,
    },
    Flush(Sender<()>),
}

static SAVE_TX: OnceLock<Sender<SaveCommand>> = OnceLock::new();

impl PersistedState {
    pub fn load_default() -> anyhow::Result<Self> {
        flush_default_save_queue();
        Self::load_from_path(&default_state_path())
    }

    pub fn save_default(&self) -> anyhow::Result<()> {
        self.save_to_path(&default_state_path())
    }

    pub fn enqueue_default_save(&self) {
        let _ = save_queue().send(SaveCommand::Save {
            path: default_state_path(),
            state: self.clone(),
        });
    }

    pub fn flush_default_save_queue() {
        flush_default_save_queue();
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

    fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut state: Self = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        state.normalize();
        Ok(state)
    }

    fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut copy = self.clone();
        copy.normalize();
        let content = serde_json::to_string_pretty(&copy)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .or_else(|_| {
                fs::copy(&tmp, path)?;
                fs::remove_file(&tmp)
            })
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

fn normalized_project_cache(
    projects: &[String],
    cache: &[PersistedProject],
) -> Vec<PersistedProject> {
    let mut cache_by_path = HashMap::with_capacity(cache.len());
    for project in cache {
        let normalized = normalize_project_path(&PathBuf::from(&project.path))
            .to_string_lossy()
            .into_owned();
        cache_by_path
            .entry(normalized)
            .or_insert_with(|| project.sessions.clone());
    }

    projects
        .iter()
        .map(|path| {
            let normalized = normalize_project_path(&PathBuf::from(path))
                .to_string_lossy()
                .into_owned();
            let sessions = cache_by_path.get(&normalized).cloned().unwrap_or_default();
            PersistedProject {
                path: normalized,
                sessions,
            }
        })
        .collect()
}

fn save_queue() -> &'static Sender<SaveCommand> {
    SAVE_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = thread::Builder::new()
            .name("pi-harness-state-save".into())
            .spawn(move || {
                let mut pending: Option<(PathBuf, PersistedState)> = None;
                let mut deadline: Option<Instant> = None;

                loop {
                    match deadline {
                        Some(next_deadline) => match rx
                            .recv_timeout(next_deadline.saturating_duration_since(Instant::now()))
                        {
                            Ok(SaveCommand::Save { path, state }) => {
                                pending = Some((path, state));
                                deadline = Some(Instant::now() + SAVE_DEBOUNCE);
                            }
                            Ok(SaveCommand::Flush(done)) => {
                                if let Some((path, state)) = pending.take() {
                                    let _ = state.save_to_path(&path);
                                }
                                deadline = None;
                                let _ = done.send(());
                            }
                            Err(RecvTimeoutError::Timeout) => {
                                if let Some((path, state)) = pending.take() {
                                    let _ = state.save_to_path(&path);
                                }
                                deadline = None;
                            }
                            Err(RecvTimeoutError::Disconnected) => {
                                if let Some((path, state)) = pending.take() {
                                    let _ = state.save_to_path(&path);
                                }
                                break;
                            }
                        },
                        None => match rx.recv() {
                            Ok(SaveCommand::Save { path, state }) => {
                                pending = Some((path, state));
                                deadline = Some(Instant::now() + SAVE_DEBOUNCE);
                            }
                            Ok(SaveCommand::Flush(done)) => {
                                if let Some((path, state)) = pending.take() {
                                    let _ = state.save_to_path(&path);
                                }
                                let _ = done.send(());
                            }
                            Err(_) => break,
                        },
                    }
                }
            });
        tx
    })
}

fn flush_default_save_queue() {
    let (tx, rx) = mpsc::channel();
    if save_queue().send(SaveCommand::Flush(tx)).is_ok() {
        let _ = rx.recv();
    }
}

pub fn default_state_path() -> PathBuf {
    app_state_dir().join("state.json")
}
