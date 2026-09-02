//! Daemon hub: owns agent PTY processes so they survive TUI client exit.
//!
//! One hub thread owns all session state; per-session forwarder threads drain
//! PTY events into it, per-connection reader threads decode client frames into
//! it, and every broadcast goes out through bounded writer queues. Sessions
//! are keyed by the harness `Session.local_id`; `Spawn` is idempotent, so
//! reconnecting clients re-ensure live sessions instead of respawning them.

use std::collections::{HashMap, HashSet};
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
    connections: HashMap<u64, SyncSender<DaemonToClient>>,
    sessions: HashMap<String, SessionState>,
    /// Connections whose `Hello` the hub has processed (i.e. `Welcome`
    /// queued). Unsolicited frames (`broadcast`) are only delivered to
    /// handshaken connections, so a client mid-handshake can never observe
    /// anything before its `Welcome`.
    handshaken: HashSet<u64>,
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
            handshaken: HashSet::new(),
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
                    self.handshaken.remove(&id);
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
                let accepted = wire_version == proto::WIRE_VERSION;
                let reply = if accepted {
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
                // Only a completed handshake lifts the gate: a `Rejected`
                // client has no `Welcome` under it and must never receive
                // unsolicited frames.
                if accepted {
                    self.handshaken.insert(conn);
                    // Backfill the bounded sidecar history so a client
                    // attaching after the daemon has been running starts
                    // from the same view live clients already hold. Rides
                    // this connection's queue right after `Welcome`, so
                    // `Welcome` is still its first frame and no live
                    // broadcast can slip ahead of the restored history.
                    for line in self.sidecar_history.iter().cloned() {
                        self.send(conn, DaemonToClient::SidecarLine { line });
                    }
                }
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
        match target
            .session_file
            .as_ref()
            .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        {
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
        for (id, tx) in &self.connections {
            // A connection only ever receives unsolicited frames after its
            // handshake completed, so `Welcome` is always its first frame.
            if self.handshaken.contains(id) {
                match tx.try_send(message.clone()) {
                    Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
                }
            }
        }
    }
}
