use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::agent::AGENT_EXTENSION_PATH_ENV;

pub const PACKAGED_EXTENSION_REL: &str = "share/amux/pi-extension/index.js";
pub const DEV_EXTENSION_REL: &str = "pi-extension/index.js";

#[derive(Clone, Debug)]
pub enum AgentLaunch {
    Binary(PathBuf),
    PackageRunner {
        runner: PathBuf,
        prefix_args: Vec<String>,
    },
}

impl AgentLaunch {
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
    pub launch: AgentLaunch,
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

#[cfg(test)]
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
        "Could not find pi. Install with: npm install -g @mariozechner/pi-coding-agent".to_string()
    })?;

    let mut argv = match discovered.launch {
        AgentLaunch::Binary(path) => vec![path.into_os_string()],
        AgentLaunch::PackageRunner {
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
    if let Ok(path) = std::env::var(AGENT_EXTENSION_PATH_ENV) {
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

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEV_EXTENSION_REL);
    dev.exists().then_some(dev)
}

fn discover_inner(config_override: Option<&str>) -> Option<DiscoverResult> {
    if let Some(path) = config_override {
        let path = PathBuf::from(path);
        return try_binary(&path);
    }

    if let Ok(path) = std::env::var("AGENT_BINARY") {
        let path = PathBuf::from(path);
        return try_binary(&path);
    }

    if let Some(path) = which("pi") {
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
        launch: AgentLaunch::Binary(path.to_path_buf()),
        version: probe_version_binary(path),
    })
}

