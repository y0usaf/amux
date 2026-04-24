use std::rc::Rc;

use arboard::Clipboard;
use softbuffer::{Context, Surface};
use winit::event_loop::OwnedDisplayHandle;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::config::{AppConfig, KeyChordState, Keymap};
use crate::render::TextRenderer;
use crate::sidecar::SidecarListener;

mod core;
mod events;
mod input;
mod layout;
mod project_actions;
mod rendering;
mod selection;
mod session_actions;
mod sidebar;
mod sidecar_reducer;
mod sidecar_sync;
mod terminal_manager;
mod terminal_sync;
mod theme;
mod workspace;

#[cfg(test)]
mod tests;

pub struct App {
    config: AppConfig,
    keymap: Keymap,
    key_chord_state: KeyChordState,
    window: Option<Rc<Window>>,
    context: Option<Context<OwnedDisplayHandle>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    text: Option<TextRenderer>,
    sidecar: SidecarListener,
    terminal_manager: terminal_manager::TerminalManager,
    workspace: workspace::Workspace,
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
