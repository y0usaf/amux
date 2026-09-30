//! fx's session-store contract: the directories fx owns and where its state
//! lives. harness-core splices this module in via `#[cfg(feature = "fx")]`
//! `#[path]` like the pi and omp adapters.
//!
//! fx keeps one directory per session under `$HOME/.fx/sessions/<id>/`:
//! `session.json` (facts), `display.json` (title/preview), `events.jsonl`
//! (append-only event log), and `session.lock` (liveness). There is no
//! per-project directory encoding: sessions carry `workspace_root` and the
//! scan filters after listing.

use std::path::PathBuf;

/// fx relocates its whole state dir with this env var.
pub const FX_DIR_ENV: &str = "FX_DIR";
/// Harness override for the session root (flat dir).
pub const SESSION_DIR_ENV: &str = "AGENT_HARNESS_FX_SESSION_DIR";

/// fx keeps its state under `$HOME/.fx`.
pub const DEFAULT_AGENT_DIR_REL: &str = ".fx";

/// Sidecar wire env vars, consumed by harness-core when it launches the
/// agent's sidecar (`terminal::process`). fx has no extension host, so
/// `extension_path()` stays None and there is no extension-path override.
pub const SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_FX_SIDECAR_SOCKET";
pub const SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_FX_SESSION_KEY";
pub const ASCII_ENV: &str = "AGENT_HARNESS_FX_ASCII";

/// Prefix for the harness sidecar socket under the runtime dir.
pub const SOCKET_PREFIX: &str = "fx-sidecar";

pub fn default_agent_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(FX_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(dir);
    }
    Some(home_dir()?.join(DEFAULT_AGENT_DIR_REL))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
