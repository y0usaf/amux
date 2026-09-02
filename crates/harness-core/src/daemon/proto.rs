//! Framed wire protocol between the harness TUI clients and the session
//! daemon.
//!
//! Frame = `u32` LE length + UTF-8 JSON payload (serde_json, base64 for byte
//! payloads). A versioned socket directory (`wire_v<N>`) keeps old and new
//! binaries from ever connecting across protocol changes; the in-band
//! `Hello`/`Welcome` exchange double-checks that both sides speak the same
//! version.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::terminal::TerminalTarget;

pub const WIRE_VERSION: u32 = 3;

/// Hard frame cap (1 MiB). PTY output chunks are 8 KiB reads; base64 inflates
/// them 4/3, so this bounds a single frame comfortably while keeping a
/// malformed peer from allocating arbitrarily.
const MAX_FRAME_SIZE: u32 = 1024 * 1024;

/// One terminal grid cell coordinate on the shared screen.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionPoint {
    pub row: u16,
    pub col: u16,
}

/// Normalized selection span over the shared screen, rendered identically by
/// every attached client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionSpan {
    pub start: SelectionPoint,
    pub end: SelectionPoint,
}

/// Authoritative per-session view state the daemon hands to a newly
/// attaching client. `log` is base64 PTY output covering (at least) the tail
/// of the session; replaying it into a fresh parser sized `rows` x `cols`
/// reconstructs screen, scrollback history, and alternate-screen state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalReplay {
    pub rows: u16,
    pub cols: u16,
    pub selection: Option<SelectionSpan>,
    pub log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClientToDaemon {
    Hello {
        wire_version: u32,
    },
    Ping,
    /// Idempotent: an alive session is a no-op returning its state.
    /// `req_id` correlates the synchronous reply; 0 = unsolicited.
    Spawn {
        req_id: u64,
        session_id: String,
        target: TerminalTarget,
        rows: u16,
        cols: u16,
    },
    /// Base64-encoded bytes to write to the session PTY.
    Input {
        session_id: String,
        bytes: String,
    },
    Resize {
        session_id: String,
        rows: u16,
        cols: u16,
    },
    /// `stop_and_wait` semantics: terminate, wait graceful, force-kill, wait.
    /// Synchronous: replies exactly once with `Killed` (same `req_id`).
    Kill {
        req_id: u64,
        session_id: String,
        graceful_ms: u64,
        force_ms: u64,
    },
    /// Publish this client's selection span for the session; `None` clears
    /// it. Fire-and-forget: the daemon stores it and hands it back in
    /// [`TerminalReplay`] to future attaching clients.
    SetSelection {
        session_id: String,
        selection: Option<SelectionSpan>,
    },
    /// Rail hello line for newly connecting agent extensions.
    SetHello {
        line: String,
    },
    /// One-shot line to currently connected agent extensions.
    BroadcastLine {
        line: String,
    },
    /// Discovery: enumerate every daemon session. Synchronous: replies
    /// exactly once with `Sessions` (same `req_id`).
    ListSessions {
        req_id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DaemonToClient {
    Welcome {
        wire_version: u32,
    },
    Rejected {
        reason: String,
    },
    Spawned {
        req_id: u64,
        session_id: String,
        pid: Option<u32>,
        already_running: bool,
        exit_status: Option<String>,
        /// Present iff `already_running`: the daemon's authoritative view
        /// state, sent atomically with this reply so replayed history and
        /// subsequent live output never interleave out of order.
        replay: Option<TerminalReplay>,
    },
    /// Base64-encoded PTY output chunk.
    Output {
        session_id: String,
        bytes: String,
    },
    Exited {
        session_id: String,
        status: String,
    },
    /// `req_id` 0 = unsolicited (session-level error broadcast).
    Error {
        req_id: u64,
        session_id: Option<String>,
        message: String,
    },
    Killed {
        req_id: u64,
        session_id: String,
    },
    /// Raw NDJSON sidecar line (snapshot or theme) passed through unparsed.
    SidecarLine {
        line: String,
    },
    Pong,
    /// Unsolicited discovery broadcast: a new daemon session appeared
    /// (fresh spawn). Carries the daemon's canonical session key.
    SessionOpened {
        id: String,
        pid: Option<u32>,
    },
    /// Unsolicited discovery broadcast: a daemon session ended.
    SessionClosed {
        id: String,
    },
    /// Reply to [`ClientToDaemon::ListSessions`].
    Sessions {
        req_id: u64,
        sessions: Vec<SessionInfo>,
    },
}

/// One daemon session as reported by discovery (`ListSessions` /
/// `SessionOpened`). `id` is the daemon's canonical session key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub pid: Option<u32>,
    pub running: bool,
    pub exit_status: Option<String>,
}

pub fn write_msg<W: Write, T: Serialize>(writer: &mut W, message: &T) -> std::io::Result<()> {
    let payload = serde_json::to_vec(message)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    if payload.len() as u64 > MAX_FRAME_SIZE as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

/// One frame read. Returns `Ok(None)` on clean EOF before any byte of a new
/// frame, `Err` on truncation mid-frame or oversized frames.
pub fn read_msg<R: Read, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> std::io::Result<Option<T>> {
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length_bytes);
    if length == 0 || length > MAX_FRAME_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut payload = vec![0u8; length as usize];
    reader.read_exact(&mut payload).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("truncated frame: {error}"),
        )
    })?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}
