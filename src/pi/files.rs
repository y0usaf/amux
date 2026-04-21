use std::path::{Path, PathBuf};

use crate::pi::{ARCHIVE_DIR_NAME, LIVE_ROOT_REL};
use crate::util::{app_runtime_dir, normalize_project_path};

pub fn socket_path() -> PathBuf {
    app_runtime_dir().join(format!("pi-sidecar-{}.sock", std::process::id()))
}

pub fn is_pi_session_path(path: &Path) -> bool {
    pi_root().is_some_and(|root| path.starts_with(root))
}

pub fn archive_session_file(source: &Path) -> Result<(), String> {
    let Some(dest_dir) = archive_dir() else {
        return Err(format!(
            "cannot archive session {}: Pi archive dir unavailable",
            source.display()
        ));
    };
    move_file_into_dir_with_fallback(source, &dest_dir)
}

pub fn restore_session_file(source: &Path, project_path: &Path) -> Result<(), String> {
    let Some(dest_dir) = live_project_dir(project_path) else {
        return Err(format!(
            "cannot restore session {}: Pi live dir unavailable for {}",
            source.display(),
            project_path.display()
        ));
    };
    move_file_into_dir_with_fallback(source, &dest_dir)
}

pub fn live_project_dir(project_path: &Path) -> Option<PathBuf> {
    Some(pi_root()?.join(encode_project_path(project_path)))
}

fn pi_root() -> Option<PathBuf> {
    Some(home_dir()?.join(LIVE_ROOT_REL))
}

fn archive_dir() -> Option<PathBuf> {
    Some(pi_root()?.join(ARCHIVE_DIR_NAME))
}

fn encode_project_path(project_path: &Path) -> String {
    let normalized = normalize_project_path(project_path);
    let text = normalized.to_string_lossy();
    let trimmed = text.trim_start_matches('/');
    format!("--{}--", trimmed.replace('/', "-"))
}

fn move_file_into_dir_with_fallback(source: &Path, dest_dir: &Path) -> Result<(), String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("cannot move {}: missing file name", source.display()))?;
    std::fs::create_dir_all(dest_dir)
        .map_err(|error| format!("cannot create {}: {error}", dest_dir.display()))?;

    let dest = dest_dir.join(file_name);
    match std::fs::rename(source, &dest) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            std::fs::copy(source, &dest).map_err(|copy_error| {
                format!(
                    "cannot move {} -> {}: rename failed ({rename_error}); copy failed ({copy_error})",
                    source.display(),
                    dest.display()
                )
            })?;
            if let Err(remove_error) = std::fs::remove_file(source) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "copied {} -> {} but failed to remove source: {remove_error}",
                    source.display(),
                    dest.display()
                ));
            }
            Ok(())
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
