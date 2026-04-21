use std::env;
use std::path::{Path, PathBuf};

pub fn normalize_project_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    absolute.canonicalize().unwrap_or(absolute)
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
