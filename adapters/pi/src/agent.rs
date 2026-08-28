//! pi adapter: binary discovery, launch argv, extension packaging, and the
//! on-disk session-store contract (env vars, default dirs, project-dir
//! encoding, socket prefix).
//!
//! Session-store reading machinery lives at harness level
//! (`crates/harness-core` `agent` module) and consumes the pi-shaped facts
//! declared here.

#[path = "agent/discovery.rs"]
mod discovery;

#[path = "agent/store.rs"]
pub mod store;

pub use discovery::{
    discover, discover_fresh, extension_path, launch_argv, DiscoverResult, PiLaunch,
};
