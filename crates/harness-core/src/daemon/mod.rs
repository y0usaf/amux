//! Session daemon: owns agent PTY processes so they survive TUI client exit
//! ([[principle:daemon-thin-client]]). Clients attach over a Unix socket
//! speaking [`proto`]; agents keep dialing the same sidecar socket they always
//! have, now hosted here instead of in the TUI process.

pub mod client;
pub mod proto;
pub mod server;

use std::path::PathBuf;
use crate::agent::SOCKET_PREFIX;
use crate::util::app_runtime_dir;

/// Socket directory leaf encodes the wire version: bumping
/// [`proto::WIRE_VERSION`] renames the directory so old and new binaries
/// never share sockets across a protocol change.
fn socket_dir() -> PathBuf {
    app_runtime_dir().join(format!("wire_v{}", proto::WIRE_VERSION))
}

pub fn ctl_socket_path() -> PathBuf {
    socket_dir().join(format!("{SOCKET_PREFIX}-ctl.sock"))
}

pub fn sidecar_socket_path() -> PathBuf {
    socket_dir().join(format!("{SOCKET_PREFIX}-sidecar.sock"))
}

pub fn pid_file_path() -> PathBuf {
    socket_dir().join(format!("{SOCKET_PREFIX}-daemon.pid"))
}
