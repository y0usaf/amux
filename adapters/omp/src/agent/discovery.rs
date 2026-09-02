use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::agent::EXTENSION_PATH_ENV;

pub const PACKAGED_EXTENSION_REL: &str = "share/omp-harness/omp-extension/index.js";

#[derive(Clone, Debug)]
pub enum OmpLaunch {
    Binary(PathBuf),
    PackageRunner {
        runner: PathBuf,
        prefix_args: Vec<String>,
    },
}

impl OmpLaunch {
    pub fn display(&self) -> String {
        match self {
            Self::Binary(path) => path.display().to_string(),
            Self::PackageRunner {
                runner,
                prefix_args,
            } => {
                let mut parts = vec![runner.display().to_string()];
                parts.extend(prefix_args.iter().cloned());
                parts.join(" ")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiscoverResult {
    pub launch: OmpLaunch,
    pub version: Option<String>,
}

static CACHED: OnceLock<Mutex<Option<DiscoverResult>>> = OnceLock::new();

fn discovery_cache() -> &'static Mutex<Option<DiscoverResult>> {
    CACHED.get_or_init(|| Mutex::new(None))
}

pub fn discover(config_override: Option<&str>) -> Option<DiscoverResult> {
    if config_override.is_some() {
        return discover_inner(config_override);
    }

    if let Some(cached) = discovery_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return Some(cached);
    }

    let discovered = discover_inner(None)?;
    let mut cache = discovery_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = cache.as_ref() {
        return Some(cached.clone());
    }
    *cache = Some(discovered.clone());
    Some(discovered)
}

fn reset_discovery_cache() {
    *discovery_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

pub fn discover_fresh(config_override: Option<&str>) -> Option<DiscoverResult> {
    discover_inner(config_override)
}

pub fn launch_argv(
    config_override: Option<&str>,
    extra_args: &[String],
) -> Result<Vec<OsString>, String> {
    let discovered = discover(config_override).ok_or_else(|| {
        "Could not find omp. Install with: npm install -g @oh-my-pi/pi-coding-agent".to_string()
    })?;

    let mut argv = match discovered.launch {
        OmpLaunch::Binary(path) => vec![path.into_os_string()],
        OmpLaunch::PackageRunner {
            runner,
            prefix_args,
        } => {
            let mut argv = vec![runner.into_os_string()];
            argv.extend(prefix_args.into_iter().map(OsString::from));
            argv
        }
    };
    argv.extend(extra_args.iter().map(OsString::from));
    Ok(argv)
}

pub fn extension_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(EXTENSION_PATH_ENV) {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(mut exe) = std::env::current_exe() {
        exe.pop();
        exe.pop();
        exe.push(PACKAGED_EXTENSION_REL);
        if exe.exists() {
            return Some(exe);
        }
    }

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../adapters/omp/extension");
    dev.exists().then_some(dev)
}

fn discover_inner(config_override: Option<&str>) -> Option<DiscoverResult> {
    if let Some(path) = config_override {
        let path = PathBuf::from(path);
        return try_binary(&path);
    }

    if let Ok(path) = std::env::var("OMP_BINARY") {
        let path = PathBuf::from(path);
        return try_binary(&path);
    }

    if let Some(path) = which("omp") {
        if let Some(result) = try_binary(&path) {
            return Some(result);
        }
    }

    if let Some(home) = home_dir() {
        for candidate in well_known_locations(&home) {
            if let Some(result) = try_binary(&candidate) {
                return Some(result);
            }
        }
    }

    try_package_runner()
}

fn try_binary(path: &Path) -> Option<DiscoverResult> {
    if !path.is_file() || !is_executable(path) {
        return None;
    }

    Some(DiscoverResult {
        launch: OmpLaunch::Binary(path.to_path_buf()),
        version: probe_version_binary(path),
    })
}

fn try_package_runner() -> Option<DiscoverResult> {
    for (runner_name, pkg_arg) in &[
        ("bunx", "@oh-my-pi/pi-coding-agent"),
        ("npx", "@oh-my-pi/pi-coding-agent"),
    ] {
        let Some(runner_path) = which(runner_name) else {
            continue;
        };
        if Command::new(&runner_path)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .is_some_and(|status| status.success())
        {
            return Some(DiscoverResult {
                launch: OmpLaunch::PackageRunner {
                    runner: runner_path,
                    prefix_args: vec![pkg_arg.to_string()],
                },
                version: None,
            });
        }
    }
    None
}

fn probe_version_binary(path: &Path) -> Option<String> {
    let output = Command::new(path)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn well_known_locations(home: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        home.join(".bun/bin/omp"),
        home.join(".npm-global/bin/omp"),
        home.join(".npm/bin/omp"),
        home.join(".volta/bin/omp"),
        home.join(".local/share/mise/shims/omp"),
    ];

    let skip_system_paths = std::env::var_os("OMP_HARNESS_SKIP_SYSTEM_DISCOVERY_PATHS").is_some();
    #[cfg(not(test))]
    let skip_system_paths = false;

    if !skip_system_paths {
        paths.push(PathBuf::from("/usr/local/bin/omp"));
        paths.push(PathBuf::from("/run/current-system/sw/bin/omp"));
        #[cfg(target_os = "macos")]
        paths.push(PathBuf::from("/opt/homebrew/bin/omp"));
    }

    paths
}

fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
