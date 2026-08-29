//! fx layout for the harness session-store operations: live root, archive
//! dir, and the harness sidecar socket path. Unlike pi/omp there is no
//! project-path encoding: `live_project_dir` returns the single flat root
//! and the scan filters by `workspace_root`.

use std::path::{Path, PathBuf};

use super::store::{default_agent_dir, SESSION_DIR_ENV};

pub(crate) const SESSIONS_DIR_NAME: &str = "sessions";
pub(crate) const ARCHIVE_DIR_NAME: &str = "archive";

pub fn socket_path() -> PathBuf {
    crate::util::app_runtime_dir().join(format!(
        "{}-{}.sock",
        super::store::SOCKET_PREFIX,
        std::process::id()
    ))
}

pub(crate) fn sessions_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(SESSION_DIR_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(value));
    }
    Some(default_agent_dir()?.join(SESSIONS_DIR_NAME))
}

fn archive_dir() -> Option<PathBuf> {
    Some(default_agent_dir()?.join(ARCHIVE_DIR_NAME))
}

/// All fx sessions live in one flat root; project filtering happens in the
/// scan by comparing `session.json.workspace_root`.
pub fn live_project_dir(_project_path: &Path) -> Option<PathBuf> {
    sessions_root()
}

pub fn archive_session_file(source: &Path) -> Result<(), String> {
    let Some(dest_dir) = archive_dir() else {
        return Err(format!(
            "cannot archive session {}: fx archive dir unavailable",
            source.display()
        ));
    };
    move_dir_into_dir_with_fallback(source, &dest_dir)
}

pub fn restore_session_file(source: &Path, _project_path: &Path) -> Result<(), String> {
    let Some(dest_dir) = sessions_root() else {
        return Err("cannot restore session: fx sessions root unavailable".to_string());
    };
    move_dir_into_dir_with_fallback(source, &dest_dir)
}

fn move_dir_into_dir_with_fallback(source: &Path, dest_dir: &Path) -> Result<(), String> {
    let dir_name = source
        .file_name()
        .ok_or_else(|| format!("cannot move {}: missing directory name", source.display()))?;
    std::fs::create_dir_all(dest_dir)
        .map_err(|error| format!("cannot create {}: {error}", dest_dir.display()))?;

    let dest = dest_dir.join(dir_name);
    if !source.exists() {
        return Err(format!("cannot move {}: source missing", source.display()));
    }
    if dest.exists() {
        return Err(format!(
            "cannot move {} -> {}: destination exists",
            source.display(),
            dest.display()
        ));
    }
    std::fs::rename(source, &dest).map_err(|error| {
        format!(
            "cannot move {} -> {}: {error}",
            source.display(),
            dest.display()
        )
    })
}
