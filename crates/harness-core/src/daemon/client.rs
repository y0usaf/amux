//! Client-side daemon connection: attach to the session daemon (spawning it
//! on demand), issue control RPCs, and route per-session events back to the
//! TUI through the same `HostEvent` channel shape the in-process PTY path
//! used.
//!
//! One reader thread owns all socket reads. Replies are correlated to their
//! request by `req_id` through a pending table; session events route to
//! per-controller channels and sidecar lines to the sidecar stream. Nothing
//! else ever touches the read half, so a synchronous `spawn`/`kill` reply
//! cannot be stolen by event routing.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::notify::Notify;
use crate::terminal::HostEvent;
use crate::terminal::TerminalTarget;

use super::ctl_socket_path;
use super::proto::{
    self, read_msg, write_msg, ClientToDaemon, DaemonToClient, SelectionSpan, SessionInfo,
    TerminalReplay,
};

/// Daemon-initiated session lifecycle broadcast, surfaced to the app layer
/// for workspace discovery (new sessions appearing elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonSessionEvent {
    SessionOpened { id: String, pid: Option<u32> },
    SessionClosed { id: String },
}

/// Reply to an idempotent ensure-spawn: whether the session was already
/// running, plus its pid, exit status, and — when adopting a live session —
/// the daemon's authoritative view state to restore from.
#[derive(Debug, Clone)]
pub struct SpawnOutcome {
    /// The daemon's canonical session key: the resolved session file when
    /// the target carries one, else the requested key. Adopting clients use
    /// this for all subsequent wire traffic.
    pub session_id: String,
    pub already_running: bool,
    pub pid: Option<u32>,
    pub exit_status: Option<String>,
    pub replay: Option<TerminalReplay>,
}

/// Poll cadence while waiting for a freshly spawned daemon's socket.
const SPAWN_POLL: Duration = Duration::from_millis(50);
/// Total wait for a spawned daemon to start listening.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Socket + routing state shared between clones. The reader thread does NOT
/// hold this Arc: it owns clones of the individual fields, so the last
/// `DaemonClient` drop can shut the socket down and the daemon sees EOF.
struct Shared {
    stream: Mutex<UnixStream>,
    next_req_id: AtomicU64,
}

/// req_id -> reply channel for in-flight synchronous RPCs.
type Pending = Mutex<HashMap<u64, Sender<DaemonToClient>>>;
/// session_id -> controller event channel.
type Routes = Mutex<HashMap<String, Sender<HostEvent>>>;

pub struct DaemonClient {
    shared: Arc<Shared>,
    pending: Arc<Pending>,
    routes: Arc<Routes>,
    /// Single-consumer sidecar stream; shared behind a mutex so clones drain
    /// the same queue without duplication.
    sidecar_rx: Arc<Mutex<Receiver<crate::sidecar::SidecarMessage>>>,
    /// Daemon-initiated session discovery events (opened/closed), drained by
    /// the app layer for workspace adoption.
    session_events: Arc<Mutex<Vec<DaemonSessionEvent>>>,
    notify: Notify,
}

impl Clone for DaemonClient {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            pending: Arc::clone(&self.pending),
            routes: Arc::clone(&self.routes),
            sidecar_rx: Arc::clone(&self.sidecar_rx),
            session_events: Arc::clone(&self.session_events),
            notify: self.notify.clone(),
        }
    }
}

