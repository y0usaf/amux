use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

use crate::agent;
use crate::notify::Notify;

const READ_BUFFER_SIZE: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalTarget {
    pub pi_binary: Option<String>,
    pub sidecar_extension_path: Option<PathBuf>,
    pub sidecar_socket_path: PathBuf,
    pub tui_mode: Option<String>,
    pub harness_session_id: String,
    pub cwd: PathBuf,
    pub session_file: Option<PathBuf>,
    /// Render the agent's in-terminal rail with the ASCII glyph set
    /// (`agent::ASCII_ENV=1`).
    pub ascii: bool,
    /// Per-symbol rail glyph overrides forwarded as JSON
    /// (`AGENT_HARNESS_SYMBOL_OVERRIDES`).
    pub symbol_overrides: BTreeMap<String, String>,
}

/// Process identity: the sidecar socket the daemon owns plus the harness
/// session the process was launched for. Everything else on a target
/// (cwd, session file, launch flags) can drift without changing which
/// process is live, and the daemon keys sessions by harness session id — so
/// this check must agree with that keying or an adopt is misread as a
/// restart and the client view resets.
pub(crate) fn targets_share_process(
    current: Option<&TerminalTarget>,
    next: Option<&TerminalTarget>,
) -> bool {
    matches!((current, next),
        (Some(current), Some(next))
            if current.sidecar_socket_path == next.sidecar_socket_path
                && current.harness_session_id == next.harness_session_id)
}

pub(crate) struct HostProcess {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub(crate) killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    pub(crate) rx: Receiver<HostEvent>,
    pub(crate) pid: Option<u32>,
}

pub(crate) enum HostEvent {
    Output(Vec<u8>),
    Exited(String),
    Error(String),
}

impl HostProcess {
    pub(crate) fn terminate(&self) -> Result<(), String> {
        let mut killer = self
            .killer
            .lock()
            .map_err(|_| "terminal killer lock poisoned".to_string())?;
        match killer.kill() {
            Ok(()) => Ok(()),
            Err(error) if process_is_missing_error(&error) => Ok(()),
            Err(error) => Err(format!("terminate failed: {error}")),
        }
    }

    pub(crate) fn force_kill(&self) -> Result<(), String> {
        match self.pid {
            Some(pid) => force_kill_pid(pid),
            None => self.terminate(),
        }
    }

    pub(crate) fn wait_for_exit(&mut self, timeout: Duration) -> Result<bool, String> {
        if timeout.is_zero() {
            return Ok(false);
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            match self.rx.recv_timeout(remaining) {
                Ok(HostEvent::Exited(_)) => return Ok(true),
                Ok(HostEvent::Output(_) | HostEvent::Error(_)) => {}
                Err(RecvTimeoutError::Timeout) => return Ok(false),
                Err(RecvTimeoutError::Disconnected) => return Ok(true),
            }
        }
    }
}

impl HostProcess {
    /// Hand the event stream to a sole consumer. The daemon forwarder thread
    /// takes it; after this, `wait_for_exit` must not be used (a second
    /// consumer would race `Exited` delivery).
    pub(crate) fn take_events(&mut self) -> Receiver<HostEvent> {
        std::mem::replace(&mut self.rx, mpsc::channel().1)
    }
}

pub(crate) fn spawn_process(
    target: &TerminalTarget,
    cols: u16,
    rows: u16,
    notify: Notify,
) -> Result<HostProcess, String> {
    // Agent launch flags are pi-shaped: `-e <extension>`, `--session <file>`,
    // `--tui-mode <mode>`. fx accepts none of them (it has no extension host
    // and resumes with `--resume <id>`), so under feature = "fx" the harness
    // launches a bare `fx` in the workspace directory and fx starts a fresh
    // session; row-to-session correlation then happens via the scan instead
    // of at spawn time.
    #[cfg(not(feature = "fx"))]
    let mut args = {
        let mut args = Vec::new();
        if let Some(ref extension_path) = target.sidecar_extension_path {
            args.push("-e".to_string());
            args.push(extension_path.display().to_string());
        }
        if let Some(ref session_file) = target.session_file {
            args.push("--session".to_string());
            args.push(session_file.display().to_string());
        }
        #[cfg(not(feature = "omp"))]
        if let Some(tui_mode) = &target.tui_mode {
            args.push("--tui-mode".to_string());
            args.push(tui_mode.clone());
        }
        args
    };
    #[cfg(feature = "fx")]
    let mut args = Vec::new();
    let argv = agent::launch_argv(target.pi_binary.as_deref(), &args)?;

    let env = spawn_env(target);
    spawn_argv(argv, &target.cwd, &env, cols, rows, notify)
}

