use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use winit::event_loop::EventLoopProxy;

use crate::pi;

const READ_BUFFER_SIZE: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalTarget {
    pub pi_binary: Option<String>,
    pub sidecar_extension_path: Option<PathBuf>,
    pub sidecar_socket_path: PathBuf,
    pub harness_session_id: String,
    pub cwd: PathBuf,
    pub session_file: Option<PathBuf>,
}

pub(crate) fn targets_share_process(
    current: Option<&TerminalTarget>,
    next: Option<&TerminalTarget>,
) -> bool {
    matches!((current, next),
        (Some(current), Some(next))
            if current.pi_binary == next.pi_binary
                && current.sidecar_extension_path == next.sidecar_extension_path
                && current.sidecar_socket_path == next.sidecar_socket_path
                && current.harness_session_id == next.harness_session_id
                && current.cwd == next.cwd
                && session_files_share_process(current.session_file.as_deref(), next.session_file.as_deref()))
}

fn session_files_share_process(
    current: Option<&std::path::Path>,
    next: Option<&std::path::Path>,
) -> bool {
    current == next || current.is_none() || next.is_none()
}

pub(crate) struct HostProcess {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub(crate) killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    pub(crate) rx: Receiver<HostEvent>,
}

pub(crate) enum HostEvent {
    Output(Vec<u8>),
    Exited(String),
    Error(String),
}

pub(crate) fn spawn_process(
    target: &TerminalTarget,
    cols: u16,
    rows: u16,
    proxy: EventLoopProxy<()>,
) -> Result<HostProcess, String> {
    let mut args = Vec::new();
    if let Some(ref extension_path) = target.sidecar_extension_path {
        args.push("-e".to_string());
        args.push(extension_path.display().to_string());
    }
    if let Some(ref session_file) = target.session_file {
        args.push("--session".to_string());
        args.push(session_file.display().to_string());
    }
    let argv = pi::launch_argv(target.pi_binary.as_deref(), &args)?;

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
    cmd.cwd(&target.cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env(
        pi::PI_SIDECAR_SOCKET_ENV,
        target.sidecar_socket_path.display().to_string(),
    );
    cmd.env(pi::PI_SIDECAR_SESSION_KEY_ENV, &target.harness_session_id);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| format!("spawn failed: {error}"))?;
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
    let read_proxy = proxy.clone();
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
                        let _ = read_proxy.send_event(());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = read_tx.send(HostEvent::Error(error.to_string()));
                        let _ = read_proxy.send_event(());
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("reader thread failed: {error}"))?;

    let wait_proxy = proxy;
    std::thread::Builder::new()
        .name("pi-harness-terminal-wait".into())
        .spawn(move || {
            let status = child
                .wait()
                .map(|status| status.to_string())
                .unwrap_or_else(|error| error.to_string());
            let _ = tx.send(HostEvent::Exited(status));
            let _ = wait_proxy.send_event(());
        })
        .map_err(|error| format!("wait thread failed: {error}"))?;

    Ok(HostProcess {
        master: pair.master,
        writer,
        killer,
        rx,
    })
}
