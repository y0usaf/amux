use std::io::Write;
use std::sync::mpsc::TryRecvError;

use anyhow::Context;
use portable_pty::PtySize;
use vt100::Parser;

use super::process::{
    spawn_process, targets_share_process, HostEvent, HostProcess, TerminalTarget,
};
use super::selection::{TerminalSelection, TerminalSelectionPoint, TerminalSelectionRange};
use crate::notify::Notify;

const DEFAULT_TERMINAL_COLS: u16 = 100;
const DEFAULT_TERMINAL_ROWS: u16 = 32;
const TERMINAL_SCROLLBACK: usize = 5_000;
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Clone, Debug)]
pub enum TerminalStatus {
    Empty,
    Launching,
    Running,
    Exited(String),
    Error(String),
}

pub(crate) fn disconnected_terminal_status(status: &TerminalStatus) -> Option<TerminalStatus> {
    match status {
        TerminalStatus::Exited(_) | TerminalStatus::Error(_) => None,
        TerminalStatus::Empty | TerminalStatus::Launching | TerminalStatus::Running => {
            Some(TerminalStatus::Error("terminal disconnected".into()))
        }
    }
}

pub struct TerminalController {
    notify: Notify,
    parser: Parser,
    target: Option<TerminalTarget>,
    process: Option<HostProcess>,
    status: TerminalStatus,
    rows: u16,
    cols: u16,
    scrollback: usize,
    selection: TerminalSelection,
}

impl TerminalController {
    pub fn new(notify: Notify) -> Self {
        Self {
            notify,
            parser: Parser::new(
                DEFAULT_TERMINAL_ROWS,
                DEFAULT_TERMINAL_COLS,
                TERMINAL_SCROLLBACK,
            ),
            target: None,
            process: None,
            status: TerminalStatus::Empty,
            rows: DEFAULT_TERMINAL_ROWS,
            cols: DEFAULT_TERMINAL_COLS,
            scrollback: 0,
            selection: TerminalSelection::default(),
        }
    }

    pub fn attach(&mut self, target: Option<TerminalTarget>) -> anyhow::Result<bool> {
        if self.target == target {
            return Ok(false);
        }

        if targets_share_process(self.target.as_ref(), target.as_ref()) {
            self.target = target;
            return Ok(false);
        }

        self.stop();
        self.target = target;
        self.scrollback = 0;
        self.selection.clear();
        self.parser = Parser::new(self.rows, self.cols, TERMINAL_SCROLLBACK);

        match self.target.clone() {
            Some(target) => {
                self.status = TerminalStatus::Launching;
                match spawn_process(&target, self.cols, self.rows, self.notify.clone()) {
                    Ok(process) => {
                        self.process = Some(process);
                        self.status = TerminalStatus::Running;
                    }
                    Err(error) => {
                        self.status = TerminalStatus::Error(error);
                    }
                }
            }
            None => {
                self.status = TerminalStatus::Empty;
            }
        }

        Ok(true)
    }

