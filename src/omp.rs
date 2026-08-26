#[path = "omp/discovery.rs"]
mod discovery;
#[path = "omp/files.rs"]
mod files;
#[path = "omp/scan.rs"]
mod scan;
#[path = "omp/types.rs"]
mod types;
#[path = "omp/usage.rs"]
mod usage;

pub use crate::state::ScannedSession;
pub use discovery::{
    discover, discover_fresh, extension_path, launch_argv, DiscoverResult, OmpLaunch,
};
pub use files::{
    archive_session_file, is_omp_session_path, live_project_dir, restore_session_file, socket_path,
};
pub use scan::{evict_old_archived_sessions, scan_archived_sessions, scan_live_sessions};
pub use types::{PiSessionStage, PiSidecarSnapshot};
pub use usage::{
    load_usage_report, load_usage_report_from_path, PiUsageDay, PiUsageModelBreakdown,
    PiUsageReport, PiUsageTotals,
};

pub const OMP_SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_OMP_SIDECAR_SOCKET";
pub const OMP_SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_OMP_SESSION_KEY";
pub const OMP_EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_OMP_EXTENSION";
