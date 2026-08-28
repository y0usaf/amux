//! omp's session-store contract: the directories omp owns and how project
//! paths encode to on-disk session folder names. harness-core's `agent`
//! machinery reads the shared JSONL format; everything omp-specific about
//! where that store lives is declared here.

use std::path::{Path, PathBuf};

/// omp relocates its whole agent dir with this env var.
pub const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
/// omp relocates its session root with this env var (flat dir, no project encoding).
pub const SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";

/// omp keeps its state under `$HOME/.omp/agent`.
pub const DEFAULT_AGENT_DIR_REL: &str = ".omp/agent";

/// Sidecar wire env vars, matching what the packaged omp extension reads.
pub const SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_OMP_SIDECAR_SOCKET";
pub const SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_OMP_SESSION_KEY";
pub const EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_OMP_EXTENSION";
pub const ASCII_ENV: &str = "AGENT_HARNESS_OMP_ASCII";

/// Prefix for the harness sidecar socket under the runtime dir.
pub const SOCKET_PREFIX: &str = "omp-sidecar";

/// `$XDG_DATA_HOME/omp` becomes the data root once it exists (mirrors omp's
/// DirResolver), flattening sessions to `<root>/sessions`.
pub fn default_agent_dir() -> Option<PathBuf> {
    if let Some(app_root) = std::env::var_os("XDG_DATA_HOME")
        .map(|root| PathBuf::from(root).join("omp"))
        .filter(|app_root| app_root.exists())
    {
        return Some(app_root);
    }
    Some(home_dir()?.join(DEFAULT_AGENT_DIR_REL))
}

/// Mirrors oh-my-pi's `getDefaultSessionDirName` (session-paths.ts): project
/// dirs under HOME encode home-relative with a single leading dash, under the
/// temp root with `-tmp`, and everything else keeps the legacy `--abs--` form.
/// omp >=17.2.9 migrated the old `--<home>-…--` names to this scheme on first
/// run, so scanning must match it or every project resolves to a dead
/// directory and its sessions vanish from the sidebar.
pub fn encode_project_path(project_path: &Path) -> String {
    let resolved = crate::util::normalize_project_path(project_path);
    let encode_relative =
        |relative: &Path| relative.to_string_lossy().replace(['/', '\\', ':'], "-");
    let scopes = [("-", home_dir()), ("-tmp", Some(std::env::temp_dir()))];
    for (prefix, root) in scopes {
        let Some(root) = root else { continue };
        let root = crate::util::normalize_project_path(&root);
        let Ok(relative) = resolved.strip_prefix(&root) else {
            continue;
        };
        let out = if relative.as_os_str().is_empty() {
            prefix.to_string()
        } else if prefix.ends_with('-') {
            format!("{prefix}{}", encode_relative(relative))
        } else {
            format!("{prefix}-{}", encode_relative(relative))
        };
        return out;
    }
    let absolute = resolved.strip_prefix("/").unwrap_or(&resolved);
    format!("--{}--", encode_relative(absolute))
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
            "amux-omp-store-tests-{}-{}",
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
    fn omp_encode_project_path_matches_current_scheme() {
        let home = test_dir();
        let _guard = EnvGuard::set(&[("HOME", Some(home.as_os_str()))]);

        // Home-relative projects get a single leading dash (oh-my-pi migrates
        // the legacy `--<home>-…--` names to this form on first run).
        assert_eq!(encode_project_path(&home.join("work/proj")), "-work-proj");
        assert_eq!(encode_project_path(&home), "-");

        // Temp-root projects are scoped under -tmp.
        let tmp_root = std::env::temp_dir();
        let tmp_project = tmp_root.join("amux-encode-test/proj");
        let expected = format!(
            "-tmp-{}",
            tmp_project
                .strip_prefix(&tmp_root)
                .unwrap()
                .to_string_lossy()
                .replace('/', "-")
        );
        assert_eq!(encode_project_path(&tmp_project), expected);

        // Everything else keeps the legacy absolute form.
        assert_eq!(
            encode_project_path(Path::new("/work/tree")),
            "--work-tree--"
        );
    }

    #[test]
    fn omp_scan_finds_sessions_written_in_native_layout() {
        let home = test_dir();
        let _guard = EnvGuard::set(&[
            ("HOME", Some(home.as_os_str())),
            (AGENT_DIR_ENV, None),
            (SESSION_DIR_ENV, None),
        ]);
        let project = home.join("work/proj");
        fs::create_dir_all(&project).unwrap();

        // Write the fixture in oh-my-pi's native on-disk layout instead of
        // through live_project_dir, so a scheme drift fails here.
        let native_dir = home.join(".omp/agent/sessions").join("-work-proj");
        fs::create_dir_all(&native_dir).unwrap();
        fs::write(
            native_dir.join("s.jsonl"),
            format!(
                "{{\"type\":\"session\",\"id\":\"s\",\"cwd\":\"{}\",\"timestamp\":\"2024-01-01T00:00:00Z\"}}\n{{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"hello\"}}}}\n",
                project.to_string_lossy(),
            ),
        )
        .unwrap();

        let sessions = crate::agent::scan_live_sessions(&project);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s");
    }
}
