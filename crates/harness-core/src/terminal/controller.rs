use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use vt100::Parser;

use super::process::{targets_share_process, HostEvent, TerminalTarget};
use super::selection::{TerminalSelection, TerminalSelectionPoint, TerminalSelectionRange};
use crate::daemon::client::DaemonClient;
use crate::daemon::proto::{SelectionPoint, SelectionSpan};

const DEFAULT_TERMINAL_COLS: u16 = 100;
const DEFAULT_TERMINAL_ROWS: u16 = 32;
/// Scrollback cap shared with the daemon's authoritative parser, so replayed
/// history positions line up between the two.
pub(crate) const TERMINAL_SCROLLBACK: usize = 5_000;
const BRACKETED_PASTE_START: &[u8] = b"[200~";
const BRACKETED_PASTE_END: &[u8] = b"[201~";

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
    /// Shared daemon connection; the controller owns one session of it.
    daemon: Arc<DaemonClient>,
    session_id: String,
    events: Option<mpsc::Receiver<HostEvent>>,
    parser: Parser,
    target: Option<TerminalTarget>,
    status: TerminalStatus,
    rows: u16,
    cols: u16,
    scrollback: usize,
    selection: TerminalSelection,
}

impl TerminalController {
    pub fn new(daemon: Arc<DaemonClient>, session_id: String) -> Self {
        Self {
            daemon,
            session_id,
            events: None,
            parser: Parser::new(
                DEFAULT_TERMINAL_ROWS,
                DEFAULT_TERMINAL_COLS,
                TERMINAL_SCROLLBACK,
            ),
            target: None,
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

        if let Some(target) = self.target.clone() {
            // Route this session's wire events into a fresh channel.
            let (tx, rx) = mpsc::channel();
            self.daemon.register(&self.session_id, tx);
            self.events = Some(rx);

            self.status = TerminalStatus::Launching;
            match self
                .daemon
                .spawn(&self.session_id, &target, self.rows, self.cols)
            {
                Ok(outcome) => {
                    if let Some(replay) = outcome.replay {
                        // Adopt the live process by restoring the daemon's
                        // authoritative view: screen, scrollback history,
                        // selection, and the live PTY size.
                        self.restore_replay(replay);
                    } else if let Some(status) = outcome.exit_status {
                        self.status = TerminalStatus::Exited(status);
                    } else {
                        // Freshly spawned, or the daemon had no view to hand
                        // back; its ongoing output flows in from here.
                        self.status = TerminalStatus::Running;
                    }
                }
                Err(error) => {
                    self.status = TerminalStatus::Error(error.to_string());
                }
            }
        }

        Ok(true)
    }

    pub fn restart(&mut self) -> anyhow::Result<()> {
        let target = self.target.clone();
        // Kill daemon-side (stop_and_wait semantics), then respawn.
        self.stop_and_wait(Duration::from_millis(750), Duration::from_millis(250))?;
        let _ = self.attach(target)?;
        Ok(())
    }

    /// Drop the client-side view. The daemon-side process (if any) keeps
    /// running — detach, not kill.
    pub fn stop(&mut self) {
        self.selection.clear();
        self.detach();
        self.status = TerminalStatus::Empty;
    }

    pub fn stop_and_wait(
        &mut self,
        graceful_timeout: Duration,
        force_timeout: Duration,
    ) -> anyhow::Result<bool> {
        self.selection.clear();
        self.detach();
        if self.target.is_none() {
            self.status = TerminalStatus::Empty;
            return Ok(false);
        }
        self.daemon
            .kill(
                &self.session_id,
                graceful_timeout.as_millis() as u64,
                force_timeout.as_millis() as u64,
            )
            .map_err(anyhow::Error::msg)
            .context("stopping terminal process")?;
        self.status = TerminalStatus::Empty;
        Ok(true)
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

        self.daemon.resize(&self.session_id, rows, cols);
    }

    pub fn drain_events(&mut self) -> bool {
        let mut changed = false;
        let Some(events) = self.events.as_ref() else {
            return false;
        };

        loop {
            match events.try_recv() {
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
                    self.events = None;
                    break;
                }
            }
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

    pub fn max_scrollback(&self) -> usize {
        let mut snapshot = self.parser.screen().clone();
        snapshot.set_scrollback(usize::MAX);
        snapshot.scrollback()
    }

    /// Adopt the daemon's authoritative view state after attaching to an
    /// already-running session.
    fn restore_replay(&mut self, replay: crate::daemon::proto::TerminalReplay) {
        let (rows, cols) = (replay.rows.max(1), replay.cols.max(1));
        self.rows = rows;
        self.cols = cols;
        self.parser = Parser::new(rows, cols, TERMINAL_SCROLLBACK);
        if let Ok(bytes) = BASE64.decode(replay.log.as_bytes()) {
            self.parser.process(&bytes);
        }
        // Reattach lands at the live bottom edge, matching the daemon's own
        // follow-end position.
        self.scrollback = 0;
        self.parser.screen_mut().set_scrollback(0);
        self.selection = replay
            .selection
            .map(|span| {
                let mut selection = TerminalSelection::default();
                selection.set(TerminalSelectionPoint {
                    row: span.start.row,
                    col: span.start.col,
                });
                selection.update_focus(TerminalSelectionPoint {
                    row: span.end.row,
                    col: span.end.col,
                });
                selection
            })
            .unwrap_or_default();
        self.status = TerminalStatus::Running;
    }

    /// Publish the current selection span to the daemon so future attaching
    /// clients restore it.
    fn publish_selection(&self) {
        let selection = self.selection.normalized().map(|range| SelectionSpan {
            start: SelectionPoint {
                row: range.start.row,
                col: range.start.col,
            },
            end: SelectionPoint {
                row: range.end.row,
                col: range.end.col,
            },
        });
        self.daemon.set_selection(&self.session_id, selection);
    }

    pub fn begin_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        if self.selection.anchor() == Some(point) && self.selection.focus() == Some(point) {
            return false;
        }
        self.selection.set(point);
        self.publish_selection();
        true
    }

    pub fn update_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        if self.selection.focus() == Some(point) {
            return false;
        }
        self.selection.update_focus(point);
        self.publish_selection();
        true
    }

    pub fn clear_selection(&mut self) -> bool {
        let had_selection = self.selection.anchor().is_some() || self.selection.focus().is_some();
        self.selection.clear();
        if had_selection {
            self.publish_selection();
        }
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
        self.daemon.input(&self.session_id, bytes);
        Ok(())
    }

    /// Unregister from the daemon's event routing without touching the
    /// daemon-side process.
    fn detach(&mut self) {
        self.events = None;
        self.daemon.unregister(&self.session_id);
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
        // Detach only: agent processes outlive their client
        // ([[principle:daemon-thin-client]]). Explicit kills go through
        // `stop_and_wait`.
        self.detach();
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
