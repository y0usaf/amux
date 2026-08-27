use super::{TuiApp, TUI_BRACKETED_PASTE_END, TUI_BRACKETED_PASTE_START};

impl TuiApp {
    pub(super) fn handle_host_bracketed_paste(&mut self, bytes: &[u8]) -> bool {
        if self.host_bracketed_paste.is_some() {
            if bytes == TUI_BRACKETED_PASTE_END {
                let paste = self.host_bracketed_paste.take().unwrap_or_default();
                self.forward_host_paste(&paste);
            } else {
                self.host_bracketed_paste
                    .as_mut()
                    .expect("paste buffer exists")
                    .extend_from_slice(bytes);
            }
            return true;
        }

        if bytes == TUI_BRACKETED_PASTE_START {
            self.host_bracketed_paste = Some(Vec::new());
            return true;
        }

        false
    }

    pub(super) fn forward_host_paste(&mut self, paste: &[u8]) {
        if self.command_line.is_some() {
            self.insert_command_line_bytes(paste);
        } else {
            self.core.paste_bytes_to_current_terminal(paste);
        }
    }
}
