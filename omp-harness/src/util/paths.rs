use std::env;
use std::path::{Component, Path, PathBuf};

pub fn normalize_project_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    absolute
        .canonicalize()
        .unwrap_or_else(|_| normalize_lexical_path(&absolute))
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut has_root = false;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => {
                normalized.push(std::path::MAIN_SEPARATOR_STR);
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = normalized
                    .components()
                    .next_back()
                    .is_some_and(|last| matches!(last, Component::Normal(_)));
                if can_pop {
                    normalized.pop();
                } else if !has_root {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

pub fn app_config_dir() -> PathBuf {
    xdg_config_home().join("pi-harness")
}

pub fn app_state_dir() -> PathBuf {
    xdg_state_home().join("pi-harness")
}

pub fn app_runtime_dir() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("pi-harness");
    }

    xdg_state_home().join("pi-harness").join("runtime")
}

fn xdg_config_home() -> PathBuf {
    if let Some(value) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(value);
    }

    home_dir().join(".config")
}

fn xdg_state_home() -> PathBuf {
    if let Some(value) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(value);
    }

    home_dir().join(".local").join("state")
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::ffi::OsString;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    struct EnvVarGuard {
        key: String,
        old: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let old = env::var_os(key);
            // SAFETY: callers serialize process-environment mutation with the global test lock.
            unsafe { env::set_var(key, value) };
            Self {
                key: key.to_string(),
                old,
            }
        }

        fn remove(key: &str) -> Self {
            let old = env::var_os(key);
            // SAFETY: callers serialize process-environment mutation with the global test lock.
            unsafe { env::remove_var(key) };
            Self {
                key: key.to_string(),
                old,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => {
                    // SAFETY: restoring previous process environment value while tests hold the env lock.
                    unsafe { env::set_var(&self.key, value) };
                }
                None => {
                    // SAFETY: restoring previous unset process environment state while tests hold the env lock.
                    unsafe { env::remove_var(&self.key) };
                }
            }
        }
    }

    fn with_env_var_set<R>(key: &str, value: &str, f: impl FnOnce() -> R) -> R {
        let _guard = EnvVarGuard::set(key, value);
        f()
    }

    fn with_env_var_removed<R>(key: &str, f: impl FnOnce() -> R) -> R {
        let _guard = EnvVarGuard::remove(key);
        f()
    }

    #[test]
    fn normalize_project_path_makes_relative_paths_absolute_even_when_missing() {
        let relative = Path::new("src/../definitely-missing-test-path");
        let normalized = normalize_project_path(relative);
        assert!(normalized.is_absolute());
        assert!(normalized.ends_with("definitely-missing-test-path"));
    }

    #[test]
    fn normalize_project_path_canonicalizes_existing_paths() {
        let normalized = normalize_project_path(Path::new("src/../src"));
        assert!(normalized.is_absolute());
        assert!(normalized.ends_with("src"));
        assert!(!normalized.to_string_lossy().contains("/../"));
    }

    #[test]
    fn normalize_project_path_collapses_missing_parent_segments_lexically() {
        let normalized = normalize_project_path(Path::new(
            "definitely-missing-parent-segment/../normalized-target",
        ));
        assert!(normalized.is_absolute());
        assert!(normalized.ends_with("normalized-target"));
        assert!(!normalized.to_string_lossy().contains("/../"));
    }

    #[test]
    fn app_dirs_prefer_xdg_variables_and_runtime_falls_back_to_state() {
        let _lock = test_support::env_lock();
        with_env_var_set("XDG_CONFIG_HOME", "/tmp/pi-config", || {
            assert_eq!(app_config_dir(), PathBuf::from("/tmp/pi-config/pi-harness"));
        });

        with_env_var_set("XDG_STATE_HOME", "/tmp/pi-state", || {
            assert_eq!(app_state_dir(), PathBuf::from("/tmp/pi-state/pi-harness"));
            with_env_var_removed("XDG_RUNTIME_DIR", || {
                assert_eq!(
                    app_runtime_dir(),
                    PathBuf::from("/tmp/pi-state/pi-harness/runtime")
                );
            });
        });
    }

    #[test]
    fn app_runtime_dir_uses_runtime_env_when_present() {
        let _lock = test_support::env_lock();
        with_env_var_set("XDG_RUNTIME_DIR", "/tmp/pi-runtime", || {
            assert_eq!(
                app_runtime_dir(),
                PathBuf::from("/tmp/pi-runtime/pi-harness")
            );
        });
    }

    #[test]
    fn app_runtime_dir_prefers_runtime_over_state_when_both_are_set() {
        let _lock = test_support::env_lock();
        with_env_var_set("XDG_STATE_HOME", "/tmp/pi-state", || {
            with_env_var_set("XDG_RUNTIME_DIR", "/tmp/pi-runtime", || {
                assert_eq!(
                    app_runtime_dir(),
                    PathBuf::from("/tmp/pi-runtime/pi-harness")
                );
            });
        });
    }

    #[test]
    fn env_helpers_restore_original_value_after_panic() {
        let _lock = test_support::env_lock();
        let _original = EnvVarGuard::set("XDG_CONFIG_HOME", "/tmp/pi-config-original");

        let result = catch_unwind(AssertUnwindSafe(|| {
            with_env_var_set("XDG_CONFIG_HOME", "/tmp/pi-config-override", || {
                panic!("boom");
            });
        }));

        assert!(result.is_err());
        assert_eq!(
            env::var_os("XDG_CONFIG_HOME"),
            Some(OsString::from("/tmp/pi-config-original"))
        );
    }
}
