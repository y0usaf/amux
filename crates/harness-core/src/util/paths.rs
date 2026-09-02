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
    home_dir().join(".pi")
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
