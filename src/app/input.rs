use super::*;

pub(super) const ZOOM_WHEEL_PIXELS_PER_STEP: f64 = 60.0;

impl App {
    pub(super) fn clear_current_terminal_selection(&mut self) -> bool {
        self.current_terminal_mut()
            .is_some_and(TerminalController::clear_selection)
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

    pub(super) fn terminal_clipboard_shortcut_consumed(
        &mut self,
        event: &winit::event::KeyEvent,
    ) -> bool {
        if event.state != ElementState::Pressed || self.modifiers.alt_key() {
            return false;
        }

        let copy = matches!(&event.logical_key, Key::Character(text)
            if text.eq_ignore_ascii_case("c")
                && (self.modifiers.super_key()
                    || (self.modifiers.control_key() && self.modifiers.shift_key())));
        if copy {
            return self.copy_current_terminal_selection();
        }

        let paste = matches!(&event.logical_key, Key::Character(text)
            if text.eq_ignore_ascii_case("v")
                && (self.modifiers.super_key() || self.modifiers.control_key()))
            || matches!(&event.logical_key, Key::Named(NamedKey::Insert)
                if self.modifiers.shift_key());
        if paste {
            return self.paste_current_terminal_clipboard();
        }

        false
    }

    pub(super) fn primary_shortcut_active(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.super_key()
    }

    pub(super) fn zoom_shortcut_consumed(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.state != ElementState::Pressed || self.modifiers.alt_key() {
            return false;
        }
        if !self.primary_shortcut_active() {
            return false;
        }

        match &event.logical_key {
            Key::Character(text) => match text.as_str() {
                "=" | "+" => self.zoom_in(),
                "-" | "_" => self.zoom_out(),
                "0" | ")" => self.zoom_reset(),
                _ => false,
            },
            _ => false,
        }
    }

    pub(super) fn shortcut_consumed(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }

        if !self.modifiers.control_key() {
            return false;
        }

        match &event.logical_key {
            Key::Named(NamedKey::Delete) if self.modifiers.shift_key() => {
                self.remove_selected_project();
                return true;
            }
            Key::Named(NamedKey::Delete) => {
                self.archive_selected_session();
                return true;
            }
            _ => {}
        }

        let key = match &event.logical_key {
            Key::Character(text) => text.to_ascii_lowercase(),
            _ => return false,
        };

        match key.as_str() {
            "h" if !self.modifiers.shift_key() => {
                self.cycle_projects(-1);
                true
            }
            "l" if !self.modifiers.shift_key() => {
                self.cycle_projects(1);
                true
            }
            "k" if !self.modifiers.shift_key() => {
                self.cycle_sessions(-1);
                true
            }
            "j" if !self.modifiers.shift_key() => {
                self.cycle_sessions(1);
                true
            }
            "o" if !self.modifiers.shift_key() => {
                self.open_project_picker();
                true
            }
            "n" => {
                self.new_session();
                true
            }
            "r" => {
                if self.modifiers.shift_key() {
                    self.refresh_all_sessions();
                } else {
                    self.refresh_current_session();
                }
                true
            }
            "d" if self.modifiers.shift_key() => {
                self.remove_selected_project();
                true
            }
            _ => false,
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
