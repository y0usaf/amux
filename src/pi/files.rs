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

fn move_file_into_dir_with_fallback(source: &Path, dest_dir: &Path) -> Result<(), String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("cannot move {}: missing file name", source.display()))?;
    std::fs::create_dir_all(dest_dir)
        .map_err(|error| format!("cannot create {}: {error}", dest_dir.display()))?;

    let dest = dest_dir.join(file_name);
    if dest.exists() {
        return Err(format!(
            "cannot move {} -> {}: destination exists",
            source.display(),
            dest.display()
        ));
    }

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

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let lock = test_support::env_lock();
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self {
                _lock: lock,
                key,
                old,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "pi-harness-files-tests-{}-{}",
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
    fn encode_project_path_replaces_separators_for_absolute_paths() {
        let encoded = encode_project_path(Path::new("/tmp/project"));
        assert_eq!(encoded, "--tmp-project--");
    }

    #[cfg(windows)]
    #[test]
    fn encode_project_path_sanitizes_drive_prefixes() {
        let encoded = encode_project_path(Path::new(r"C:\work\tree"));
        assert!(!encoded.contains(['/', '\\', ':']));
        assert!(encoded.starts_with("--C_"));
    }

    #[test]
    fn encode_project_path_normalizes_relative_components() {
        let encoded = encode_project_path(Path::new("src/../src"));
        assert!(encoded.ends_with("src--"));
        assert!(!encoded.contains(".."));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn live_project_dir_uses_home_and_encoded_project_path() {
        let home = TestDir::new();
        let _guard = EnvGuard::set("HOME", home.path());

        let live = live_project_dir(Path::new("/work/tree")).unwrap();
        assert_eq!(live, home.path().join(LIVE_ROOT_REL).join("--work-tree--"));
    }

    #[test]
    fn is_pi_session_path_matches_only_under_pi_root() {
        let home = TestDir::new();
        let _guard = EnvGuard::set("HOME", home.path());
        let inside = home
            .path()
            .join(LIVE_ROOT_REL)
            .join("--work-tree--/a.jsonl");
        let outside = home.path().join("elsewhere/a.jsonl");

        assert!(is_pi_session_path(&inside));
        assert!(!is_pi_session_path(&outside));
    }

    #[test]
    fn archive_session_file_moves_into_archive_dir() {
        let home = TestDir::new();
        let _guard = EnvGuard::set("HOME", home.path());
        let source_dir = home.path().join("sessions");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "payload").unwrap();

        archive_session_file(&source).unwrap();

        let archived = home
            .path()
            .join(LIVE_ROOT_REL)
            .join(ARCHIVE_DIR_NAME)
            .join("session.jsonl");
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(archived).unwrap(), "payload");
    }

    #[test]
    fn restore_session_file_moves_into_live_project_dir() {
        let home = TestDir::new();
        let _guard = EnvGuard::set("HOME", home.path());
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
        let _guard = EnvGuard::set("HOME", home.path());
        let source_dir = home.path().join("sessions");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("session.jsonl");
        fs::write(&source, "new-payload").unwrap();

        let archive_dir = home.path().join(LIVE_ROOT_REL).join(ARCHIVE_DIR_NAME);
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
        let _guard = EnvGuard::set("HOME", home.path());
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