/// Adapter- and target-specific environment for spawned agents.
fn spawn_env(target: &TerminalTarget) -> Vec<(String, String)> {
    let mut env = Vec::new();
    env.push((
        agent::SIDECAR_SOCKET_ENV.to_string(),
        target.sidecar_socket_path.display().to_string(),
    ));
    env.push((
        agent::SIDECAR_SESSION_KEY_ENV.to_string(),
        target.harness_session_id.clone(),
    ));
    if target.ascii {
        env.push((agent::ASCII_ENV.to_string(), "1".to_string()));
    }
    if !target.symbol_overrides.is_empty() {
        match serde_json::to_string(&target.symbol_overrides) {
            Ok(json) => env.push(("AGENT_HARNESS_SYMBOL_OVERRIDES".to_string(), json)),
            Err(error) => {
                log::warn!("failed to serialize rail symbol overrides: {error}");
            }
        }
    }
    env
}

/// Spawn an arbitrary argv on a fresh PTY with the harness terminal
/// environment. Split from [`spawn_process`] so daemon tests can drive plain
/// argv without an agent binary.
pub(crate) fn spawn_argv(
    argv: Vec<std::ffi::OsString>,
    cwd: &std::path::Path,
    extra_env: &[(String, String)],
    cols: u16,
    rows: u16,
    notify: Notify,
) -> Result<HostProcess, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let mut cmd = CommandBuilder::from_argv(argv);
    cmd.env_clear();
    for (key, value) in std::env::vars_os() {
        cmd.env(key, value);
    }
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| format!("spawn failed: {error}"))?;
    let pid = child.process_id();
    let killer = Arc::new(Mutex::new(child.clone_killer()));
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("reader clone failed: {error}"))?;
    let writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|error| format!("writer acquire failed: {error}"))?,
    ));

    let (tx, rx) = mpsc::channel();
    let read_tx = tx.clone();
    let read_notify = notify.clone();
    std::thread::Builder::new()
        .name("pi-harness-terminal-reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; READ_BUFFER_SIZE];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if read_tx.send(HostEvent::Output(buf[..n].to_vec())).is_err() {
                            break;
                        }
                        read_notify();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = read_tx.send(HostEvent::Error(error.to_string()));
                        read_notify();
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("reader thread failed: {error}"))?;

    let wait_notify = notify;
    std::thread::Builder::new()
        .name("pi-harness-terminal-wait".into())
        .spawn(move || {
            let status = child
                .wait()
                .map(|status| status.to_string())
                .unwrap_or_else(|error| error.to_string());
            let _ = tx.send(HostEvent::Exited(status));
            wait_notify();
        })
        .map_err(|error| format!("wait thread failed: {error}"))?;

    Ok(HostProcess {
        master: pair.master,
        writer,
        killer,
        rx,
        pid,
    })
}

#[cfg(unix)]
fn force_kill_pid(pid: u32) -> Result<(), String> {
    let result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if process_is_missing_error(&error) {
        return Ok(());
    }
    Err(format!("force kill failed for pid {pid}: {error}"))
}

#[cfg(not(unix))]
fn force_kill_pid(_pid: u32) -> Result<(), String> {
    Err("force kill unavailable on this platform".to_string())
}

fn process_is_missing_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::NotFound {
        return true;
    }
    #[cfg(unix)]
    {
        return error.raw_os_error() == Some(libc::ESRCH);
    }
    #[cfg(not(unix))]
    {
        false
    }
}