fn try_package_runner() -> Option<DiscoverResult> {
    for (runner_name, pkg_arg) in &[
        ("bunx", "@mariozechner/pi-coding-agent"),
        ("npx", "@mariozechner/pi-coding-agent"),
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
                launch: AgentLaunch::PackageRunner {
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
        home.join(".bun/bin/pi"),
        home.join(".npm-global/bin/pi"),
        home.join(".npm/bin/pi"),
        home.join(".volta/bin/pi"),
        home.join(".local/share/mise/shims/pi"),
    ];

    #[cfg(test)]
    let skip_system_paths = std::env::var_os("AGENT_HARNESS_SKIP_SYSTEM_DISCOVERY_PATHS").is_some();
    #[cfg(not(test))]
    let skip_system_paths = false;

    if !skip_system_paths {
        paths.push(PathBuf::from("/usr/local/bin/pi"));
        paths.push(PathBuf::from("/run/current-system/sw/bin/pi"));
        #[cfg(target_os = "macos")]
        paths.push(PathBuf::from("/opt/homebrew/bin/pi"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        key: &'static str,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &OsStr) -> Self {
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

    fn with_env_var_set<R>(key: &'static str, value: &OsStr, f: impl FnOnce() -> R) -> R {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        let result = f();
        match old {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        result
    }

    fn with_env_var_removed<R>(key: &'static str, f: impl FnOnce() -> R) -> R {
        let old = std::env::var_os(key);
        std::env::remove_var(key);
        let result = f();
        if let Some(value) = old {
            std::env::set_var(key, value);
        }
        result
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "amux-discovery-tests-{}-{}",
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

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    #[test]
    fn pi_launch_display_formats_binary_and_runner_variants() {
        let binary = AgentLaunch::Binary(PathBuf::from("/tmp/pi"));
        assert_eq!(binary.display(), "/tmp/pi");

        let runner = AgentLaunch::PackageRunner {
            runner: PathBuf::from("/usr/bin/env"),
            prefix_args: vec!["bunx".into(), "pi".into()],
        };
        assert_eq!(runner.display(), "/usr/bin/env bunx pi");
    }

    #[test]
    fn which_finds_executable_from_path() {
        let dir = TestDir::new();
        let exe = dir.path().join("pi-test");
        write_executable(&exe, "#!/bin/sh\nexit 0\n");
        let _guard = EnvGuard::set("PATH", dir.path().as_os_str());

        assert_eq!(which("pi-test"), Some(exe));
    }

    #[test]
    fn try_binary_reports_version_for_existing_executable() {
        let dir = TestDir::new();
        let exe = dir.path().join("pi");
        write_executable(&exe, "#!/bin/sh\necho pi-test 1.2.3\n");

        let result = try_binary(&exe).unwrap();
        match result.launch {
            AgentLaunch::Binary(path) => assert_eq!(path, exe),
            AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
        }
        assert_eq!(result.version.as_deref(), Some("pi-test 1.2.3"));
    }

    #[cfg(unix)]
    #[test]
    fn try_binary_rejects_non_executable_files() {
        let dir = TestDir::new();
        let file = dir.path().join("pi");
        fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();

        assert!(try_binary(&file).is_none());
    }

    #[test]
    fn extension_path_prefers_existing_env_override() {
        let dir = TestDir::new();
        let extension = dir.path().join("harness-sidechannel.js");
        fs::write(&extension, "// test extension\n").unwrap();
        let _guard = EnvGuard::set(AGENT_EXTENSION_PATH_ENV, extension.as_os_str());

        assert_eq!(extension_path(), Some(extension));
    }

    #[test]
    fn well_known_locations_include_user_and_system_defaults() {
        let _lock = test_support::env_lock();
        let home = Path::new("/home/tester");

        with_env_var_removed("AGENT_HARNESS_SKIP_SYSTEM_DISCOVERY_PATHS", || {
            let locations = well_known_locations(home);

            assert!(locations.contains(&home.join(".bun/bin/pi")));
            assert!(locations.contains(&home.join(".volta/bin/pi")));
            assert!(locations.contains(&PathBuf::from("/usr/local/bin/pi")));
            assert!(locations.contains(&PathBuf::from("/run/current-system/sw/bin/pi")));
        });
    }

    #[test]
    fn config_override_bypasses_cached_default_discovery() {
        let dir = TestDir::new();
        let cached = dir.path().join("pi-cached");
        let override_path = dir.path().join("pi-override");
        write_executable(&cached, "#!/bin/sh\nexit 0\n");
        write_executable(&override_path, "#!/bin/sh\nexit 0\n");
        let _guard = EnvGuard::set("AGENT_BINARY", cached.as_os_str());

        reset_discovery_cache();
        let _ = discover(None);
        let result = discover(Some(override_path.to_str().unwrap())).expect("override result");

        match result.launch {
            AgentLaunch::Binary(path) => assert_eq!(path, override_path),
            AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
        }
        reset_discovery_cache();
    }

    #[test]
    fn invalid_config_override_does_not_fall_back_to_env_or_path() {
        let dir = TestDir::new();
        let path_pi = dir.path().join("pi");
        let missing = dir.path().join("missing-pi");
        write_executable(
            &path_pi,
            "#!/bin/sh
exit 0
",
        );
        let path = std::env::join_paths([dir.path()]).unwrap();
        let _lock = test_support::env_lock();

        with_env_var_set("PATH", path.as_os_str(), || {
            with_env_var_removed("AGENT_BINARY", || {
                reset_discovery_cache();
                assert!(discover(Some(missing.to_str().unwrap())).is_none());
                reset_discovery_cache();
            })
        });
    }

    #[test]
    fn invalid_pi_binary_env_does_not_fall_back_to_path() {
        let dir = TestDir::new();
        let path_pi = dir.path().join("pi");
        let missing = dir.path().join("missing-pi");
        write_executable(
            &path_pi,
            "#!/bin/sh
exit 0
",
        );
        let path = std::env::join_paths([dir.path()]).unwrap();
        let _lock = test_support::env_lock();

        with_env_var_set("PATH", path.as_os_str(), || {
            with_env_var_set("AGENT_BINARY", missing.as_os_str(), || {
                reset_discovery_cache();
                assert!(discover(None).is_none());
                reset_discovery_cache();
            })
        });
    }

    #[test]
    fn default_discovery_caches_first_result_while_discover_fresh_tracks_env_changes() {
        let dir = TestDir::new();
        let first = dir.path().join("pi-a");
        let second = dir.path().join("pi-b");
        write_executable(&first, "#!/bin/sh\nexit 0\n");
        write_executable(&second, "#!/bin/sh\nexit 0\n");
        let _lock = test_support::env_lock();

        let (first_result, second_cached, second_fresh) = with_env_var_removed("PATH", || {
            with_env_var_removed("HOME", || {
                reset_discovery_cache();
                let first_result = with_env_var_set("AGENT_BINARY", first.as_os_str(), || {
                    discover(None).expect("first discovery")
                });
                let second_cached = with_env_var_set("AGENT_BINARY", second.as_os_str(), || {
                    discover(None).expect("cached discovery")
                });
                let second_fresh = with_env_var_set("AGENT_BINARY", second.as_os_str(), || {
                    discover_fresh(None).expect("fresh discovery")
                });
                reset_discovery_cache();
                (first_result, second_cached, second_fresh)
            })
        });

        match first_result.launch {
            AgentLaunch::Binary(path) => assert_eq!(path, first),
            AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
        }
        match second_cached.launch {
            AgentLaunch::Binary(path) => assert_eq!(path, first),
            AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
        }
        match second_fresh.launch {
            AgentLaunch::Binary(path) => assert_eq!(path, second),
            AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
        }
    }

    #[test]
    fn package_runner_falls_back_from_missing_bunx_to_working_npx() {
        let dir = TestDir::new();
        let npx = dir.path().join("npx");
        write_executable(&npx, "#!/bin/sh\nexit 0\n");
        let path = std::env::join_paths([dir.path()]).unwrap();
        let _guard = EnvGuard::set("PATH", path.as_os_str());

        let result = try_package_runner().expect("should find npx");
        match result.launch {
            AgentLaunch::PackageRunner {
                runner,
                prefix_args,
            } => {
                assert_eq!(runner, npx);
                assert_eq!(prefix_args, vec!["@mariozechner/pi-coding-agent"]);
            }
            AgentLaunch::Binary(_) => panic!("expected package runner launch"),
        }
    }

    #[test]
    fn discover_finds_binary_on_path_after_cache_reset() {
        let dir = TestDir::new();
        let exe = dir.path().join("pi");
        let path = std::env::join_paths([dir.path()]).unwrap();
        let _lock = test_support::env_lock();

        with_env_var_set("HOME", dir.path().as_os_str(), || {
            with_env_var_set("PATH", path.as_os_str(), || {
                with_env_var_removed("AGENT_BINARY", || {
                    with_env_var_set(
                        "AGENT_HARNESS_SKIP_SYSTEM_DISCOVERY_PATHS",
                        OsStr::new("1"),
                        || {
                            reset_discovery_cache();
                            assert!(discover(None).is_none());

                            write_executable(&exe, "#!/bin/sh\nexit 0\n");
                            reset_discovery_cache();
                            let result = discover(None).expect("should discover pi on PATH");
                            match result.launch {
                                AgentLaunch::Binary(path) => assert_eq!(path, exe),
                                AgentLaunch::PackageRunner { .. } => panic!("expected binary launch"),
                            }
                            reset_discovery_cache();
                        },
                    )
                })
            })
        });
    }
}
