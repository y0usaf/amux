//! fx binary discovery and launch argv.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct FxLaunch(pub PathBuf);

impl FxLaunch {
    pub fn display(&self) -> String {
        self.0.display().to_string()
    }
}

#[derive(Clone, Debug)]
pub struct DiscoverResult {
    pub launch: FxLaunch,
    pub version: Option<String>,
}

pub fn discover(config_override: Option<&str>) -> Option<DiscoverResult> {
    discover_inner(config_override)
}

pub fn discover_fresh(config_override: Option<&str>) -> Option<DiscoverResult> {
    discover_inner(config_override)
}

pub fn launch_argv(
    config_override: Option<&str>,
    extra_args: &[String],
) -> Result<Vec<OsString>, String> {
    let discovered = discover(config_override).ok_or_else(|| {
        "Could not find fx. Install with: curl -fsSL https://fx.sh/setup.sh | bash".to_string()
    })?;
    let mut argv = vec![discovered.launch.0.into_os_string()];
    argv.extend(extra_args.iter().map(OsString::from));
    Ok(argv)
}

/// fx has no extension host; this stays None so harness code that wires the
/// sidecar extension into a launch simply skips it.
pub fn extension_path() -> Option<PathBuf> {
    None
}

fn discover_inner(config_override: Option<&str>) -> Option<DiscoverResult> {
    if let Some(path) = config_override {
        return try_binary(Path::new(path));
    }
    if let Ok(path) = std::env::var("FX_BINARY") {
        return try_binary(Path::new(&path));
    }
    if let Some(path) = which("fx") {
        if let Some(result) = try_binary(&path) {
            return Some(result);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for candidate in [home.join(".fx/bin/fx"), home.join(".local/bin/fx")] {
            if let Some(result) = try_binary(&candidate) {
                return Some(result);
            }
        }
    }
    None
}

fn try_binary(path: &Path) -> Option<DiscoverResult> {
    if !path.is_file() {
        return None;
    }
    let output = Command::new(path)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    Some(DiscoverResult {
        launch: FxLaunch(path.to_path_buf()),
        version,
    })
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
