//! Daemon hub: owns agent PTY processes so they survive TUI client exit.
//!
//! One hub thread owns all session state; per-session forwarder threads drain
//! PTY events into it, per-connection reader threads decode client frames into
//! it, and every broadcast goes out through bounded writer queues. Sessions
//! are keyed by the harness `Session.local_id`; `Spawn` is idempotent, so
//! reconnecting clients re-ensure live sessions instead of respawning them.

use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use vt100::Parser;

use super::proto::{SelectionPoint, SelectionSpan, TerminalReplay};
use crate::notify::{noop, Notify};
use crate::sidecar::SidecarListener;
use crate::terminal::TERMINAL_SCROLLBACK;
use crate::terminal::{spawn_process, HostEvent, HostProcess, TerminalTarget};
use std::sync::Arc;

use super::ctl_socket_path;
use super::proto::{self, write_msg, ClientToDaemon, DaemonToClient, SessionInfo};

/// Bounded writer queue per connection; a stalled client cannot stall the hub.
const WRITE_QUEUE_DEPTH: usize = 128;
/// Byte cap on the per-session replay log. A `TerminalReplay` frame base64s
/// this tail, which stays under [`proto::MAX_FRAME_SIZE`]. When the log
/// outgrows twice this cap it is compacted from the front, so the cost is
/// amortized over many output chunks.
const OUTPUT_LOG_LIMIT: usize = 512 * 1024;
/// Graceful then forced kill timeouts for daemon-side stops.
const KILL_GRACEFUL_MS: u64 = 750;
const KILL_FORCE_MS: u64 = 250;

pub fn run_foreground() -> std::io::Result<()> {
    run_foreground_with(spawn_process)
}

/// `--daemon` entry: daemonize (double fork so the surviving process cannot
/// reacquire a controlling terminal), then run the foreground loop. The
/// direct child waits for the daemonized grandchild and exits, so a client
/// that spawned us can reap quickly.
pub fn run_daemonized() -> std::io::Result<()> {
    daemonize_process()?;
    run_foreground_with(spawn_process)
}

