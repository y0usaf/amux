//! fx adapter: store contract, discovery, and the fx-format readers that
//! replace harness-core's pi-format `files`/`scan`/`usage`/`types` modules.
//! harness-core splices the whole module in via `#[cfg(feature = "fx")]`.

#[path = "agent/store.rs"]
pub mod store;

#[path = "agent/discovery.rs"]
mod discovery;

#[path = "agent/files.rs"]
pub mod files;

#[path = "agent/scan.rs"]
pub mod scan;

#[path = "agent/usage.rs"]
pub mod usage;

#[path = "agent/types.rs"]
pub mod types;

pub use discovery::{
    discover, discover_fresh, extension_path, launch_argv, DiscoverResult, FxLaunch,
};
