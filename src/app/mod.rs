use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use arboard::Clipboard;
use softbuffer::{Context, Surface};
use winit::event_loop::{EventLoopProxy, OwnedDisplayHandle};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::config::{AppConfig, KeyChordState, Keymap};
use crate::render::TextRenderer;
use crate::sidecar::SidecarListener;
use crate::state::{PersistedState, Project};
use crate::terminal::TerminalController;

mod core;
mod events;
mod input;
mod layout;
mod project_actions;
mod rendering;
mod selection;
mod session_actions;
mod sidebar;
mod sidecar_sync;
mod terminal_sync;
mod theme;

#[cfg(test)]
mod tests;

pub struct App {
    proxy: EventLoopProxy<()>,
    initial_project_paths: Vec<PathBuf>,
    config: AppConfig,
    keymap: Keymap,
    key_chord_state: KeyChordState,
    persisted: PersistedState,
    window: Option<Rc<Window>>,
    context: Option<Context<OwnedDisplayHandle>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    text: Option<TextRenderer>,
    terminals: HashMap<String, TerminalController>,
    sidecar: SidecarListener,
    sidecar_extension_path: Option<PathBuf>,
    sidecar_socket_path: PathBuf,
    projects: Vec<Project>,
    selected_project: usize,
    selected_session: Option<usize>,
    sidebar_scroll: usize,
    sidebar_sync_to_selection: bool,
    sidebar_wheel_remainder: f64,
    terminal_wheel_remainder: f64,
    zoom_wheel_remainder: f64,
    ui_scale: f32,
    modifiers: ModifiersState,
    cursor_pos: (f64, f64),
    terminal_selection_in_progress: bool,
    clipboard: Option<Clipboard>,
    note: Option<String>,
    needs_redraw: bool,
}
