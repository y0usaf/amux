use std::path::{Path, PathBuf};

use super::implementation::store::{
    default_agent_dir, encode_project_path, AGENT_DIR_ENV, SESSION_DIR_ENV, SOCKET_PREFIX,
};
use crate::util::app_runtime_dir;

pub(crate) const SESSIONS_DIR_NAME: &str = "sessions";
pub(crate) const ARCHIVE_DIR_NAME: &str = "ARCHIVE";

pub fn socket_path() -> PathBuf {
    app_runtime_dir().join(format!("{SOCKET_PREFIX}-{}.sock", std::process::id()))
}

pub fn archive_session_file(source: &Path) -> Result<(), String> {
    let Some(dest_dir) = archive_dir() else {
        return Err(format!(
            "cannot archive session {}: agent archive dir unavailable",
            source.display()
        ));
    };
    move_file_into_dir_with_fallback(source, &dest_dir)
}

pub fn restore_session_file(source: &Path, project_path: &Path) -> Result<(), String> {
    let Some(dest_dir) = live_project_dir(project_path) else {
        return Err(format!(
            "cannot restore session {}: agent live dir unavailable for {}",
            source.display(),
            project_path.display()
        ));
    };
    move_file_into_dir_with_fallback(source, &dest_dir)
}

pub fn live_project_dir(project_path: &Path) -> Option<PathBuf> {
    if let Some(dir) = configured_session_dir() {
        return Some(dir);
    }
    Some(sessions_root()?.join(encode_project_path(project_path)))
}

pub(crate) fn sessions_root() -> Option<PathBuf> {
    configured_session_dir().or_else(|| Some(agent_dir()?.join(SESSIONS_DIR_NAME)))
}

fn agent_dir() -> Option<PathBuf> {
    env_path(AGENT_DIR_ENV).or_else(default_agent_dir)
}

fn configured_session_dir() -> Option<PathBuf> {
    env_path(SESSION_DIR_ENV)
}

pub(super) fn archive_dir() -> Option<PathBuf> {
    Some(sessions_root()?.join(ARCHIVE_DIR_NAME))
}

fn env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    expand_tilde(&path)
}

fn expand_tilde(path: &Path) -> Option<PathBuf> {
    if path == Path::new("~") {
        return home_dir();
    }
    if let Ok(rest) = path.strip_prefix("~") {
        return Some(home_dir()?.join(rest));
    }
    Some(path.to_path_buf())
}

fn move_file_into_dir_with_fallback(source: &Path, dest_dir: &Path) -> Result<(), String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("cannot move {}: missing file name", source.display()))?;
    std::fs::create_dir_all(dest_dir)
        .map_err(|error| format!("cannot create {}: {error}", dest_dir.display()))?;

    let dest = dest_dir.join(file_name);
    if path_exists(&dest)? {
        if path_exists(source)? {
            return Err(format!(
                "cannot move {} -> {}: destination exists",
                source.display(),
                dest.display()
            ));
        }
        return Ok(());
    }
    if !path_exists(source)? {
        return Err(format!(
            "cannot move {} -> {}: source missing",
            source.display(),
            dest.display()
        ));
    }

    match std::fs::rename(source, &dest) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            if path_exists(&dest).unwrap_or(false) && !path_exists(source).unwrap_or(true) {
                return Ok(());
            }
            std::fs::copy(source, &dest).map_err(|copy_error| {
                format!(
                    "cannot move {} -> {}: rename failed ({rename_error}); copy failed ({copy_error})",
                    source.display(),
                    dest.display()
                )
            })?;
            if let Err(remove_error) = std::fs::remove_file(source) {
                return Err(format!(
                    "copied {} -> {} but failed to remove source; leaving both files in place: {remove_error}",
                    source.display(),
                    dest.display()
                ));
            }
            Ok(())
        }
    }
}

fn path_exists(path: &Path) -> Result<bool, String> {
    path.try_exists()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