    pub fn restart(&mut self) -> anyhow::Result<()> {
        let target = self.target.clone();
        let _ = self.attach(None)?;
        let _ = self.attach(target)?;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.selection.clear();
        if let Some(process) = self.process.take() {
            if let Ok(mut killer) = process.killer.lock() {
                let _ = killer.kill();
            }
        }
        self.status = TerminalStatus::Empty;
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if self.rows == rows && self.cols == cols {
            return;
        }

        self.rows = rows;
        self.cols = cols;
        self.parser.screen_mut().set_size(rows, cols);
        self.clear_selection();
        self.set_scrollback(self.scrollback);

        if let Some(process) = self.process.as_ref() {
            let _ = process.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    pub fn drain_events(&mut self) -> bool {
        let mut changed = false;
        let mut drop_process = false;

        while let Some(process) = self.process.as_ref() {
            match process.rx.try_recv() {
                Ok(HostEvent::Output(bytes)) => {
                    self.parser.process(&bytes);
                    self.scrollback = self.parser.screen().scrollback();
                    if self.scrollback == 0 {
                        self.parser.screen_mut().set_scrollback(0);
                    }
                    self.status = TerminalStatus::Running;
                    changed = true;
                }
                Ok(HostEvent::Exited(status)) => {
                    self.status = TerminalStatus::Exited(status);
                    drop_process = true;
                    changed = true;
                    break;
                }
                Ok(HostEvent::Error(error)) => {
                    self.status = TerminalStatus::Error(error);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(status) = disconnected_terminal_status(&self.status) {
                        self.status = status;
                        changed = true;
                    }
                    drop_process = true;
                    break;
                }
            }
        }

        if drop_process {
            self.process = None;
        }

        changed
    }

    pub fn scroll_by_lines(&mut self, delta_lines: i32) -> bool {
        if delta_lines == 0 {
            return false;
        }

        let requested = if delta_lines > 0 {
            self.scrollback.saturating_add(delta_lines as usize)
        } else {
            self.scrollback
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.set_scrollback(requested)
    }

    pub fn status(&self) -> &TerminalStatus {
        &self.status
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn begin_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        if self.selection.anchor() == Some(point) && self.selection.focus() == Some(point) {
            return false;
        }
        self.selection.set(point);
        true
    }

    pub fn update_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        if self.selection.focus() == Some(point) {
            return false;
        }
        self.selection.update_focus(point);
        true
    }

    pub fn clear_selection(&mut self) -> bool {
        let had_selection = self.selection.anchor().is_some() || self.selection.focus().is_some();
        self.selection.clear();
        had_selection
    }

    pub fn selection_range(&self) -> Option<TerminalSelectionRange> {
        self.selection.normalized()
    }

    pub fn selection_text(&self) -> Option<String> {
        let selection = self.selection.normalized()?;
        let text = self.parser.screen().contents_between(
            selection.start.row,
            selection.start.col,
            selection.end.row,
            selection.end.col,
        );
        (!text.is_empty()).then_some(text)
    }

    pub fn paste_text(&mut self, text: &str) -> anyhow::Result<bool> {
        self.paste_bytes(text.as_bytes())
            .context("writing clipboard paste")
    }

    pub fn paste_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }
        let bytes = encoded_paste_bytes(bytes, self.parser.screen().bracketed_paste());
        self.write_bytes(&bytes).context("writing terminal paste")?;
        Ok(true)
    }

    pub fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }
        self.write_bytes(bytes).context("writing terminal input")?;
        Ok(true)
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        self.set_scrollback(0)
    }

    fn set_scrollback(&mut self, requested: usize) -> bool {
        let before = self.scrollback;
        self.parser.screen_mut().set_scrollback(requested);
        self.scrollback = self.parser.screen().scrollback();
        let changed = self.scrollback != before;
        if changed {
            self.clear_selection();
        }
        changed
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.clear_selection();
        if self.scrollback > 0 {
            self.scroll_to_bottom();
        }
        let Some(process) = self.process.as_ref() else {
            return Ok(());
        };
        let Ok(mut writer) = process.writer.lock() else {
            anyhow::bail!("terminal writer lock poisoned");
        };
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }
}

fn encoded_paste_bytes(paste: &[u8], bracketed_paste: bool) -> Vec<u8> {
    if !bracketed_paste {
        return paste.to_vec();
    }

    let mut bytes =
        Vec::with_capacity(BRACKETED_PASTE_START.len() + paste.len() + BRACKETED_PASTE_END.len());
    bytes.extend_from_slice(BRACKETED_PASTE_START);
    bytes.extend_from_slice(paste);
    bytes.extend_from_slice(BRACKETED_PASTE_END);
    bytes
}

impl Drop for TerminalController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::encoded_paste_bytes;

    #[test]
    fn paste_bytes_are_raw_when_bracketed_paste_is_disabled() {
        assert_eq!(
            encoded_paste_bytes(b"hello\nworld", false),
            b"hello\nworld".to_vec()
        );
    }

    #[test]
    fn paste_bytes_are_wrapped_when_bracketed_paste_is_enabled() {
        assert_eq!(
            encoded_paste_bytes(b"hello\nworld", true),
            b"\x1b[200~hello\nworld\x1b[201~".to_vec()
        );
    }
}
