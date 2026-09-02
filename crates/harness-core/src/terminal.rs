#[path = "terminal/controller.rs"]
mod controller;
#[path = "terminal/process.rs"]
mod process;
#[path = "terminal/selection.rs"]
mod selection;

pub(crate) use controller::TERMINAL_SCROLLBACK;
pub use controller::{TerminalController, TerminalStatus};
pub use process::TerminalTarget;
pub(crate) use process::{spawn_argv, spawn_process, HostEvent, HostProcess};
pub(crate) use selection::terminal_selection_span;
pub use selection::{TerminalSelectionPoint, TerminalSelectionRange};
