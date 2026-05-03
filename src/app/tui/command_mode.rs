use crate::config::{AppAction, KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

use super::command::{parse_command, TuiCommand};
use super::command_line::CommandLineState;
use super::input::key_stroke_for_bytes;
use super::TuiApp;

impl TuiApp {
    pub(super) fn handle_command_line_input(&mut self, bytes: &[u8]) -> Option<bool> {
        if self.command_line.is_none() {
            if let Some(rest) = bytes.strip_prefix(b"::") {
                let mut literal = Vec::with_capacity(rest.len() + 1);
                literal.push(b':');
                literal.extend_from_slice(rest);
                self.core.clear_pending_key_chord();
                self.core
                    .send_bytes_to_current_terminal(&literal, "terminal input");
                return Some(false);
            }

            let rest = bytes.strip_prefix(b":")?;
            self.start_command_line("");
            self.insert_command_line_bytes(rest);
            self.core.clear_pending_key_chord();
            return Some(false);
        }

        if self
            .command_line
            .as_ref()
            .is_some_and(|command_line| command_line.input.is_empty())
            && bytes == b":"
        {
            self.command_line = None;
            self.core.clear_pending_key_chord();
            self.core
                .send_bytes_to_current_terminal(b":", "terminal input");
            return Some(false);
        }

        if bytes == [0x03] {
            self.cancel_command_line();
            return Some(false);
        }

        if let Some(stroke) = key_stroke_for_bytes(bytes) {
            if let Some(should_quit) = self.handle_command_line_stroke(stroke) {
                return Some(should_quit);
            }
        }

        self.insert_command_line_bytes(bytes);
        Some(false)
    }

    pub(super) fn handle_command_line_stroke(&mut self, stroke: KeyStroke) -> Option<bool> {
        let no_modifiers = stroke.modifiers == KeyModifiers::default();
        if no_modifiers {
            match stroke.key {
                KeyToken::Named(NamedKeyToken::Enter) => return Some(self.submit_command_line()),
                KeyToken::Named(NamedKeyToken::Escape) => {
                    self.cancel_command_line();
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::Backspace) => {
                    self.command_line_backspace_or_cancel();
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::Delete) => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.delete();
                    }
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::Left) => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_left();
                    }
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::Right) => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_right();
                    }
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::Home) => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_home();
                    }
                    return Some(false);
                }
                KeyToken::Named(NamedKeyToken::End) => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_end();
                    }
                    return Some(false);
                }
                _ => {}
            }
        }

        let ctrl_only =
            stroke.modifiers.control && !stroke.modifiers.shift && !stroke.modifiers.alt;
        if ctrl_only {
            match stroke.key {
                KeyToken::Character(ref key) if key == "a" => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_home();
                    }
                    return Some(false);
                }
                KeyToken::Character(ref key) if key == "e" => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.move_end();
                    }
                    return Some(false);
                }
                KeyToken::Character(ref key) if key == "u" => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.clear();
                    }
                    return Some(false);
                }
                KeyToken::Character(ref key) if key == "w" => {
                    if let Some(command_line) = &mut self.command_line {
                        command_line.delete_word_back();
                    }
                    return Some(false);
                }
                KeyToken::Character(ref key) if key == "c" => {
                    self.cancel_command_line();
                    return Some(false);
                }
                _ => {}
            }
        }

        None
    }

    pub(super) fn start_command_line(&mut self, initial: &str) {
        self.command_line = Some(CommandLineState::with_input(initial));
    }

    pub(super) fn cancel_command_line(&mut self) {
        self.command_line = None;
        self.core.clear_pending_key_chord();
    }

    pub(super) fn command_line_backspace_or_cancel(&mut self) {
        let should_cancel = self
            .command_line
            .as_ref()
            .is_some_and(|command_line| command_line.input.is_empty());
        if should_cancel {
            self.cancel_command_line();
        } else if let Some(command_line) = &mut self.command_line {
            command_line.backspace();
        }
    }

    pub(super) fn insert_command_line_bytes(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return false;
        };
        if text.chars().any(char::is_control) {
            return false;
        }
        if let Some(command_line) = &mut self.command_line {
            command_line.insert_str(text);
            return true;
        }
        false
    }

    pub(super) fn submit_command_line(&mut self) -> bool {
        let input = self
            .command_line
            .take()
            .map(|command_line| command_line.input)
            .unwrap_or_default();
        self.core.clear_pending_key_chord();
        let input = input.trim();
        if input.is_empty() {
            return false;
        }

        match parse_command(input) {
            Ok(TuiCommand::Open(path)) => self.core.open_project_path(path),
            Ok(TuiCommand::Archive) => self.open_archive_viewer(),
            Ok(TuiCommand::Refresh) => self.core.run_action(AppAction::RefreshSession),
            Ok(TuiCommand::Reload) => self.core.run_action(AppAction::RefreshAllSessions),
            Ok(TuiCommand::Usage) => self.open_usage_overlay(),


            Ok(TuiCommand::Quit) => return true,
            Ok(TuiCommand::Help) => self.open_help_overlay(),
            Err(note) => self.core.set_note_text(note),
        }
        false
    }
}
