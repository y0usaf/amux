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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::implementation::store::DEFAULT_AGENT_DIR_REL;
    use crate::test_support;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        old: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn default_paths(home: &Path) -> Self {
            Self::set_paths(home, None, None)
        }

        fn with_agent_dir(home: &Path, agent_dir: &Path) -> Self {
            Self::set_paths(home, Some(agent_dir), None)
        }

        fn with_session_dir(home: &Path, agent_dir: &Path, session_dir: &Path) -> Self {
            Self::set_paths(home, Some(agent_dir), Some(session_dir))
        }

        fn set_paths(home: &Path, agent_dir: Option<&Path>, session_dir: Option<&Path>) -> Self {
            let lock = test_support::env_lock();
            let keys = ["HOME", AGENT_DIR_ENV, SESSION_DIR_ENV];
            let old = keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();

            std::env::set_var("HOME", home);
            match agent_dir {
                Some(value) => std::env::set_var(AGENT_DIR_ENV, value),
                None => std::env::remove_var(AGENT_DIR_ENV),
            }
            match session_dir {
                Some(value) => std::env::set_var(SESSION_DIR_ENV, value),
                None => std::env::remove_var(SESSION_DIR_ENV),
            }

            Self { _lock: lock, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, old) in &self.old {
                match old {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "harness-core-files-tests-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn live_project_dir_uses_home_and_encoded_project_path() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());

        let live = live_project_dir(Path::new("/work/tree")).unwrap();
        assert_eq!(
            live,
            home.path()
                .join(DEFAULT_AGENT_DIR_REL)
                .join(SESSIONS_DIR_NAME)
                .join("--work-tree--")
        );
    }

    #[test]
    fn agent_dir_env_relocates_sessions_and_archive() {
        let home = TestDir::new();
        let agent_dir = home.path().join("xdg/pi/agent");
        let _guard = EnvGuard::with_agent_dir(home.path(), &agent_dir);

        assert_eq!(
            live_project_dir(Path::new("/work/tree")).unwrap(),
            agent_dir.join(SESSIONS_DIR_NAME).join("--work-tree--")
        );
        assert_eq!(
            archive_dir().unwrap(),
            agent_dir.join(SESSIONS_DIR_NAME).join(ARCHIVE_DIR_NAME)
        );
    }

    #[test]
    fn session_dir_env_overrides_agent_dir_without_project_encoding() {
        let home = TestDir::new();
        let agent_dir = home.path().join("agent");
        let session_dir = home.path().join("custom-sessions");
        let _guard = EnvGuard::with_session_dir(home.path(), &agent_dir, &session_dir);

        assert_eq!(
            live_project_dir(Path::new("/work/tree")).unwrap(),
            session_dir
        );
        assert_eq!(archive_dir().unwrap(), session_dir.join(ARCHIVE_DIR_NAME));
    }

    #[test]
    fn archive_session_file_moves_into_archive_dir() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());
        let source_dir = home.path().join("sessions");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "payload").unwrap();

        archive_session_file(&source).unwrap();

        let archived = home
            .path()
            .join(DEFAULT_AGENT_DIR_REL)
            .join(SESSIONS_DIR_NAME)
            .join(ARCHIVE_DIR_NAME)
            .join("session.jsonl");
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(archived).unwrap(), "payload");
    }

    #[test]
    fn restore_session_file_moves_into_live_project_dir() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());
        let source_dir = home.path().join("archive");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        let project = home.path().join("work/project");
        fs::create_dir_all(&project).unwrap();
        fs::write(&source, "payload").unwrap();

        restore_session_file(&source, &project).unwrap();

        let restored = live_project_dir(&project).unwrap().join("session.jsonl");
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(restored).unwrap(), "payload");
    }

    #[test]
    fn archive_session_file_rejects_existing_destination_without_overwriting() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());
        let source_dir = home.path().join("sessions");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "new-payload").unwrap();

        let archive_dir = home
            .path()
            .join(DEFAULT_AGENT_DIR_REL)
            .join(SESSIONS_DIR_NAME)
            .join(ARCHIVE_DIR_NAME);
        fs::create_dir_all(&archive_dir).unwrap();
        let archived = archive_dir.join("session.jsonl");
        fs::write(&archived, "existing-payload").unwrap();

        let err = archive_session_file(&source).unwrap_err();

        assert!(err.contains("destination exists"));
        assert_eq!(fs::read_to_string(&source).unwrap(), "new-payload");
        assert_eq!(fs::read_to_string(&archived).unwrap(), "existing-payload");
    }

    #[test]
    fn restore_session_file_rejects_existing_destination_without_overwriting() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());
        let source_dir = home.path().join("archive");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "new-payload").unwrap();

        let project = home.path().join("work/project");
        fs::create_dir_all(&project).unwrap();
        let live_dir = live_project_dir(&project).unwrap();
        fs::create_dir_all(&live_dir).unwrap();
        let restored = live_dir.join("session.jsonl");
        fs::write(&restored, "existing-payload").unwrap();

        let err = restore_session_file(&source, &project).unwrap_err();

        assert!(err.contains("destination exists"));
        assert_eq!(fs::read_to_string(&source).unwrap(), "new-payload");
        assert_eq!(fs::read_to_string(&restored).unwrap(), "existing-payload");
    }

    #[test]
    fn archive_session_file_treats_existing_archive_and_missing_source_as_success() {
        let home = TestDir::new();
        let _guard = EnvGuard::default_paths(home.path());
        let source_dir = home.path().join("sessions");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");

        let archive_dir = home
            .path()
            .join(DEFAULT_AGENT_DIR_REL)
            .join(SESSIONS_DIR_NAME)
            .join(ARCHIVE_DIR_NAME);
        fs::create_dir_all(&archive_dir).unwrap();
        let archived = archive_dir.join("session.jsonl");
        fs::write(&archived, "archived-payload").unwrap();

        archive_session_file(&source).unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read_to_string(&archived).unwrap(), "archived-payload");
    }

    #[test]
    fn move_file_into_dir_with_fallback_moves_file_and_preserves_contents() {
        let dir = TestDir::new();
        let source_dir = dir.path().join("source");
        let dest_dir = dir.path().join("dest");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "payload").unwrap();

        move_file_into_dir_with_fallback(&source, &dest_dir).unwrap();

        let dest = dest_dir.join("session.jsonl");
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(dest).unwrap(), "payload");
    }

    #[test]
    fn move_file_into_dir_with_fallback_rejects_paths_without_file_name() {
        let err = move_file_into_dir_with_fallback(Path::new("/"), Path::new("/tmp/test-dest"))
            .unwrap_err();
        assert!(err.contains("missing file name"));
    }
}
