//! Harness-level reader for the agents' own on-disk session stores.
//!
//! The harness owns no session store of its own: pi and omp persist their
//! sessions (live + ARCHIVE JSONL) and this module only reads, archives,
//! restores, and summarizes them, filtered by the active project list.
//! Everything agent-shaped beyond that (binary discovery, launch argv,
//! extension packaging, and each agent's store contract: env vars, default
//! dirs, project-dir encoding, socket prefix) lives in the per-agent
//! adapters and is spliced in below via `#[path]`.

mod files;
mod scan;
mod types;
mod usage;

#[cfg(feature = "omp")]
#[path = "../../../adapters/omp/src/agent.rs"]
mod implementation;
#[cfg(not(feature = "omp"))]
#[path = "../../../adapters/pi/src/agent.rs"]
mod implementation;

pub use crate::state::ScannedSession;
pub use files::{archive_session_file, live_project_dir, restore_session_file, socket_path};
pub use scan::{evict_old_archived_sessions, scan_archived_sessions, scan_live_sessions};
pub use types::{PiSessionStage, PiSidecarSnapshot};
pub use usage::{
    load_usage_report, load_usage_report_from_path, PiUsageDay, PiUsageModelBreakdown,
    PiUsageReport, PiUsageTotals,
};

pub(crate) use implementation::store::{
    ASCII_ENV, EXTENSION_PATH_ENV, SIDECAR_SESSION_KEY_ENV, SIDECAR_SOCKET_ENV,
};
#[cfg(feature = "omp")]
pub use implementation::OmpLaunch;
#[cfg(not(feature = "omp"))]
pub use implementation::PiLaunch;
pub use implementation::{discover, discover_fresh, extension_path, launch_argv, DiscoverResult};