impl Drop for DaemonClient {
    fn drop(&mut self) {
        // Last reference: tear the socket down so the daemon observes EOF
        // even though the reader thread is parked on a cloned fd.
        if Arc::strong_count(&self.shared) == 1 {
            if let Ok(stream) = self.shared.stream.lock() {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

impl DaemonClient {
    /// Connect to the daemon, spawning `<current_exe> --daemon` when the
    /// socket is missing or stale. Handshakes before returning.
    pub fn connect_or_spawn(notify: Notify) -> anyhow::Result<Self> {
        let path = ctl_socket_path();
        let mut stream = Self::connect_with_spawn(&path)?;
        Self::handshake(&mut stream)?;
        Self::attached_post_handshake(stream, notify)
    }

    /// Adopt an already-connected (and handshaken) socket. Test seam: lets a
    /// fake server drive the client deterministically.
    pub fn attached_post_handshake(stream: UnixStream, notify: Notify) -> anyhow::Result<Self> {
        let (sidecar_tx, sidecar_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            stream: Mutex::new(stream),
            next_req_id: AtomicU64::new(1),
        });
        let client = Self {
            shared,
            pending: Arc::new(Mutex::new(HashMap::new())),
            routes: Arc::new(Mutex::new(HashMap::new())),
            sidecar_rx: Arc::new(Mutex::new(sidecar_rx)),
            session_events: Arc::new(Mutex::new(Vec::new())),
            notify: notify.clone(),
        };
        client.spawn_reader(sidecar_tx, notify)?;
        Ok(client)
    }

    fn connect_with_spawn(path: &std::path::Path) -> anyhow::Result<UnixStream> {
        if let Ok(stream) = UnixStream::connect(path) {
            return Ok(stream);
        }
        // Stale socket from a crashed daemon, or no daemon yet: reclaim and
        // spawn.
        let _ = std::fs::remove_file(path);
        let current_exe = std::env::current_exe()?;
        let mut child = std::process::Command::new(current_exe)
            .arg("--daemon")
            .spawn()
            .map_err(|error| anyhow::anyhow!("spawning harness daemon: {error}"))?;
        // The direct child exits after double-forking; reap it.
        let _ = child.wait();

        let deadline = Instant::now() + SPAWN_TIMEOUT;
        loop {
            match UnixStream::connect(path) {
                Ok(stream) => return Ok(stream),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(SPAWN_POLL);
                }
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "harness daemon did not start ({}): {error}",
                        path.display()
                    ))
                }
            }
        }
    }

    fn handshake(stream: &mut UnixStream) -> anyhow::Result<()> {
        write_msg(
            stream,
            &ClientToDaemon::Hello {
                wire_version: proto::WIRE_VERSION,
            },
        )?;
        match read_msg::<_, DaemonToClient>(stream)? {
            Some(DaemonToClient::Welcome { wire_version })
                if wire_version == proto::WIRE_VERSION =>
            {
                Ok(())
            }
            Some(DaemonToClient::Rejected { reason }) => Err(anyhow::anyhow!(
                "harness daemon rejected connection: {reason}"
            )),
            Some(_) => Err(anyhow::anyhow!(
                "harness daemon sent unexpected handshake reply"
            )),
            None => Err(anyhow::anyhow!("harness daemon closed during handshake")),
        }
    }

    /// Send a request and wait for its correlated reply. The reader thread
    /// delivers the reply to the returned channel; broadcasts during the wait
    /// are routed as usual, never consumed by the caller.
    fn request(&self, build: impl FnOnce(u64) -> ClientToDaemon) -> anyhow::Result<DaemonToClient> {
        let req_id = self.shared.next_req_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(req_id, tx);
        {
            let mut stream = self
                .shared
                .stream
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Err(error) = write_msg(&mut *stream, &build(req_id)) {
                self.pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&req_id);
                return Err(error.into());
            }
        }
        let reply = rx
            .recv()
            .map_err(|_| anyhow::anyhow!("daemon reader dropped request reply"))?;
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&req_id);
        Ok(reply)
    }

    /// Reader thread: the only consumer of the socket read half. Owns its
    /// pieces outright (no `Arc<Shared>`) so a last-client drop can shut the
    /// socket down while this thread is parked on a read.
    fn spawn_reader(
        &self,
        sidecar_tx: Sender<crate::sidecar::SidecarMessage>,
        notify: Notify,
    ) -> anyhow::Result<()> {
        let mut reader = self
            .shared
            .stream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_clone()?;
        let pending = Arc::clone(&self.pending);
        let routes = Arc::clone(&self.routes);
        let session_events = Arc::clone(&self.session_events);
        std::thread::Builder::new()
            .name("harness-daemon-client-reader".into())
            .spawn(move || {
                loop {
                    match read_msg::<_, DaemonToClient>(&mut reader) {
                        Ok(Some(message)) => {
                            let was_reply = match &message {
                                DaemonToClient::Spawned { req_id, .. }
                                | DaemonToClient::Killed { req_id, .. }
                                | DaemonToClient::Sessions { req_id, .. }
                                | DaemonToClient::Error { req_id, .. }
                                    if *req_id != 0 =>
                                {
                                    let sender = pending
                                        .lock()
                                        .ok()
                                        .and_then(|mut pending| pending.remove(req_id));
                                    if let Some(sender) = sender {
                                        let _ = sender.send(message.clone());
                                    }
                                    true
                                }
                                _ => false,
                            };
                            if was_reply {
                                notify();
                                continue;
                            }
                            match message {
                                DaemonToClient::Output { session_id, bytes } => {
                                    if let Ok(bytes) = BASE64.decode(bytes.as_bytes()) {
                                        route_event(&routes, &session_id, HostEvent::Output(bytes));
                                        notify();
                                    }
                                }
                                DaemonToClient::Exited { session_id, status } => {
                                    route_event(&routes, &session_id, HostEvent::Exited(status));
                                    notify();
                                }
                                DaemonToClient::Error {
                                    session_id: Some(session_id),
                                    message,
                                    ..
                                } => {
                                    route_event(&routes, &session_id, HostEvent::Error(message));
                                    notify();
                                }
                                DaemonToClient::SidecarLine { line } => {
                                    if let Some((message, _raw)) =
                                        crate::sidecar::parse_sidecar_line(&line)
                                    {
                                        let _ = sidecar_tx.send(message);
                                        notify();
                                    }
                                }
                                DaemonToClient::Error { message, .. } => {
                                    log::warn!("harness daemon error: {message}");
                                }
                                DaemonToClient::SessionOpened { id, pid } => {
                                    if let Ok(mut events) = session_events.lock() {
                                        events.push(DaemonSessionEvent::SessionOpened {
                                            id,
                                            pid,
                                        });
                                    }
                                    notify();
                                }
                                DaemonToClient::SessionClosed { id } => {
                                    if let Ok(mut events) = session_events.lock() {
                                        events.push(DaemonSessionEvent::SessionClosed {
                                            id,
                                        });
                                    }
                                    notify();
                                }
                                DaemonToClient::Sessions { .. }
                                | DaemonToClient::Spawned { .. }
                                | DaemonToClient::Killed { .. }
                                | DaemonToClient::Welcome { .. }
                                | DaemonToClient::Rejected { .. }
                                | DaemonToClient::Pong => {}
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                // Reader is gone: fail every in-flight RPC so blocked callers
                // cannot wait forever on a dead daemon.
                pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clear();
            })
            .map_err(|error| anyhow::anyhow!("daemon client reader thread: {error}"))?;
        Ok(())
    }

    /// Idempotent ensure: returns the session's live process state and, when
    /// adopting an already-running session, the replay needed to restore the
    /// client's view.
    pub fn spawn(
        &self,
        session_id: &str,
        target: &TerminalTarget,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<SpawnOutcome> {
        let session_id = session_id.to_string();
        match self.request(|req_id| ClientToDaemon::Spawn {
            req_id,
            session_id: session_id.clone(),
            target: target.clone(),
            rows,
            cols,
        })? {
            DaemonToClient::Spawned {
                session_id: canonical,
                pid,
                already_running,
                exit_status,
                replay,
                ..
            } => {
                // The daemon keys sessions by their canonical identity
                // (resolved session file); rebind this client's route so
                // frames keyed canonically reach the controller that
                // registered under the requested key.
                if canonical != session_id {
                    if let Ok(mut routes) = self.routes.lock() {
                        if let Some(tx) = routes.remove(&session_id) {
                            routes.insert(canonical.clone(), tx);
                        }
                    }
                }
                Ok(SpawnOutcome {
                    session_id: canonical,
                    already_running,
                    pid,
                    exit_status,
                    replay,
                })
            }
            DaemonToClient::Error { message, .. } => Err(anyhow::anyhow!("{message}")),
            other => Err(anyhow::anyhow!("unexpected spawn reply: {other:?}")),
        }
    }

    /// Discovery: enumerate the daemon's sessions so the workspace can adopt
    /// rows this client did not spawn itself.
    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionInfo>> {
        match self.request(|req_id| ClientToDaemon::ListSessions { req_id })? {
            DaemonToClient::Sessions { sessions, .. } => Ok(sessions),
            other => Err(anyhow::anyhow!("unexpected list_sessions reply: {other:?}")),
        }
    }

    pub fn input(&self, session_id: &str, bytes: &[u8]) {
        let Ok(mut stream) = self.shared.stream.lock() else {
            return;
        };
        let _ = write_msg(
            &mut *stream,
            &ClientToDaemon::Input {
                session_id: session_id.to_string(),
                bytes: BASE64.encode(bytes),
            },
        );
    }

    /// Fire-and-forget selection publish; the daemon stores it and hands it
    /// back in replays to future attaching clients.
    pub fn set_selection(&self, session_id: &str, selection: Option<SelectionSpan>) {
        let Ok(mut stream) = self.shared.stream.lock() else {
            return;
        };
        let _ = write_msg(
            &mut *stream,
            &ClientToDaemon::SetSelection {
                session_id: session_id.to_string(),
                selection,
            },
        );
    }

    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) {
        let Ok(mut stream) = self.shared.stream.lock() else {
            return;
        };
        let _ = write_msg(
            &mut *stream,
            &ClientToDaemon::Resize {
                session_id: session_id.to_string(),
                rows,
                cols,
            },
        );
    }

    /// `stop_and_wait` semantics, executed daemon-side.
    pub fn kill(&self, session_id: &str, graceful_ms: u64, force_ms: u64) -> Result<(), String> {
        let session_id = session_id.to_string();
        match self.request(|req_id| ClientToDaemon::Kill {
            req_id,
            session_id: session_id.clone(),
            graceful_ms,
            force_ms,
        }) {
            Ok(DaemonToClient::Killed { .. }) => Ok(()),
            Ok(DaemonToClient::Error { message, .. }) => Err(message),
            Ok(other) => Err(format!("unexpected kill reply: {other:?}")),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn set_hello(&self, line: String) {
        let Ok(mut stream) = self.shared.stream.lock() else {
            return;
        };
        let _ = write_msg(&mut *stream, &ClientToDaemon::SetHello { line });
    }

    pub fn broadcast_line(&self, line: String) {
        let Ok(mut stream) = self.shared.stream.lock() else {
            return;
        };
        let _ = write_msg(&mut *stream, &ClientToDaemon::BroadcastLine { line });
    }

    /// Subscribe a session's event channel; called when a controller attaches.
    pub fn register(&self, session_id: &str, tx: Sender<HostEvent>) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(session_id.to_string(), tx);
        }
    }

    pub fn unregister(&self, session_id: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.remove(session_id);
        }
    }

    /// Drain the next parsed sidecar message, mirroring the old in-process
    /// `SidecarListener::try_recv` used by the TUI event loop.
    pub fn try_recv_sidecar(&self) -> Option<crate::sidecar::SidecarMessage> {
        self.sidecar_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_recv()
            .ok()
    }

    /// Drain the next daemon-initiated session discovery event, if any.
    pub fn try_recv_session_event(&self) -> Option<DaemonSessionEvent> {
        let mut events = self
            .session_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if events.is_empty() {
            return None;
        }
        Some(events.remove(0))
    }
}

fn route_event(
    routes: &Mutex<HashMap<String, Sender<HostEvent>>>,
    session_id: &str,
    event: HostEvent,
) {
    if let Ok(routes) = routes.lock() {
        if let Some(tx) = routes.get(session_id) {
            let _ = tx.send(event);
        }
    }
}
