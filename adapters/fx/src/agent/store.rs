//! fx's session-store contract: the directories fx owns and where its state
//! lives. harness-core splices this module in via `#[cfg(feature = "fx")]`
//! `#[path]` like the pi and omp adapters.
//!
//! fx keeps one directory per session under `$HOME/.fx/sessions/<id>/`:
//! `session.json` (facts), `display.json` (title/preview), `events.jsonl`
//! (append-only event log), and `session.lock` (liveness). There is no
//! per-project directory encoding: sessions carry `workspace_root` and the
//! scan filters after listing.

use std::path::PathBuf;

/// fx relocates its whole state dir with this env var.
pub const FX_DIR_ENV: &str = "FX_DIR";
/// Harness override for the session root (flat dir).
pub const SESSION_DIR_ENV: &str = "AGENT_HARNESS_FX_SESSION_DIR";

/// fx keeps its state under `$HOME/.fx`.
pub const DEFAULT_AGENT_DIR_REL: &str = ".fx";

/// Sidecar wire env vars. fx has no extension host yet, so nothing consumes
/// these today; they keep the harness-sidecar plumbing uniform across
/// adapters and become live when fx gains an extension surface.
pub const SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_FX_SIDECAR_SOCKET";
pub const SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_FX_SESSION_KEY";
pub const EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_FX_EXTENSION";
pub const ASCII_ENV: &str = "AGENT_HARNESS_FX_ASCII";

/// Prefix for the harness sidecar socket under the runtime dir.
pub const SOCKET_PREFIX: &str = "fx-sidecar";

pub fn default_agent_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(FX_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(dir);
    }
    Some(home_dir()?.join(DEFAULT_AGENT_DIR_REL))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::ffi::OsStr;
    use std::sync::MutexGuard;

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        old: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn set(keys: &[(&'static str, Option<&OsStr>)]) -> Self {
            let lock = test_support::env_lock();
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

    #[test]
    fn default_agent_dir_prefers_fx_dir_env() {
        let _guard = EnvGuard::set(&[(FX_DIR_ENV, Some(OsStr::new("/tmp/fx-root")))]);
        assert_eq!(default_agent_dir(), Some(PathBuf::from("/tmp/fx-root")));
    }

    #[test]
    fn default_agent_dir_falls_back_to_home_dot_fx() {
        let _guard = EnvGuard::set(&[
            ("HOME", Some(OsStr::new("/tmp/fx-home"))),
            (FX_DIR_ENV, None),
        ]);
        assert_eq!(default_agent_dir(), Some(PathBuf::from("/tmp/fx-home/.fx")));
    }
}
