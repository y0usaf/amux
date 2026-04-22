use arboard::Clipboard;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
use arboard::{LinuxClipboardKind, SetExtLinux};
use winit::event::{KeyEvent, MouseScrollDelta};

use crate::config::{AppAction, KeyChordState, KeyStroke, Keymap, KeymapMatch};
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
        let clipboard_text = {
            let Some(clipboard) = self.clipboard_mut() else {
                return false;
            };
            clipboard
                .get_text()
                .map_err(|error| format!("clipboard: {error}"))
        };
        let text = match clipboard_text {
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
        let stroke = KeyStroke::from_event(event, self.modifiers);
        let Some(outcome) = advance_shortcut_match(
            &self.keymap,
            &mut self.key_chord_state,
            stroke,
            event.state == winit::event::ElementState::Pressed && !event.repeat,
        ) else {
            return false;
        };

        match outcome {
            KeymapMatch::NoMatch => false,
            KeymapMatch::Pending => true,
            KeymapMatch::Triggered(action) => {
                self.run_shortcut_action(action);
                true
            }
        }
    }
}

fn advance_shortcut_match(
    keymap: &Keymap,
    state: &mut KeyChordState,
    stroke: Option<KeyStroke>,
    pressed: bool,
) -> Option<KeymapMatch> {
    match stroke {
        Some(stroke) => Some(keymap.advance(state, stroke)),
        None if pressed => {
            state.clear();
            None
        }
        None => None,
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

pub(super) fn terminal_selection_point_for_metrics(
    rect: Rect,
    cell_width: i32,
    cell_height: i32,
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
    let cell_w = f64::from(cell_width.max(1));
    let cell_h = f64::from(cell_height.max(1));
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

#[cfg(test)]
mod shortcut_tests {
    use super::advance_shortcut_match;
    use crate::config::{AppAction, AppConfig, KeyChordState, KeyStroke, KeymapMatch};

    #[test]
    fn unmapped_pressed_event_clears_pending_chord() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "new_session".into(),
            crate::config::ConfigKeybind::Single("ctrl+p n".into()),
        );

        let keymap = config.keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            advance_shortcut_match(
                &keymap,
                &mut state,
                Some(KeyStroke::parse("ctrl+p").unwrap()),
                true,
            ),
            Some(KeymapMatch::Pending)
        );
        assert_eq!(state.pending(), &[KeyStroke::parse("ctrl+p").unwrap()]);

        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, None, true),
            None
        );
        assert!(state.pending().is_empty());
        assert_eq!(
            advance_shortcut_match(
                &keymap,
                &mut state,
                Some(KeyStroke::parse("n").unwrap()),
                true,
            ),
            Some(KeymapMatch::NoMatch)
        );
    }

    #[test]
    fn repeat_or_release_does_not_clear_pending_chord() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "new_session".into(),
            crate::config::ConfigKeybind::Single("ctrl+p n".into()),
        );

        let keymap = config.keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            advance_shortcut_match(
                &keymap,
                &mut state,
                Some(KeyStroke::parse("ctrl+p").unwrap()),
                true,
            ),
            Some(KeymapMatch::Pending)
        );

        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, None, false),
            None
        );
        assert_eq!(state.pending(), &[KeyStroke::parse("ctrl+p").unwrap()]);
    }

    #[test]
    fn shortcut_match_triggers_actions_for_mapped_strokes() {
        let keymap = AppConfig::default().keymap();
        let mut state = KeyChordState::default();

        assert_eq!(
            advance_shortcut_match(
                &keymap,
                &mut state,
                Some(KeyStroke::parse("ctrl+left").unwrap()),
                true,
            ),
            Some(KeymapMatch::Triggered(AppAction::PreviousProject))
        );
    }
}

pub(super) fn terminal_selection_point_for_position(
    rect: Rect,
    text: &TextRenderer,
    rows: u16,
    cols: u16,
    x: f64,
    y: f64,
) -> Option<TerminalSelectionPoint> {
    terminal_selection_point_for_metrics(
        rect,
        text.metrics.cell_width,
        text.metrics.cell_height,
        rows,
        cols,
        x,
        y,
    )
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    #[test]
    fn take_zoom_steps_accumulates_pixel_remainder() {
        let mut remainder = 0.0;
        let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 30.0));

        assert_eq!(take_zoom_steps(&delta, &mut remainder), 0);
        assert!((remainder - 0.5).abs() < 1e-9);

        assert_eq!(take_zoom_steps(&delta, &mut remainder), 1);
        assert!(remainder.abs() < 1e-9);
    }

    #[test]
    fn take_wheel_lines_accumulates_negative_pixel_remainder() {
        let mut remainder = 0.0;
        let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -6.0));

        assert_eq!(take_wheel_lines(&delta, 10, &mut remainder), 0);
        assert!((remainder + 0.6).abs() < 1e-9);

        assert_eq!(take_wheel_lines(&delta, 10, &mut remainder), -1);
        assert!((remainder + 0.2).abs() < 1e-9);
    }

    #[test]
    fn terminal_selection_point_returns_none_for_zero_sized_grid() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 40,
            h: 20,
        };

        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 0, 4, 15.0, 25.0),
            None
        );
        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 1, 0, 15.0, 25.0),
            None
        );
    }

    #[test]
    fn terminal_selection_point_clamps_and_uses_half_cell_threshold() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 40,
            h: 40,
        };

        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 2, 4, 15.0, 25.0),
            Some(TerminalSelectionPoint { row: 0, col: 0 })
        );
        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 2, 4, 15.1, 25.0),
            Some(TerminalSelectionPoint { row: 0, col: 1 })
        );
        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 2, 4, -100.0, 200.0),
            Some(TerminalSelectionPoint { row: 1, col: 0 })
        );
        assert_eq!(
            terminal_selection_point_for_metrics(rect, 10, 20, 2, 4, 1_000.0, 200.0),
            Some(TerminalSelectionPoint { row: 1, col: 4 })
        );
    }
}
