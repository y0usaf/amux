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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::fs;
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        old: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn set(keys: &[(&'static str, Option<&OsStr>)]) -> Self {
            let lock = crate::test_support::env_lock();
            let old = keys
                .iter()
                .map(|(key, _)| (*key, std::env::var_os(key)))
                .collect();
            for (key, value) in keys {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
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

    fn test_dir() -> PathBuf {
        let unique = format!(
            "amux-pi-store-tests-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn encode_project_path_matches_pi_store_layout() {
        assert_eq!(encode_project_path(Path::new("/project")), "--project--");
        assert_eq!(
            encode_project_path(Path::new("/home/u/work/proj")),
            "--home-u-work-proj--"
        );
        assert!(encode_project_path(Path::new("src/../src")).ends_with("src--"));
        assert!(!encode_project_path(Path::new("src/../src")).contains(".."));
        assert!(!encode_project_path(Path::new("src/../src")).contains('/'));
    }

    #[cfg(windows)]
    #[test]
    fn encode_project_path_sanitizes_drive_prefixes() {
        let encoded = encode_project_path(Path::new(r"C:\work\tree"));
        assert!(!encoded.contains(['/', '\\', ':']));
        assert!(encoded.starts_with("--C_"));
    }

    #[test]
    fn pi_scan_finds_sessions_written_in_native_layout() {
        let home = test_dir();
        let _guard = EnvGuard::set(&[
            ("HOME", Some(home.as_os_str())),
            (AGENT_DIR_ENV, None),
            (SESSION_DIR_ENV, None),
        ]);
        let project = home.join("work/proj");
        fs::create_dir_all(&project).unwrap();

        // Write the fixture in pi's native on-disk layout instead of through
        // live_project_dir, so a scheme drift fails here.
        let resolved = fs::canonicalize(&project).unwrap();
        let native_dir = home.join(".pi/agent/sessions").join(format!(
            "--{}--",
            resolved
                .to_string_lossy()
                .trim_start_matches('/')
                .replace('/', "-")
        ));
        fs::create_dir_all(&native_dir).unwrap();
        fs::write(
            native_dir.join("s.jsonl"),
            format!(
                "{{\"type\":\"session\",\"id\":\"s\",\"cwd\":\"{}\",\"timestamp\":\"2024-01-01T00:00:00Z\"}}\n{{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"hello\"}}}}\n",
                resolved.to_string_lossy(),
            ),
        )
        .unwrap();

        let sessions = crate::agent::scan_live_sessions(&project);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s");
    }
}
