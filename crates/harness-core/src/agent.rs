//! Harness-level reader for the agents' own on-disk session stores.
//!
//! The harness owns no session store of its own: pi, omp, and fx persist
//! their sessions on disk and this module only reads, archives, restores,
//! and summarizes them, filtered by the active project list.
//! Everything agent-shaped beyond that (binary discovery, launch argv,
//! extension packaging, and each agent's store contract: env vars, default
//! dirs, project-dir encoding, socket prefix) lives in the per-agent
//! adapters and is spliced in below via `#[path]`.
//!
//! pi and omp share one session format, so their adapters only supply the
//! store contract and discovery while `files`/`scan`/`usage`/`types` below
//! parse the shared JSONL. fx has its own format (directory per session),
//! so under `feature = "fx"` the adapter supplies all of them and the
//! shared parsers are compiled out.

#[cfg(not(feature = "fx"))]
mod files;
#[cfg(not(feature = "fx"))]
mod scan;
#[cfg(not(feature = "fx"))]
mod types;
// The pi/omp reader module, compiled out under fx; the shared report types
// it re-exports live in usage_types.rs, which both paths include.
#[cfg(not(feature = "fx"))]
mod usage;
#[cfg(not(feature = "fx"))]
#[path = "agent/usage_types.rs"]
pub(crate) mod usage_types;
#[cfg(not(feature = "fx"))]
pub use usage::{
    load_usage_report, load_usage_report_from_path, PiUsageDay, PiUsageModelBreakdown,
    PiUsageReport, PiUsageTotals,
};

// fx wins if both features are set. Cargo unifies features when several
// workspace members build in one invocation, so prefer building each TUI
// with `cargo build -p <crate>`; the per-package build gets exactly one
// adapter.
#[cfg(all(feature = "omp", not(feature = "fx")))]
#[path = "agent/implementation/omp.rs"]
mod implementation;
#[cfg(feature = "fx")]
#[path = "agent/implementation/fx.rs"]
mod implementation;
#[cfg(not(any(feature = "omp", feature = "fx")))]
#[path = "agent/implementation/pi.rs"]
mod implementation;

#[cfg(feature = "fx")]
pub use implementation::{files, scan, types};
// fx's usage module returns pi-shaped report structs; under feature = "fx"
// the shared usage module is compiled out with the rest of the pi-format
// readers, so the type definitions move to usage_types.rs (included by both
// paths) and both usage modules re-export from it.
#[cfg(feature = "fx")]
#[path = "agent/usage_types.rs"]
pub(crate) mod usage_types;
#[cfg(feature = "fx")]
pub use usage_types::{PiUsageDay, PiUsageModelBreakdown, PiUsageReport, PiUsageTotals};
// fx adapter provides the same reader functions under the same names.
#[cfg(feature = "fx")]
pub use implementation::usage::{load_usage_report, load_usage_report_from_path};

pub use crate::state::ScannedSession;
pub use files::{archive_session_file, live_project_dir, restore_session_file, socket_path};
pub use scan::{evict_old_archived_sessions, scan_archived_sessions, scan_live_sessions};
pub use types::{PiSessionStage, PiSidecarSnapshot};

pub(crate) use implementation::store::{
    ASCII_ENV, EXTENSION_PATH_ENV, SIDECAR_SESSION_KEY_ENV, SIDECAR_SOCKET_ENV, SOCKET_PREFIX,
};
#[cfg(feature = "fx")]
pub use implementation::FxLaunch;
#[cfg(all(feature = "omp", not(feature = "fx")))]
pub use implementation::OmpLaunch;
#[cfg(not(any(feature = "omp", feature = "fx")))]
pub use implementation::PiLaunch;
pub use implementation::{discover, discover_fresh, extension_path, launch_argv, DiscoverResult};
