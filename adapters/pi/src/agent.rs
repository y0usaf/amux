#[path = "agent/discovery.rs"]
mod discovery;
#[path = "agent/files.rs"]
mod files;
#[path = "agent/scan.rs"]
mod scan;
#[path = "agent/types.rs"]
mod types;
#[path = "agent/usage.rs"]
mod usage;

pub use crate::state::ScannedSession;
pub use discovery::{
    discover, discover_fresh, extension_path, launch_argv, DiscoverResult, PiLaunch,
};
pub use files::{
    archive_session_file, is_pi_session_path, live_project_dir, restore_session_file, socket_path,
};
pub use scan::{evict_old_archived_sessions, scan_archived_sessions, scan_live_sessions};
pub use types::{PiSessionStage, PiSidecarSnapshot};
pub use usage::{
    load_usage_report, load_usage_report_from_path, PiUsageDay, PiUsageModelBreakdown,
    PiUsageReport, PiUsageTotals,
};

pub const PI_SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_PI_SIDECAR_SOCKET";
pub const PI_SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_PI_SESSION_KEY";
pub const PI_EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_PI_EXTENSION";

pub const OMP_SIDECAR_SOCKET_ENV: &str = PI_SIDECAR_SOCKET_ENV;
pub const OMP_SIDECAR_SESSION_KEY_ENV: &str = PI_SIDECAR_SESSION_KEY_ENV;
