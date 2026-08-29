//! Compile-time adapter shim: re-exports the fx adapter's surface at
//! this module's root so harness-core's `implementation::` references and
//! the cfg-selected `mod implementation` in agent.rs line up.
#[path = "../../../../../adapters/fx/src/agent.rs"]
pub mod adapter;
pub use adapter::*;
