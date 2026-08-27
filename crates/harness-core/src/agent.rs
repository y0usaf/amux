#[cfg(feature = "omp")]
#[path = "../../../adapters/omp/src/agent.rs"]
mod implementation;
#[cfg(not(feature = "omp"))]
#[path = "../../../adapters/pi/src/agent.rs"]
mod implementation;

pub use implementation::*;
