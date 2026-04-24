#[path = "pi/discovery.rs"]
mod discovery;
#[path = "pi/files.rs"]
mod files;
#[path = "pi/scan.rs"]
mod scan;
#[path = "pi/types.rs"]
mod types;

pub use crate::state::ScannedSession;
pub use discovery::{
    discover, discover_fresh, extension_path, launch_argv, DiscoverResult, PiLaunch,
};
pub use files::{
    archive_session_file, is_pi_session_path, live_project_dir, restore_session_file, socket_path,
};
pub use scan::scan_live_sessions;
pub use types::{PiSessionStage, PiSidecarSnapshot};

pub const PI_SIDECAR_SOCKET_ENV: &str = "AGENT_HARNESS_PI_SIDECAR_SOCKET";
pub const PI_SIDECAR_SESSION_KEY_ENV: &str = "AGENT_HARNESS_PI_SESSION_KEY";
pub const PI_EXTENSION_PATH_ENV: &str = "AGENT_HARNESS_PI_EXTENSION";