/// Reproduce daemonize 0.5's setup: fork, wait-and-exit in the original
/// parent, setsid + chdir + umask, fork again, then redirect stdio so the
/// surviving process holds no controlling terminal.
fn daemonize_process() -> std::io::Result<()> {
    unsafe {
        let first_pid = libc::fork();
        if first_pid < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if first_pid > 0 {
            let mut status = 0;
            let _ = libc::waitpid(first_pid, &mut status, 0);
            std::process::exit(0);
        }
        if libc::setsid() < 0 {
            return Err(std::io::Error::last_os_error());
        }
        std::env::set_current_dir("/")?;
        libc::umask(0o027);
        let second_pid = libc::fork();
        if second_pid < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if second_pid > 0 {
            std::process::exit(0);
        }
    }
    // stdin: /dev/null; stdout/stderr: the daemon log so failures are
    // observable without a controlling terminal.
    let devnull = std::fs::File::open("/dev/null")?;
    let null_fd = devnull.as_raw_fd();
    let log_path = crate::util::app_state_dir().join("daemon.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_fd = log.as_raw_fd();
    unsafe {
        for (fd, source) in [(0, null_fd), (1, log_fd), (2, log_fd)] {
            if libc::dup2(source, fd) < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// Write the daemon pid file so clients can attribute stale sockets.
extern "C" fn signal_handler(_sig: libc::c_int) {
    // Async-signal-safe: a single write to a pipe fd.
    unsafe {
        let pipe_fd = SIGNAL_PIPE_FD.load(Ordering::SeqCst);
        if pipe_fd >= 0 {
            let byte = [1u8];
            let _ = libc::write(pipe_fd, byte.as_ptr() as *const libc::c_void, 1);
        }
    }
}

static SIGNAL_PIPE_FD: AtomicI32 = AtomicI32::new(-1);

/// Restore SIGTERM/SIGINT to defaults and disarm the self-pipe.
unsafe fn restore_signal_handlers() {
    SIGNAL_PIPE_FD.store(-1, Ordering::SeqCst);
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

/// Point SIGTERM/SIGINT at the self-pipe so termination tears down on the
/// hub thread.
unsafe fn install_signal_handlers(pipe_fd: i32) {
    SIGNAL_PIPE_FD.store(pipe_fd, Ordering::SeqCst);
    unsafe {
        let handler = signal_handler as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

pub(crate) fn write_pid_file() -> std::io::Result<()> {
    let path = super::pid_file_path();
    std::fs::write(path, std::process::id().to_string())
}

/// Spawn seam: production resolves argv through the adapter; tests inject a
/// plain-argv spawner to drive real PTY children without an agent binary.
type Spawner = fn(&TerminalTarget, u16, u16, Notify) -> Result<HostProcess, String>;

pub(crate) fn run_foreground_with(spawner: Spawner) -> std::io::Result<()> {
    let path = ctl_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            // Lost the spawn race or a live daemon already holds the socket.
            if UnixStream::connect(&path).is_ok() {
                return Ok(());
            }
            // Stale socket from a crashed daemon: reclaim it.
            let _ = std::fs::remove_file(&path);
            UnixListener::bind(&path)?
        }
        Err(error) => return Err(error),
    };
    write_pid_file()?;
    eprintln!("harness daemon listening on {}", path.display());

    let (hub_tx, hub_rx) = mpsc::channel();
    let accept_hub_tx = hub_tx.clone();
    // SIGTERM/SIGINT -> self-pipe -> Shutdown, so teardown runs on the hub
    // thread instead of inside a signal handler. `sig_tx` lives for this
    // whole function (dropped only after `restore_signal_handlers` below),
    // so the registered fd stays open for the daemon's lifetime.
    let (sig_tx, sig_rx) = UnixStream::pair()?;
    unsafe {
        install_signal_handlers(sig_tx.as_raw_fd());
    }
    let signal_hub_tx = hub_tx.clone();
    std::thread::Builder::new()
        .name("harness-daemon-signals".into())
        .spawn(move || {
            let mut buf = [0u8; 1];
            let mut reader = sig_rx;
            loop {
                match reader.read(&mut buf) {
                    Ok(1) => {
                        let _ = signal_hub_tx.send(HubEvent::Shutdown);
                    }
                    _ => return,
                }
            }
        })
        .expect("signal thread");
    let _accept = std::thread::Builder::new()
        .name("harness-daemon-accept".into())
        .spawn(move || {
            for incoming in listener.incoming() {
                match incoming {
                    Ok(stream) => {
                        if accept_hub_tx.send(HubEvent::NewClient(stream)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("daemon accept failed: {error}");
                        break;
                    }
                }
            }
        })
        .expect("accept thread");

    // Agent sidechannel: agents dial this socket; inbound lines relay to
    // attached clients, hello/broadcast lines flow back.
    let sidecar_notify_tx = hub_tx.clone();
    let sidecar_notify: Notify = Arc::new(move || {
        let _ = sidecar_notify_tx.send(HubEvent::SidecarReady);
    });
    // Fatal: without the sidecar socket agents cannot report state, so the
    // daemon would be useless.
    let sidecar = SidecarListener::start(sidecar_notify, super::sidecar_socket_path())
        .map_err(|error| std::io::Error::other(format!("sidecar socket unavailable: {error}")))?;

    Hub {
        spawner,
        sidecar: Some(sidecar),
        ..Hub::default()
    }
    .run(hub_rx, hub_tx);
    // Undo process-global signal state; a stale handler writing to a closed
    // (or worse, reused) fd after daemon exit is not acceptable.
    unsafe { restore_signal_handlers() };
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(super::pid_file_path());
    Ok(())
}

struct Hub {
    spawner: Spawner,
    /// Hosts the agent sidechannel socket; inbound agent lines are relayed to
    /// attached clients verbatim, outbound client lines go to extensions.
    sidecar: Option<SidecarListener>,
    sessions: HashMap<String, SessionState>,
    connections: HashMap<u64, SyncSender<DaemonToClient>>,
    next_conn_id: u64,
    /// Ring buffer of validated sidecar lines, replayed to each newly
    /// attached client so late joiners see the agent state early clients got
    /// live.
    sidecar_history: std::collections::VecDeque<String>,
    /// Bounded sidecar history: at most this many raw lines are replayed to
    /// a newly attached client.
    sidecar_history_cap: usize,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            spawner: spawn_process,
            sidecar: None,
            sessions: HashMap::new(),
            connections: HashMap::new(),
            next_conn_id: 0,
            sidecar_history: std::collections::VecDeque::new(),
            sidecar_history_cap: 256,
        }
    }
}
struct SessionState {
    process: Option<HostProcess>,
    /// Signalled by the forwarder when the child exits; kills wait here
    /// because the forwarder is the sole event-stream consumer.
    exit_signal: mpsc::Receiver<()>,
    /// Status string of the last exited child, None while running.
    exit_status: Option<String>,
    /// Authoritative view state: every attached client renders from a parser
    /// reconstructed by replaying this session's output log.
    view: SessionView,
}

/// Daemon-owned terminal view: the live parser/grid plus the bounded output
/// log replays are built from, and the last reported selection span.
struct SessionView {
    parser: Parser,
    log: Vec<u8>,
    selection: Option<SelectionSpan>,
}

impl SessionView {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::new(rows.max(1), cols.max(1), TERMINAL_SCROLLBACK),
            log: Vec::new(),
            selection: None,
        }
    }

    /// Fold a PTY output chunk into the authoritative view.
    fn record_output(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        self.log.extend_from_slice(bytes);
        // Compact only when the log doubles past the cap, so front-draining
        // stays amortized instead of running on every chunk.
        if self.log.len() >= OUTPUT_LOG_LIMIT * 2 {
            let keep_from = self.log.len() - OUTPUT_LOG_LIMIT;
            self.log.drain(..keep_from);
        }
    }

    fn log_tail(&self) -> &[u8] {
        let start = self.log.len().saturating_sub(OUTPUT_LOG_LIMIT);
        &self.log[start..]
    }

    /// Snapshot for an attaching client, clamping a stale selection span to
    /// the live grid bounds.
    fn replay(&self) -> TerminalReplay {
        let (rows, cols) = self.parser.screen().size();
        let selection = self.selection.map(|span| clamp_span(span, rows, cols));
        TerminalReplay {
            rows,
            cols,
            selection,
            log: BASE64.encode(self.log_tail()),
        }
    }
}

fn clamp_span(span: SelectionSpan, rows: u16, cols: u16) -> SelectionSpan {
    let clamp_point = |point: SelectionPoint| SelectionPoint {
        row: point.row.min(rows.saturating_sub(1)),
        col: point.col.min(cols),
    };
    SelectionSpan {
        start: clamp_point(span.start),
        end: clamp_point(span.end),
    }
}

enum HubEvent {
    NewClient(UnixStream),
    ClientGone(u64),
    ClientMessage(u64, ClientToDaemon),
    SessionEvent {
        session_id: String,
        event: SessionEvent,
    },
    SidecarReady,
    Shutdown,
}

enum SessionEvent {
    Output(Vec<u8>),
    Error(String),
    Exited(String),
}

impl Hub {
    fn run(mut self, hub_rx: mpsc::Receiver<HubEvent>, hub_tx: mpsc::Sender<HubEvent>) {
        loop {
            let event = match hub_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    if self.idle() {
                        eprintln!("harness daemon idle: no clients, no live sessions");
                        return;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            };
            match event {
                HubEvent::NewClient(stream) => self.on_new_client(stream, &hub_tx),
                HubEvent::ClientGone(id) => {
                    self.connections.remove(&id);
                }
                HubEvent::ClientMessage(id, message) => {
                    self.on_client_message(id, message, &hub_tx)
                }
                HubEvent::SessionEvent { session_id, event } => {
                    self.on_session_event(session_id, event)
                }
                HubEvent::SidecarReady => self.drain_sidecar(),
                HubEvent::Shutdown => {
                    self.kill_all();
                    return;
                }
            }
        }
    }

    fn idle(&self) -> bool {
        let has_live = self
            .sessions
            .values()
            .any(|session| session.process.is_some());
        self.connections.is_empty() && !has_live
    }

    fn on_new_client(&mut self, stream: UnixStream, hub_tx: &mpsc::Sender<HubEvent>) {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        let (tx, rx) = mpsc::sync_channel::<DaemonToClient>(WRITE_QUEUE_DEPTH);
        self.connections.insert(id, tx);

        // Reader thread: decodes frames into the hub.
        let reader_hub_tx = hub_tx.clone();
        let reader_stream = match stream.try_clone() {
            Ok(reader) => reader,
            Err(_) => {
                self.connections.remove(&id);
                return;
            }
        };
        let _ = std::thread::Builder::new()
            .name("harness-daemon-reader".into())
            .spawn(move || {
                let mut reader = reader_stream;
                loop {
                    match proto::read_msg::<_, ClientToDaemon>(&mut reader) {
                        Ok(Some(message)) => {
                            if reader_hub_tx
                                .send(HubEvent::ClientMessage(id, message))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                let _ = reader_hub_tx.send(HubEvent::ClientGone(id));
            })
            .expect("reader thread");

        // Writer thread: drains the bounded queue into the socket.
        let mut writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(_) => {
                self.connections.remove(&id);
                return;
            }
        };
        let _ = std::thread::Builder::new()
            .name("harness-daemon-writer".into())
            .spawn(move || {
                for message in rx {
                    if write_msg(&mut writer, &message).is_err() {
                        break;
                    }
                }
            })
            .expect("writer thread");
    }

    fn on_client_message(
        &mut self,
        conn: u64,
        message: ClientToDaemon,
        hub_tx: &mpsc::Sender<HubEvent>,
    ) {
        match message {
            ClientToDaemon::Hello { wire_version } => {
                let reply = if wire_version == proto::WIRE_VERSION {
                    DaemonToClient::Welcome {
                        wire_version: proto::WIRE_VERSION,
                    }
                } else {
                    DaemonToClient::Rejected {
                        reason: format!(
                            "wire protocol version mismatch: client {wire_version}, daemon {}",
                            proto::WIRE_VERSION
                        ),
                    }
                };
                self.send(conn, reply);
            }
            ClientToDaemon::Ping => self.send(conn, DaemonToClient::Pong),
            ClientToDaemon::Spawn {
                req_id,
                session_id,
                target,
                rows,
                cols,
            } => self.spawn_session(conn, req_id, session_id, target, rows, cols, hub_tx),
            ClientToDaemon::Input { session_id, bytes } => match BASE64.decode(bytes.as_bytes()) {
                Ok(bytes) => self.write_input(&session_id, &bytes),
                Err(error) => self.send(
                    conn,
                    DaemonToClient::Error {
                        req_id: 0,
                        session_id: Some(session_id),
                        message: format!("bad input encoding: {error}"),
                    },
                ),
            },
            ClientToDaemon::Resize {
                session_id,
                rows,
                cols,
            } => self.resize_session(&session_id, rows, cols),
            ClientToDaemon::Kill {
                req_id,
                session_id,
                graceful_ms,
                force_ms,
            } => {
                let status = self.kill_session(&session_id, graceful_ms, force_ms);
                let reply = match status {
                    Ok(()) => DaemonToClient::Killed { req_id, session_id },
                    Err(message) => DaemonToClient::Error {
                        req_id,
                        session_id: Some(session_id),
                        message,
                    },
                };
                self.send(conn, reply);
            }
            ClientToDaemon::SetSelection {
                session_id,
                selection,
            } => {
                if let Some(state) = self.sessions.get_mut(&session_id) {
                    state.view.selection = selection;
                }
            }
            ClientToDaemon::SetHello { line } => {
                if let Some(sidecar) = &self.sidecar {
                    sidecar.set_hello(line);
                }
            }
            ClientToDaemon::BroadcastLine { line } => {
                if let Some(sidecar) = &self.sidecar {
                    sidecar.broadcast(&line);
                }
            }
            ClientToDaemon::ListSessions { req_id } => {
                let sessions = self
                    .sessions
                    .iter()
                    .map(|(id, state)| SessionInfo {
                        id: id.clone(),
                        pid: state.process.as_ref().and_then(|process| process.pid),
                        running: state.process.is_some(),
                        exit_status: state.exit_status.clone(),
                    })
                    .collect();
                self.send(conn, DaemonToClient::Sessions { req_id, sessions });
            }
        }
    }

    fn spawn_session(
        &mut self,
        conn: u64,
        req_id: u64,
        session_id: String,
        target: TerminalTarget,
        rows: u16,
        cols: u16,
        hub_tx: &mpsc::Sender<HubEvent>,
    ) {
        // Canonical identity: sessions claiming the same resolved session
        // file share one daemon session, so a second client attaching to the
        // same underlying agent lands on the existing entry instead of
        // double-spawning the pi session file.
        let key = self.resolve_key(&session_id, &target);
        let already_running = self
            .sessions
            .get(&key)
            .is_some_and(|session| session.process.is_some());
        if !already_running {
            match self.spawn_child(&key, target, rows, cols, hub_tx) {
                Ok(()) => {}
                Err(message) => {
                    self.send(
                        conn,
                        DaemonToClient::Error {
                            req_id,
                            session_id: Some(key),
                            message,
                        },
                    );
                    return;
                }
            }
        }
        let state = &self.sessions[&key];
        // Discovery: announce the daemon's canonical identity for the session
        // so every attached client can converge on one row.
        self.broadcast(DaemonToClient::SessionOpened {
            id: key.clone(),
            pid: state.process.as_ref().and_then(|process| process.pid),
        });
        // Replay rides the Spawned reply itself, so the ordering between
        // restored history and subsequent live output is atomic on the wire.
        let replay = already_running.then(|| state.view.replay());
        // The canonical key rides every Spawned reply so a client that keyed
        // the session differently (draft uuid, scan-discovered id) can rebind
        // its workspace row to the daemon's stable identity.
        self.send(
            conn,
            DaemonToClient::Spawned {
                req_id,
                session_id: key,
                pid: state.process.as_ref().and_then(|process| process.pid),
                already_running,
                exit_status: state.exit_status.clone(),
                replay,
            },
        );
    }

    /// Resolve a spawn request's canonical session key: the resolved
    /// (canonicalized) session file when the target carries one, so every
    /// client naming the same underlying pi session file converges on one
    /// daemon session; the client-supplied key otherwise.
    fn resolve_key(&self, session_id: &str, target: &TerminalTarget) -> String {
        match target.session_file.as_ref().map(|path| {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.clone())
        }) {
            Some(session_file) => session_file.to_string_lossy().into_owned(),
            None => session_id.to_string(),
        }
    }

    fn spawn_child(
        &mut self,
        session_id: &str,
        target: TerminalTarget,
        rows: u16,
        cols: u16,
        hub_tx: &mpsc::Sender<HubEvent>,
    ) -> Result<(), String> {
        let mut process = (self.spawner)(&target, cols, rows, noop())?;
        let (exit_signal_tx, exit_signal) = mpsc::channel();

        // Forwarder thread: sole consumer of the PTY event stream; relays
        // events into the hub and signals exits for kill barriers.
        let rx = process.take_events();
        let forward_hub_tx = hub_tx.clone();
        let forward_id = session_id.to_string();
        std::thread::Builder::new()
            .name("harness-daemon-forward".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    let event = match event {
                        HostEvent::Output(bytes) => SessionEvent::Output(bytes),
                        HostEvent::Error(message) => SessionEvent::Error(message),
                        HostEvent::Exited(status) => {
                            let _ = exit_signal_tx.send(());
                            SessionEvent::Exited(status)
                        }
                    };
                    if forward_hub_tx
                        .send(HubEvent::SessionEvent {
                            session_id: forward_id.clone(),
                            event,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| format!("forwarder thread failed: {error}"))?;

        self.sessions.insert(
            session_id.to_string(),
            SessionState {
                process: Some(process),
                exit_signal,
                exit_status: None,
                // Fresh process: a fresh authoritative view. The previous
                // view belonged to the replaced child and is not history the
                // new child can continue.
                view: SessionView::new(rows, cols),
            },
        );
        Ok(())
    }

    fn write_input(&self, session_id: &str, bytes: &[u8]) {
        if let Some(process) = self
            .sessions
            .get(session_id)
            .and_then(|s| s.process.as_ref())
        {
            if let Ok(mut writer) = process.writer.lock() {
                let _ = writer.write_all(bytes);
                let _ = writer.flush();
            }
        }
    }

    fn resize_session(&mut self, session_id: &str, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let Some(state) = self.sessions.get_mut(session_id) else {
            return;
        };
        if let Some(process) = state.process.as_mut() {
            let _ = process.master.resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        // Keep the authoritative grid in lockstep with the PTY so replays
        // carry the live size.
        state.view.parser.screen_mut().set_size(rows, cols);
    }

    /// terminate -> wait graceful -> force-kill -> wait force (the old
    /// `stop_and_wait` semantics, now daemon-side).
    fn kill_session(
        &mut self,
        session_id: &str,
        graceful_ms: u64,
        force_ms: u64,
    ) -> Result<(), String> {
        // Keep the entry (view included) instead of removing it: a restart
        // cycle kills here and respawns right after, and the retained state
        // means a client reattaching before the respawn still sees the last
        // screen rather than an empty one. The next spawn replaces the view.
        let Some(state) = self.sessions.get_mut(session_id) else {
            return Ok(());
        };
        let Some(process) = state.process.take() else {
            return Ok(());
        };
        process
            .terminate()
            .map_err(|error| format!("signalling terminal process: {error}"))?;
        // Borrow note: `exit_signal` is moved out so `state` (holding the
        // retained view) stays borrowed for the rest of the wait.
        let exit_signal = std::mem::replace(&mut state.exit_signal, mpsc::channel().1);
        match exit_signal.recv_timeout(Duration::from_millis(graceful_ms)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        process
            .force_kill()
            .map_err(|error| format!("force killing terminal process: {error}"))?;
        if exit_signal
            .recv_timeout(Duration::from_millis(force_ms))
            .is_err()
        {
            return Err(format!(
                "terminal process {session_id} did not exit after force kill"
            ));
        }
        Ok(())
    }

    /// Graceful teardown of every live child (SIGTERM path).
    fn kill_all(&mut self) {
        let session_ids: Vec<String> = self.sessions.keys().cloned().collect();
        for session_id in session_ids {
            let _ = self.kill_session(&session_id, KILL_GRACEFUL_MS, KILL_FORCE_MS);
        }
    }

    fn on_session_event(&mut self, session_id: String, event: SessionEvent) {
        match event {
            SessionEvent::Output(bytes) => {
                // Feed the authoritative view only while a child owns the
                // session: trailing bytes that race a kill/respawn must not
                // corrupt the replacement child's fresh view.
                if let Some(state) = self.sessions.get_mut(&session_id) {
                    if state.process.is_some() {
                        state.view.record_output(&bytes);
                    }
                }
                let encoded = BASE64.encode(&bytes);
                self.broadcast(DaemonToClient::Output {
                    session_id,
                    bytes: encoded,
                });
            }
            SessionEvent::Error(message) => {
                self.broadcast(DaemonToClient::Error {
                    req_id: 0,
                    session_id: Some(session_id),
                    message,
                });
            }
            SessionEvent::Exited(status) => {
                if let Some(state) = self.sessions.get_mut(&session_id) {
                    state.process = None;
                    state.exit_status = Some(status.clone());
                }
                self.broadcast(DaemonToClient::Exited {
                    session_id: session_id.clone(),
                    status,
                });
                // Discovery: the daemon session is gone even though its
                // retained entry (exit status, view) stays for reattach.
                self.broadcast(DaemonToClient::SessionClosed { id: session_id });
            }
        }
    }

    /// Relay validated agent lines to every attached client verbatim, and
    /// keep a bounded ring so newly attached clients can be backfilled.
    fn drain_sidecar(&mut self) {
        while let Some(sidecar) = &self.sidecar {
            let Some(line) = sidecar.try_recv_raw() else {
                break;
            };
            self.sidecar_history.push_back(line.clone());
            if self.sidecar_history.len() > self.sidecar_history_cap {
                self.sidecar_history.pop_front();
            }
            self.broadcast(DaemonToClient::SidecarLine { line });
        }
    }

    fn send(&self, conn: u64, message: DaemonToClient) {
        // Blocking send: the writer thread drains continuously, and a dead
        // connection's reader EOF makes the hub drop this sender, failing
        // the next send immediately.
        if let Some(tx) = self.connections.get(&conn) {
            let _ = tx.send(message);
        }
    }

    fn broadcast(&self, message: DaemonToClient) {
        for tx in self.connections.values() {
            match tx.try_send(message.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Instant;

    fn temp_runtime_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("harness-daemon-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn test_spawn(
        _target: &TerminalTarget,
        cols: u16,
        rows: u16,
        notify: Notify,
    ) -> Result<HostProcess, String> {
        // Interactive shell: reads stdin, so Input round-trips to Output.
        let argv: Vec<OsString> = vec!["/bin/sh".into()];
        crate::terminal::spawn_argv(argv, std::path::Path::new("/tmp"), &[], cols, rows, notify)
    }

    pub(super) fn connect_and_hello(path: &std::path::Path) -> UnixStream {
        // The daemon binds asynchronously; poll for the socket like the
        // client's spawn path will.
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match UnixStream::connect(path) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("daemon socket never appeared: {error}"),
            }
        };
        let mut stream = stream;
        proto::write_msg(
            &mut stream,
            &ClientToDaemon::Hello {
                wire_version: proto::WIRE_VERSION,
            },
        )
        .unwrap();
        let welcome: DaemonToClient = proto::read_msg(&mut stream).unwrap().unwrap();
        assert_eq!(
            welcome,
            DaemonToClient::Welcome {
                wire_version: proto::WIRE_VERSION
            }
        );
        stream
    }

    fn spawn_session(stream: &mut UnixStream, session_id: &str) -> (Option<u32>, bool) {
        proto::write_msg(
            stream,
            &ClientToDaemon::Spawn {
                req_id: 1,
                session_id: session_id.to_string(),
                target: test_target(),
                rows: 32,
                cols: 100,
            },
        )
        .unwrap();
        loop {
            match proto::read_msg::<_, DaemonToClient>(stream)
                .unwrap()
                .unwrap()
            {
                DaemonToClient::Spawned {
                    session_id: id,
                    pid,
                    already_running,
                    ..
                } => {
                    assert_eq!(id, session_id);
                    return (pid, already_running);
                }
                DaemonToClient::Output { .. } => {}
                // Discovery broadcasts ride along with every spawn; ignore
                // them while waiting for the Spawned reply.
                DaemonToClient::SessionOpened { .. }
                | DaemonToClient::SessionClosed { .. } => {}
                other => panic!("unexpected message while spawning: {other:?}"),
            }
        }
    }

    fn test_target() -> TerminalTarget {
        serde_json::from_value(serde_json::json!({
            "pi_binary": null,
            "sidecar_extension_path": null,
            "sidecar_socket_path": "/tmp/test-sidecar.sock",
            "tui_mode": null,
            "harness_session_id": "t1",
            "cwd": "/tmp",
            "session_file": null,
            "ascii": false,
            "symbol_overrides": {}
        }))
        .unwrap()
    }

    fn read_until(stream: &mut UnixStream, needle: &str) -> Vec<DaemonToClient> {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buffer = String::new();
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        while Instant::now() < deadline {
            match proto::read_msg::<_, DaemonToClient>(stream) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Ok(Some(message)) => {
                    if let DaemonToClient::Output { bytes, .. } = &message {
                        if let Ok(decoded) = BASE64.decode(bytes.as_bytes()) {
                            buffer.push_str(&String::from_utf8_lossy(&decoded));
                        }
                    }
                    seen.push(message);
                    if buffer.contains(needle) {
                        return seen;
                    }
                }
                Ok(None) => panic!("daemon closed connection while waiting for {needle:?}"),
                Err(error) => panic!("read error while waiting for {needle:?}: {error}"),
            }
        }
        panic!("timed out waiting for {needle:?}");
    }

    fn pid_alive(pid: u32) -> bool {
        std::path::Path::new("/proc").join(pid.to_string()).exists()
    }

    /// Restores XDG_RUNTIME_DIR on drop; tests share one process env.
    pub(super) struct EnvGuard {
        previous: Option<std::ffi::OsString>,
        runtime_dir: PathBuf,
    }

    impl EnvGuard {
        pub(super) fn set(tag: &str) -> Self {
            let previous = std::env::var_os("XDG_RUNTIME_DIR");
            let runtime_dir = temp_runtime_dir(tag);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
            Self {
                previous,
                runtime_dir,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("XDG_RUNTIME_DIR", previous),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.runtime_dir);
        }
    }

    #[test]
    fn child_survives_client_disconnect_and_reattaches() {
        use std::io::{BufRead, BufReader};
        let _env = env_lock();
        let _runtime = EnvGuard::set("survival");
        let path = ctl_socket_path();

        let daemon = std::thread::Builder::new()
            .name("harness-daemon-test".into())
            .spawn(move || run_foreground_with(test_spawn))
            .unwrap();

        // Client A: spawn the session, observe its output.
        let mut client_a = connect_and_hello(&path);
        let (pid, already_running) = spawn_session(&mut client_a, "t1");
        let pid = pid.expect("spawned child has a pid");
        assert!(!already_running);
        proto::write_msg(
            &mut client_a,
            &ClientToDaemon::Input {
                session_id: "t1".into(),
                bytes: BASE64.encode(b"echo alive-marker\n"),
            },
        )
        .unwrap();
        read_until(&mut client_a, "alive-marker");

        // A dies without saying goodbye.
        drop(client_a);
        std::thread::sleep(Duration::from_millis(500));
        assert!(pid_alive(pid), "child must outlive its client");

        // Client B reconnects: same session, already running, same pid.
        let mut client_b = connect_and_hello(&path);
        let (pid_b, already_running_b) = spawn_session(&mut client_b, "t1");
        assert_eq!(pid_b, Some(pid));
        assert!(already_running_b);

        // The reattach reply carries the daemon's authoritative view: the
        // screen contents B missed while disconnected must be replayable.
        let replay = {
            // Re-run the spawn exchange to capture the full Spawned frame.
            proto::write_msg(
                &mut client_b,
                &ClientToDaemon::Spawn {
                    req_id: 3,
                    session_id: "t1".into(),
                    target: test_target(),
                    rows: 32,
                    cols: 100,
                },
            )
            .unwrap();
            loop {
                match proto::read_msg::<_, DaemonToClient>(&mut client_b)
                    .unwrap()
                    .unwrap()
                {
                    DaemonToClient::Spawned {
                        req_id: 3, replay, ..
                    } => break replay,
                    DaemonToClient::Output { .. } => {}
                    DaemonToClient::SessionOpened { .. }
                    | DaemonToClient::SessionClosed { .. } => {}
                    other => panic!("unexpected while awaiting replay spawn: {other:?}"),
                }
            }
        }
        .expect("reattaching to a live session carries a replay");
        let restored = BASE64.decode(replay.log.as_bytes()).unwrap();
        let restored_text = String::from_utf8_lossy(&restored);
        assert!(
            restored_text.contains("alive-marker"),
            "replay must restore pre-disconnect screen, got: {restored_text:?}"
        );
        assert!(replay.rows > 0 && replay.cols > 0);

        // B talks to the same live child.
        proto::write_msg(
            &mut client_b,
            &ClientToDaemon::Input {
                session_id: "t1".into(),
                bytes: BASE64.encode(b"echo reattached-$((1+1))\n"),
            },
        )
        .unwrap();
        read_until(&mut client_b, "reattached-2");

        // Explicit kill ends the child.
        proto::write_msg(
            &mut client_b,
            &ClientToDaemon::Kill {
                req_id: 2,
                session_id: "t1".into(),
                graceful_ms: 750,
                force_ms: 250,
            },
        )
        .unwrap();
        loop {
            match proto::read_msg::<_, DaemonToClient>(&mut client_b)
                .unwrap()
                .unwrap()
            {
                DaemonToClient::Killed { session_id, .. } => {
                    assert_eq!(session_id, "t1");
                    break;
                }
                DaemonToClient::Output { .. } => {}
                other => panic!("unexpected message while killing: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(!pid_alive(pid), "child must be gone after Kill");

        // Sidecar: a fake agent dials the daemon-owned sidecar socket; its
        // snapshot line relays verbatim to the attached client, and the
        // client's hello line reaches the agent.
        client_b
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let agent = {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match UnixStream::connect(super::super::sidecar_socket_path()) {
                    Ok(stream) => break stream,
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => panic!("sidecar socket never appeared: {error}"),
                }
            }
        };
        let snapshot = r#"{"type":"snapshot","sessionId":"pi-1","harnessSessionId":"t1","stage":"idle","tsMs":123}"#;
        let agent_writer = agent.try_clone().unwrap();
        std::thread::spawn(move || {
            use std::io::Write;
            let mut writer = agent_writer;
            let _ = writeln!(writer, "{snapshot}");
        });
        let relay_deadline = Instant::now() + Duration::from_secs(5);
        let relayed = loop {
            if Instant::now() > relay_deadline {
                panic!("timed out awaiting sidecar relay");
            }
            match proto::read_msg::<_, DaemonToClient>(&mut client_b) {
                Ok(Some(DaemonToClient::SidecarLine { line })) => break line,
                Ok(Some(DaemonToClient::Output { .. })) => {}
                Ok(Some(DaemonToClient::Exited { .. })) => {}
                Ok(Some(DaemonToClient::SessionOpened { .. })) => {}
                Ok(Some(DaemonToClient::SessionClosed { .. })) => {}
                Ok(Some(other)) => {
                    panic!("unexpected while awaiting sidecar relay: {other:?}")
                }
                Ok(None) => panic!("daemon closed while awaiting sidecar relay"),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => panic!("sidecar relay read error: {error}"),
            }
        };
        assert_eq!(relayed, snapshot);

        proto::write_msg(
            &mut client_b,
            &ClientToDaemon::SetHello {
                line: "w=12".into(),
            },
        )
        .unwrap();
        let mut agent_reader = BufReader::new(agent);
        let mut hello = String::new();
        agent_reader.read_line(&mut hello).unwrap();
        assert_eq!(hello.trim(), "w=12");

        // No clients, no live sessions: the daemon exits on its own.
        drop(client_b);
        daemon.join().unwrap().expect("daemon exits cleanly");
    }
}

#[cfg(test)]
mod client_disconnect_tests {
    use super::tests::EnvGuard;
    use super::*;
    use crate::daemon::client::DaemonClient;
    use crate::test_support::env_lock;
    use std::time::Instant;

    /// A kill RPC whose daemon dies mid-request fails fast instead of
    /// deadlocking: the reader thread's exit must fail all in-flight
    /// requests.
    #[test]
    fn in_flight_request_fails_when_daemon_dies() {
        let _env = env_lock();
        let _runtime = EnvGuard::set("disconnect");
        let path = ctl_socket_path();

        // Socket pair acting as client<->daemon.
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (client_stream, server_stream) =
            UnixStream::pair().expect("socketpair available on unix");

        let notify = crate::notify::noop();
        let client = DaemonClient::attached_post_handshake(client_stream, notify).unwrap();

        // Fake daemon: reply to the spawn, then die without answering the
        // kill — exactly a daemon crash while an RPC is in flight.
        let killer = std::thread::spawn(move || {
            let mut server = server_stream;
            loop {
                match proto::read_msg::<_, ClientToDaemon>(&mut server) {
                    Ok(Some(ClientToDaemon::Spawn { req_id, .. })) => {
                        let _ = write_msg(
                            &mut server,
                            &DaemonToClient::Spawned {
                                req_id,
                                session_id: "t1".into(),
                                pid: Some(4242),
                                already_running: false,
                                exit_status: None,
                                replay: None,
                            },
                        );
                    }
                    Ok(Some(ClientToDaemon::Kill { .. })) => {
                        // Crash: no reply, connection gone.
                        let _ = server.shutdown(std::net::Shutdown::Both);
                        return;
                    }
                    _ => return,
                }
            }
        });

        let _outcome = client
            .spawn("t1", &test_target_for_disconnect(), 32, 100)
            .unwrap();

        let started = Instant::now();
        let result = client.kill("t1", 50, 50);
        assert!(elapsed_under(&started, Duration::from_secs(5)));
        assert!(result.is_err(), "kill must fail when the daemon dies");
        killer.join().unwrap();
    }

    fn elapsed_under(started: &Instant, limit: Duration) -> bool {
        started.elapsed() < limit
    }

    fn test_target_for_disconnect() -> TerminalTarget {
        serde_json::from_value(serde_json::json!({
            "pi_binary": null,
            "sidecar_extension_path": null,
            "sidecar_socket_path": "/tmp/test-sidecar.sock",
            "tui_mode": null,
            "harness_session_id": "t1",
            "cwd": "/tmp",
            "session_file": null,
            "ascii": false,
            "symbol_overrides": {}
        }))
        .unwrap()
    }
}

#[cfg(test)]
mod client_integration_tests {
    use super::tests::{test_spawn, EnvGuard};
    use super::*;
    use crate::daemon::client::DaemonClient;
    use crate::test_support::env_lock;
    use std::time::Instant;

    /// Full client path: connect_or_spawn against a live daemon, session
    /// spawn, output round trip, sidecar parse flow.
    #[test]
    fn daemon_client_round_trips_against_live_daemon() {
        let _env = env_lock();
        let _runtime = EnvGuard::set("client-integration");
        let path = ctl_socket_path();

        let daemon = std::thread::Builder::new()
            .name("harness-daemon-integration".into())
            .spawn(move || run_foreground_with(test_spawn))
            .unwrap();

        // Wait for the daemon thread's socket so connect_or_spawn takes the
        // fast path instead of spawning the test binary with `--daemon`.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(Instant::now() < deadline, "daemon socket never appeared");
            std::thread::sleep(Duration::from_millis(20));
        }

        let notify = crate::notify::noop();
        let t_connect = Instant::now();
        let client = DaemonClient::connect_or_spawn(notify).unwrap();
        eprintln!("[timing] connect: {:?}", t_connect.elapsed());

        // Spawn through the public client API.
        let target: TerminalTarget = serde_json::from_value(serde_json::json!({
            "pi_binary": null,
            "sidecar_extension_path": null,
            "sidecar_socket_path": "/tmp/test-sidecar.sock",
            "tui_mode": null,
            "harness_session_id": "t1",
            "cwd": "/tmp",
            "session_file": null,
            "ascii": false,
            "symbol_overrides": {}
        }))
        .unwrap();
        let t_spawn = Instant::now();
        let outcome = client.spawn("t1", &target, 32, 100).unwrap();
        eprintln!("[timing] spawn: {:?}", t_spawn.elapsed());
        assert!(!outcome.already_running);
        assert!(outcome.replay.is_none(), "fresh spawn has no replay");
        let pid = outcome.pid.expect("child pid");
        assert!(std::path::Path::new("/proc").join(pid.to_string()).exists());

        // Input round-trips through the same wire: ask the shell to exit.
        let t_exit = Instant::now();
        client.input("t1", b"exit\n");
        std::thread::sleep(Duration::from_millis(500));
        eprintln!("[timing] input+sleep: {:?}", t_exit.elapsed());
        assert!(
            !std::path::Path::new("/proc").join(pid.to_string()).exists(),
            "child must exit after shell exit"
        );

        // Kill RPC on the already-dead session is a clean no-op.
        let t_kill = Instant::now();
        client.kill("t1", 750, 250).unwrap();
        eprintln!("[timing] kill: {:?}", t_kill.elapsed());

        let t_shutdown = Instant::now();
        drop(client);
        daemon.join().unwrap().expect("daemon exits cleanly");
        eprintln!("[timing] daemon shutdown: {:?}", t_shutdown.elapsed());
    }
}
