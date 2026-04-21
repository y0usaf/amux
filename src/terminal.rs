#[path = "terminal/input.rs"]
mod input;
#[path = "terminal/process.rs"]
mod process;
#[path = "terminal/selection.rs"]
mod selection;

use std::io::Write;
use std::sync::mpsc::TryRecvError;

use anyhow::Context;
use portable_pty::PtySize;
use vt100::Parser;
use winit::event::KeyEvent;
use winit::event_loop::EventLoopProxy;
use winit::keyboard::ModifiersState;

use crate::terminal::input::KeyInput;
use crate::terminal::process::{spawn_process, targets_share_process, HostEvent, HostProcess};
use crate::terminal::selection::TerminalSelection;

pub use process::TerminalTarget;
pub(crate) use selection::terminal_selection_span;
pub use selection::{TerminalSelectionPoint, TerminalSelectionRange};

const DEFAULT_TERMINAL_COLS: u16 = 100;
const DEFAULT_TERMINAL_ROWS: u16 = 32;
const TERMINAL_SCROLLBACK: usize = 5_000;

#[derive(Clone, Debug)]
pub enum TerminalStatus {
    Empty,
    Launching,
    Running,
    Exited(String),
    Error(String),
}

pub struct TerminalController {
    proxy: EventLoopProxy<()>,
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
    pub fn new(proxy: EventLoopProxy<()>) -> Self {
        Self {
            proxy,
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
                match spawn_process(&target, self.cols, self.rows, self.proxy.clone()) {
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

        loop {
            let recv = match self.process.as_ref() {
                Some(process) => process.rx.try_recv(),
                None => break,
            };

            match recv {
                Ok(HostEvent::Output(bytes)) => {
                    self.clear_selection();
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
        if text.is_empty() {
            return Ok(false);
        }
        self.write_bytes(text.as_bytes())
            .context("writing clipboard paste")?;
        Ok(true)
    }

    pub fn handle_key(
        &mut self,
        event: &KeyEvent,
        modifiers: ModifiersState,
    ) -> anyhow::Result<bool> {
        match input::handle_key_input(event, modifiers, self.rows, self.parser.screen()) {
            KeyInput::Ignored => Ok(false),
            KeyInput::Scroll(delta) if delta == i32::MAX => Ok(self.scroll_to_top()),
            KeyInput::Scroll(delta) if delta == i32::MIN => Ok(self.scroll_to_bottom()),
            KeyInput::Scroll(delta) => Ok(self.scroll_by_lines(delta)),
            KeyInput::Bytes(bytes) => {
                self.write_bytes(&bytes).context("writing terminal input")?;
                Ok(true)
            }
        }
    }

    fn scroll_to_top(&mut self) -> bool {
        self.set_scrollback(usize::MAX)
    }

    fn scroll_to_bottom(&mut self) -> bool {
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

impl Drop for TerminalController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        targets_share_process, terminal_selection_span, TerminalSelectionPoint, TerminalTarget,
    };
    use crate::terminal::selection::TerminalSelection;
    use std::path::PathBuf;

    fn target(session_file: Option<&str>) -> TerminalTarget {
        TerminalTarget {
            pi_binary: Some("pi".into()),
            sidecar_extension_path: Some(PathBuf::from("/tmp/sidecar.js")),
            sidecar_socket_path: PathBuf::from("/tmp/pi.sock"),
            harness_session_id: "local-session-1".into(),
            cwd: PathBuf::from("/tmp/project"),
            session_file: session_file.map(PathBuf::from),
        }
    }

    #[test]
    fn session_file_change_reuses_existing_process() {
        assert!(targets_share_process(
            Some(&target(None)),
            Some(&target(Some("/tmp/session.jsonl"))),
        ));
    }

    #[test]
    fn process_identity_change_forces_restart() {
        let current = target(Some("/tmp/session-a.jsonl"));
        let mut next = target(Some("/tmp/session-b.jsonl"));
        next.cwd = PathBuf::from("/tmp/other-project");

        assert!(!targets_share_process(Some(&current), Some(&next)));
    }

    #[test]
    fn terminal_selection_normalizes_anchor_and_focus() {
        let mut selection = TerminalSelection::default();
        selection.set(TerminalSelectionPoint { row: 4, col: 7 });
        selection.update_focus(TerminalSelectionPoint { row: 2, col: 3 });

        let normalized = selection.normalized().expect("normalized selection");
        assert_eq!(normalized.start, TerminalSelectionPoint { row: 2, col: 3 });
        assert_eq!(normalized.end, TerminalSelectionPoint { row: 4, col: 7 });
    }

    #[test]
    fn terminal_selection_span_handles_single_and_multi_row_ranges() {
        let mut selection = TerminalSelection::default();
        selection.set(TerminalSelectionPoint { row: 1, col: 2 });
        selection.update_focus(TerminalSelectionPoint { row: 3, col: 8 });
        let selection = selection.normalized();

        assert_eq!(terminal_selection_span(selection, 0, 10), None);
        assert_eq!(terminal_selection_span(selection, 1, 10), Some((2, 8)));
        assert_eq!(terminal_selection_span(selection, 2, 10), Some((0, 10)));
        assert_eq!(terminal_selection_span(selection, 3, 10), Some((0, 8)));
        assert_eq!(terminal_selection_span(selection, 4, 10), None);
    }
}
