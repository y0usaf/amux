//! pi's session-store contract: the directories pi owns and how project
//! paths encode to on-disk session folder names. harness-core's `agent`
//! machinery reads the shared JSONL format; everything pi-specific about
//! where that store lives is declared here.

use std::path::{Path, PathBuf};

/// pi relocates its whole agent dir with this env var.
pub const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
/// pi relocates its session root with this env var (flat dir, no project encoding).
pub const SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";

/// pi keeps its state under `$HOME/.pi/agent`.
pub const DEFAULT_AGENT_DIR_REL: &str = ".pi/agent";

/// Sidecar wire env vars, matching what the packaged pi extension reads.
pub const SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_PI_SIDECAR_SOCKET";
pub const SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_PI_SESSION_KEY";
pub const EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_PI_EXTENSION";
pub const ASCII_ENV: &str = "AGENT_HARNESS_PI_ASCII";

/// Prefix for the harness sidecar socket under the runtime dir.
pub const SOCKET_PREFIX: &str = "pi-sidecar";

pub fn default_agent_dir() -> Option<PathBuf> {
    Some(home_dir()?.join(DEFAULT_AGENT_DIR_REL))
}

/// Mirrors pi's on-disk layout: every project encodes as the absolute path
/// with separators replaced by dashes, wrapped in `--…--` (`/home/u/work/proj`
/// -> `--home-u-work-proj--`). Scanning must match it or every project
/// resolves to a dead directory and its sessions vanish from the sidebar.
pub fn encode_project_path(project_path: &Path) -> String {
    let normalized = crate::util::normalize_project_path(project_path);
    let mut encoded = String::from("--");
    for component in normalized.components() {
        use std::path::Component;
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => encoded.push_str("__parent__-"),
            Component::Normal(part) => {
                encoded.push_str(&part.to_string_lossy().replace(':', "_"));
                encoded.push('-');
            }
            Component::Prefix(prefix) => {
                encoded.push_str(&prefix.as_os_str().to_string_lossy().replace(':', "_"));
                encoded.push('-');
            }
        }
    }
    encoded.push('-');
    encoded
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
