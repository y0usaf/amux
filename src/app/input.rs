use arboard::Clipboard;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
use arboard::{LinuxClipboardKind, SetExtLinux};
use winit::event::{KeyEvent, MouseScrollDelta};

use crate::config::{AppAction, KeyStroke, KeymapMatch};
use crate::render::TextRenderer;
use crate::terminal::{TerminalController, TerminalSelectionPoint};

use super::layout::Rect;
use super::App;

pub(super) const ZOOM_WHEEL_PIXELS_PER_STEP: f64 = 60.0;

impl App {
    pub(super) fn clear_current_terminal_selection(&mut self) -> bool {
        self.current_terminal_mut()
            .is_some_and(TerminalController::clear_selection)
    }

    pub(super) fn clear_pending_key_chord(&mut self) {
        self.key_chord_state.clear();
    }

    pub(super) fn clipboard_mut(&mut self) -> Option<&mut Clipboard> {
        if self.clipboard.is_none() {
            match Clipboard::new() {
                Ok(clipboard) => self.clipboard = Some(clipboard),
                Err(error) => {
                    self.set_note(format!("clipboard: {error}"));
                    return None;
                }
            }
        }
        self.clipboard.as_mut()
    }

    pub(super) fn copy_text_to_clipboard(&mut self, text: String) -> bool {
        let result: Result<(), String> = {
            let Some(clipboard) = self.clipboard_mut() else {
                return false;
            };

            #[cfg(all(
                unix,
                not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
            ))]
            {
                match clipboard
                    .set()
                    .clipboard(LinuxClipboardKind::Clipboard)
                    .text(text.as_str())
                {
                    Ok(()) => {
                        let _ = clipboard
                            .set()
                            .clipboard(LinuxClipboardKind::Primary)
                            .text(text.as_str());
                        Ok(())
                    }
                    Err(error) => Err(format!("clipboard: {error}")),
                }
            }

            #[cfg(not(all(
                unix,
                not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
            )))]
            {
                clipboard
                    .set_text(text)
                    .map_err(|error| format!("clipboard: {error}"))
            }
        };

        match result {
            Ok(()) => true,
            Err(error) => {
                self.set_note(error);
                false
            }
        }
    }

    pub(super) fn copy_current_terminal_selection(&mut self) -> bool {
        let text = self
            .current_terminal()
            .and_then(TerminalController::selection_text);
        text.is_some_and(|text| self.copy_text_to_clipboard(text))
    }

    pub(super) fn paste_current_terminal_clipboard(&mut self) -> bool {
        let text = match {
            let Some(clipboard) = self.clipboard_mut() else {
                return false;
            };
            clipboard
                .get_text()
                .map_err(|error| format!("clipboard: {error}"))
        } {
            Ok(text) => text,
            Err(error) => {
                self.set_note(error);
                return false;
            }
        };

        match self.current_terminal_mut() {
            Some(terminal) => match terminal.paste_text(&text) {
                Ok(pasted) => pasted,
                Err(error) => {
                    self.set_note(format!("terminal paste: {error}"));
                    false
                }
            },
            None => false,
        }
    }

    pub(super) fn primary_shortcut_active(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.super_key()
    }

    fn run_shortcut_action(&mut self, action: AppAction) {
        match action {
            AppAction::PreviousProject => self.cycle_projects(-1),
            AppAction::NextProject => self.cycle_projects(1),
            AppAction::PreviousSession => self.cycle_sessions(-1),
            AppAction::NextSession => self.cycle_sessions(1),
            AppAction::OpenProjectPicker => self.open_project_picker(),
            AppAction::NewSession => self.new_session(),
            AppAction::RefreshSession => self.refresh_current_session(),
            AppAction::RefreshAllSessions => self.refresh_all_sessions(),
            AppAction::ArchiveSession => self.archive_selected_session(),
            AppAction::RemoveProject => self.remove_selected_project(),
            AppAction::CopySelection => {
                let _ = self.copy_current_terminal_selection();
            }
            AppAction::PasteClipboard => {
                let _ = self.paste_current_terminal_clipboard();
            }
            AppAction::ZoomIn => {
                let _ = self.zoom_in();
            }
            AppAction::ZoomOut => {
                let _ = self.zoom_out();
            }
            AppAction::ZoomReset => {
                let _ = self.zoom_reset();
            }
        }
    }

    pub(super) fn shortcut_consumed(&mut self, event: &KeyEvent) -> bool {
        let Some(stroke) = KeyStroke::from_event(event, self.modifiers) else {
            return false;
        };

        match self.keymap.advance(&mut self.key_chord_state, stroke) {
            KeymapMatch::NoMatch => false,
            KeymapMatch::Pending => true,
            KeymapMatch::Triggered(action) => {
                self.run_shortcut_action(action);
                true
            }
        }
    }
}

pub(super) fn take_zoom_steps(delta: &MouseScrollDelta, remainder: &mut f64) -> i32 {
    let delta_steps = match delta {
        MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
        MouseScrollDelta::PixelDelta(pos) => pos.y / ZOOM_WHEEL_PIXELS_PER_STEP,
    };
    *remainder += delta_steps;
    let whole = remainder.trunc() as i32;
    *remainder -= f64::from(whole);
    whole
}

pub(super) fn take_wheel_lines(
    delta: &MouseScrollDelta,
    cell_height: i32,
    remainder: &mut f64,
) -> i32 {
    let delta_lines = match delta {
        MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
        MouseScrollDelta::PixelDelta(pos) => pos.y / f64::from(cell_height.max(1)),
    };
    *remainder += delta_lines;
    let whole = remainder.trunc() as i32;
    *remainder -= f64::from(whole);
    whole
}

pub(super) fn terminal_selection_point_for_position(
    rect: Rect,
    text: &TextRenderer,
    rows: u16,
    cols: u16,
    x: f64,
    y: f64,
) -> Option<TerminalSelectionPoint> {
    if rows == 0 || cols == 0 {
        return None;
    }

    let local_x = (x - f64::from(rect.x)).clamp(0.0, f64::from(rect.w.max(0)));
    let local_y = (y - f64::from(rect.y)).clamp(0.0, f64::from(rect.h.max(0)));
    let cell_w = f64::from(text.metrics.cell_width.max(1));
    let cell_h = f64::from(text.metrics.cell_height.max(1));
    let row = ((local_y / cell_h).floor() as u16).min(rows.saturating_sub(1));

    let raw_col = (local_x / cell_w).clamp(0.0, f64::from(cols));
    let floor = raw_col.floor();
    let col = if raw_col - floor <= 0.5 {
        floor as u16
    } else {
        floor as u16 + 1
    }
    .min(cols);

    Some(TerminalSelectionPoint { row, col })
}
