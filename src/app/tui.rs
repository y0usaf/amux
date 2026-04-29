use std::io;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::config::{AppAction, KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};
use crate::notify::Notify;
use crate::pi;
use crate::state::ScannedSession;
use crate::terminal::TerminalSelectionPoint;

use super::backend::{terminal_selection_point_for_cell_rect, HarnessCore, ShortcutOutcome};
use super::cell_surface::{
    display_cell_width, draw_box, render_cell_scrollbar, truncate_to_cells, CellSurface,
};
use super::layout::{compute_cell_layout, sidebar_content_rect, CellLayout, CellRect as Rect};
use super::scene::{
    harness_scene_layout, render_harness_scene, statusbar_new_project_rect, HarnessMode,
    ScenePalette, TerminalCursorMode,
};
use super::sidebar::SIDEBAR_SPINNER_FRAME_MS;
use super::theme::{MUTED, STATUS_BG, TERM_FG};

mod ansi;
mod input;
mod raw;

use ansi::{AnsiRenderer, DEFAULT_BG as TUI_DEFAULT_BG, DEFAULT_FG as TUI_DEFAULT_FG};
use input::{
    key_stroke_for_bytes, mouse_event_for_bytes, MouseEvent, MouseEventKind, WheelDirection,
};
use raw::{spawn_stdin_reader, terminal_size, RawTerminal};

const TUI_WHEEL_LINES: i32 = 3;
const TUI_BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const TUI_BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const TUI_QUIT_HINT: &str = "ctrl+q quit";
const TUI_COMMAND_HINT: &str = ": cmd";

#[derive(Debug)]
enum TuiEvent {
    Input(Vec<u8>),
    Wake,
}

pub fn run(initial_project_paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    let notify = tui_notify(tx.clone());
    let mut app = TuiApp::new(notify, initial_project_paths)?;
    let _raw_terminal = RawTerminal::enter()?;
    spawn_stdin_reader(tx);
    app.run(rx)
}

fn tui_notify(tx: mpsc::Sender<TuiEvent>) -> Notify {
    Arc::new(move || {
        let _ = tx.send(TuiEvent::Wake);
    })
}

#[derive(Clone, Debug)]
struct CommandLineState {
    input: String,
    cursor: usize,
}

impl CommandLineState {
    fn with_input(input: impl Into<String>) -> Self {
        let input = input.into();
        let cursor = input.len();
        Self { input, cursor }
    }

    fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn backspace(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.input.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    fn delete(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.input.drain(self.cursor..next);
        true
    }

    fn delete_word_back(&mut self) -> bool {
        let original = self.cursor;
        while self
            .input
            .get(..self.cursor)
            .and_then(|text| text.chars().next_back())
            .is_some_and(char::is_whitespace)
        {
            self.backspace();
        }
        while self
            .input
            .get(..self.cursor)
            .and_then(|text| text.chars().next_back())
            .is_some_and(|ch| !ch.is_whitespace())
        {
            self.backspace();
        }
        self.cursor != original
    }

    fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        true
    }

    fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.cursor = next;
        true
    }

    fn move_home(&mut self) -> bool {
        let changed = self.cursor != 0;
        self.cursor = 0;
        changed
    }

    fn move_end(&mut self) -> bool {
        let changed = self.cursor != self.input.len();
        self.cursor = self.input.len();
        changed
    }

    fn visible_text_and_cursor_col(&self, max_cols: usize) -> (String, i32) {
        let max_cols = max_cols.max(1);
        let full = format!(":{}", self.input);
        let cursor_byte = 1 + self.cursor;
        let full_cells = display_cell_width(&full);
        let cursor_cells = display_cell_width(&full[..cursor_byte]);
        if full_cells <= max_cols {
            return (full, cursor_cells.min(max_cols.saturating_sub(1)) as i32);
        }

        let marker = "…";
        let marker_cells = display_cell_width(marker);
        if max_cols <= marker_cells {
            return (marker.to_string(), 0);
        }

        let left_overflow = cursor_cells > max_cols.saturating_sub(marker_cells);
        let start_byte = if left_overflow {
            byte_index_at_cell_width(
                &full,
                cursor_cells.saturating_sub(max_cols.saturating_sub(marker_cells)),
            )
        } else {
            0
        };
        let available_cols = if start_byte > 0 {
            max_cols.saturating_sub(marker_cells)
        } else {
            max_cols
        };
        let mut visible = String::new();
        if start_byte > 0 {
            visible.push_str(marker);
        }
        visible.push_str(&prefix_to_cells(&full[start_byte..], available_cols));

        let cursor_col = (if start_byte > 0 { marker_cells } else { 0 })
            + display_cell_width(&full[start_byte..cursor_byte]).min(available_cols);
        (visible, cursor_col.min(max_cols.saturating_sub(1)) as i32)
    }
}

fn previous_char_boundary(value: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return None;
    }
    value[..index]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_char_boundary(value: &str, index: usize) -> Option<usize> {
    if index >= value.len() {
        return None;
    }
    value[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .or(Some(value.len()))
}

fn byte_index_at_cell_width(value: &str, target_cells: usize) -> usize {
    if target_cells == 0 {
        return 0;
    }
    let mut cells = 0;
    for (index, ch) in value.char_indices() {
        if cells >= target_cells {
            return index;
        }
        cells += command_char_width(ch);
    }
    value.len()
}

fn prefix_to_cells(value: &str, max_cols: usize) -> String {
    let mut output = String::new();
    let mut cells = 0;
    for ch in value.chars() {
        let width = command_char_width(ch);
        if cells + width > max_cols {
            break;
        }
        output.push(ch);
        cells += width;
    }
    output
}

fn command_char_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TuiCommand {
    Open(PathBuf),
    Archive,
    Refresh,
    Reload,
    Quit,
    Help,
}

fn parse_command(input: &str) -> Result<TuiCommand, String> {
    let input = input
        .trim()
        .strip_prefix(':')
        .unwrap_or(input.trim())
        .trim();
    if input.is_empty() {
        return Err("empty command".to_string());
    }

    let name_end = input.find(char::is_whitespace).unwrap_or(input.len());
    let name = input[..name_end].to_ascii_lowercase();
    let rest = input[name_end..].trim();
    match name.as_str() {
        "open" | "o" => {
            let path = parse_path_argument(rest)?;
            Ok(TuiCommand::Open(expand_home_path(&path)))
        }
        "archive" | "archives" => Ok(TuiCommand::Archive),
        "refresh" => Ok(TuiCommand::Refresh),
        "reload" => Ok(TuiCommand::Reload),
        "q" | "quit" => Ok(TuiCommand::Quit),
        "h" | "help" => Ok(TuiCommand::Help),
        _ => Err(format!("unknown command: :{name}")),
    }
}

fn parse_path_argument(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("usage: :open <dir>".to_string());
    }

    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return Err("usage: :open <dir>".to_string());
    };
    if first != '\'' && first != '"' {
        return Ok(input.to_string());
    }
    if !input.ends_with(first) || input.len() == first.len_utf8() {
        return Err("open: unterminated quoted path".to_string());
    }
    Ok(input[first.len_utf8()..input.len() - first.len_utf8()].to_string())
}

fn expand_home_path(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir_path();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir_path().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
}

#[derive(Clone, Debug)]
struct ArchiveViewerState {
    sessions: Vec<ScannedSession>,
    selected: usize,
    scroll: usize,
    note: Option<String>,
}

impl ArchiveViewerState {
    fn load() -> Self {
        let mut viewer = Self {
            sessions: pi::scan_archived_sessions(),
            selected: 0,
            scroll: 0,
            note: None,
        };
        viewer.clamp_selection();
        viewer
    }

    fn reload_sessions(&mut self) {
        let selected_id = self
            .selected_session()
            .map(|session| session.session_id.clone());
        self.sessions = pi::scan_archived_sessions();
        self.selected = selected_id
            .as_deref()
            .and_then(|id| {
                self.sessions
                    .iter()
                    .position(|session| session.session_id == id)
            })
            .unwrap_or(0);
        self.clamp_selection();
        self.scroll = self.scroll.min(self.sessions.len().saturating_sub(1));
    }

    fn selected_session(&self) -> Option<&ScannedSession> {
        self.sessions.get(self.selected)
    }

    fn move_selection(&mut self, delta: i32) {
        if self.sessions.is_empty() {
            return;
        }
        let max = self.sessions.len().saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + delta).clamp(0, max) as usize;
    }

    fn page_selection(&mut self, delta_pages: i32, visible_rows: usize) {
        let step = visible_rows.max(1) as i32;
        self.move_selection(delta_pages.saturating_mul(step));
    }

    fn select_first(&mut self) {
        self.selected = 0;
    }

    fn select_last(&mut self) {
        self.selected = self.sessions.len().saturating_sub(1);
    }

    fn ensure_selection_visible(&mut self, visible_rows: usize) {
        if self.sessions.is_empty() || visible_rows == 0 {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected.saturating_sub(visible_rows.saturating_sub(1));
        }
        self.scroll = self
            .scroll
            .min(self.sessions.len().saturating_sub(visible_rows));
    }

    fn clamp_selection(&mut self) {
        if self.sessions.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.sessions.len() - 1);
        }
    }
}

fn archive_viewer_visible_rows_for_terminal() -> usize {
    let (cols, rows) = terminal_size();
    archive_viewer_list_rows(archive_viewer_rect(i32::from(cols), i32::from(rows)))
}

fn archive_viewer_rect(cols: i32, rows: i32) -> Rect {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let width = cols.min(110).max(1);
    let height = rows.min(32).max(1);
    Rect::new((cols - width) / 2, (rows - height) / 2, width, height)
}

fn archive_viewer_list_rows(rect: Rect) -> usize {
    rect.rows.saturating_sub(5) as usize
}

fn render_archive_viewer(surface: &mut CellSurface, viewer: &mut ArchiveViewerState) {
    let rect = archive_viewer_rect(surface.cols, surface.rows);
    draw_box(surface, rect, TUI_DEFAULT_FG, TUI_DEFAULT_BG, MUTED);
    if rect.cols <= 2 || rect.rows <= 2 {
        return;
    }

    let inner = rect.inset_edges(1, 1, 1, 1);
    surface.put_text(
        rect.col + 2,
        rect.row,
        rect.cols - 4,
        TUI_DEFAULT_FG,
        TUI_DEFAULT_BG,
        " ARCHIVE ",
    );
    let count = format!(" {} archived ", viewer.sessions.len());
    let count_col = rect.col + rect.cols - display_cell_width(&count) as i32 - 2;
    if count_col > rect.col + 2 {
        surface.put_text(
            count_col,
            rect.row,
            rect.cols - 2,
            MUTED,
            TUI_DEFAULT_BG,
            &count,
        );
    }

    let hint = "↑/↓/j/k select  Enter restore  r reload  q/Esc close";
    surface.put_text(
        inner.col,
        inner.row,
        inner.cols,
        MUTED,
        TUI_DEFAULT_BG,
        hint,
    );
    let header_row = inner.row + 1;
    surface.put_text(
        inner.col,
        header_row,
        inner.cols,
        TUI_DEFAULT_FG,
        TUI_DEFAULT_BG,
        "Updated  Session / project",
    );

    let list_row = inner.row + 2;
    let footer_row = rect.row + rect.rows - 2;
    let list_rows = (footer_row - list_row).max(0) as usize;
    let list_width = (inner.cols - 1).max(0);
    viewer.ensure_selection_visible(list_rows);

    if viewer.sessions.is_empty() {
        surface.put_text(
            inner.col,
            list_row,
            list_width,
            MUTED,
            TUI_DEFAULT_BG,
            "No archived sessions. Ctrl+Delete archives the selected session.",
        );
    } else {
        let now_ms = crate::util::now_millis();
        let end = (viewer.scroll + list_rows).min(viewer.sessions.len());
        for (row_offset, index) in (viewer.scroll..end).enumerate() {
            let row = list_row + row_offset as i32;
            let selected = index == viewer.selected;
            let line = archive_viewer_row_text(&viewer.sessions[index], now_ms);
            let row_rect = Rect::new(inner.col, row, list_width, 1);
            surface.put_text_styled(
                inner.col,
                row,
                list_width,
                TUI_DEFAULT_FG,
                TUI_DEFAULT_BG,
                &truncate_to_cells(&line, list_width as usize),
                selected,
            );
            if selected {
                surface.set_reverse_rect(row_rect, true);
            }
        }
        render_cell_scrollbar(
            surface,
            inner.col + inner.cols - 1,
            list_row,
            list_rows as i32,
            list_rows,
            viewer.sessions.len(),
            viewer.scroll,
            MUTED,
            TUI_DEFAULT_BG,
            "│",
            TUI_DEFAULT_FG,
            "█",
        );
    }

    let footer = viewer
        .note
        .as_deref()
        .unwrap_or("Restores selected archive to its original project cwd.");
    surface.put_text(
        inner.col,
        footer_row,
        inner.cols,
        MUTED,
        TUI_DEFAULT_BG,
        &truncate_to_cells(footer, inner.cols.max(0) as usize),
    );
}

fn archive_viewer_row_text(session: &ScannedSession, now_ms: u64) -> String {
    format!(
        "{:<7} {}  {}",
        archive_age_label(now_ms, session.updated_at_ms),
        session.name,
        session.cwd.display()
    )
}

fn archive_age_label(now_ms: u64, updated_ms: u64) -> String {
    if updated_ms == 0 {
        return "unknown".to_string();
    }
    let age_secs = now_ms.saturating_sub(updated_ms) / 1000;
    if age_secs < 60 {
        "now".to_string()
    } else if age_secs < 60 * 60 {
        format!("{}m ago", age_secs / 60)
    } else if age_secs < 60 * 60 * 24 {
        format!("{}h ago", age_secs / (60 * 60))
    } else if age_secs < 60 * 60 * 24 * 30 {
        format!("{}d ago", age_secs / (60 * 60 * 24))
    } else if age_secs < 60 * 60 * 24 * 365 {
        format!("{}mo ago", age_secs / (60 * 60 * 24 * 30))
    } else {
        format!("{}y ago", age_secs / (60 * 60 * 24 * 365))
    }
}

struct TuiApp {
    core: HarnessCore,
    last_size: Option<(u16, u16)>,
    needs_redraw: bool,
    host_bracketed_paste: Option<Vec<u8>>,
    command_line: Option<CommandLineState>,
    archive_viewer: Option<ArchiveViewerState>,
}

impl TuiApp {
    fn new(notify: Notify, initial_project_paths: Vec<PathBuf>) -> anyhow::Result<Self> {
        let core = HarnessCore::new(notify, initial_project_paths)?;
        let mut app = Self {
            core,
            last_size: None,
            needs_redraw: true,
            host_bracketed_paste: None,
            command_line: None,
            archive_viewer: None,
        };
        app.core.sync_terminals();
        Ok(app)
    }
}

impl TuiApp {
    fn run(&mut self, rx: mpsc::Receiver<TuiEvent>) -> anyhow::Result<()> {
        let mut stdout = io::stdout();
        let mut renderer = AnsiRenderer::default();

        loop {
            self.process_background_events();
            self.check_resize();

            if self.drain_pending_events(&rx) {
                break;
            }

            if self.needs_redraw {
                self.render(&mut stdout, &mut renderer)?;
                self.needs_redraw = false;
            }

            let timeout = if self.core.has_sidebar_spinner() {
                Duration::from_millis(SIDEBAR_SPINNER_FRAME_MS)
            } else {
                Duration::from_millis(250)
            };

            match rx.recv_timeout(timeout) {
                Ok(event) => {
                    if self.handle_event(event) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.core.has_sidebar_spinner() {
                        self.needs_redraw = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }

    fn drain_pending_events(&mut self, rx: &mpsc::Receiver<TuiEvent>) -> bool {
        let mut should_quit = false;
        while let Ok(event) = rx.try_recv() {
            should_quit |= self.handle_event(event);
            if should_quit {
                break;
            }
        }
        should_quit
    }

    fn handle_event(&mut self, event: TuiEvent) -> bool {
        match event {
            TuiEvent::Input(bytes) => {
                let should_quit = self.handle_input(&bytes);
                self.needs_redraw = true;
                should_quit
            }
            TuiEvent::Wake => false,
        }
    }

    fn check_resize(&mut self) {
        let size = terminal_size();
        if self.last_size != Some(size) {
            self.last_size = Some(size);
            self.needs_redraw = true;
        }
    }

    fn render(
        &mut self,
        stdout: &mut io::Stdout,
        renderer: &mut AnsiRenderer,
    ) -> anyhow::Result<()> {
        let (cols, rows) = terminal_size();
        let layout = compute_cell_layout(cols, rows, self.core.config.layout_widths());
        let visible_sidebar_rows = sidebar_content_rect(layout.sidebar).rows.max(0) as usize;
        let frame_model = self.core.prepare_frame(
            layout.terminal.rows.max(1) as u16,
            layout.terminal.cols.max(1) as u16,
            visible_sidebar_rows,
        );

        let mut palette =
            ScenePalette::monochrome(TUI_DEFAULT_FG, TUI_DEFAULT_BG, TERM_FG, TUI_DEFAULT_BG);
        palette.border = MUTED;
        palette.muted = MUTED;
        palette.statusbar_bg = STATUS_BG;
        let mut surface =
            CellSurface::new(i32::from(cols), i32::from(rows), palette.fg, palette.bg);
        surface.fill_rect(
            Rect::new(0, 0, i32::from(cols), i32::from(rows)),
            palette.fg,
            palette.bg,
        );

        let footer_hint = format!(" {TUI_QUIT_HINT}  {TUI_COMMAND_HINT} ");
        let mut hardware_cursor = render_harness_scene(
            &mut surface,
            harness_scene_layout(&layout),
            &frame_model,
            None,
            &palette,
            TerminalCursorMode::Hardware,
            self.current_mode(),
            Some(&footer_hint),
            crate::util::now_millis(),
        );
        if let Some(command_cursor) = self.render_command_line_overlay(&mut surface) {
            hardware_cursor = Some(command_cursor);
        }
        if let Some(viewer) = &mut self.archive_viewer {
            render_archive_viewer(&mut surface, viewer);
            hardware_cursor = None;
        }
        renderer.render(stdout, &surface, hardware_cursor)?;
        Ok(())
    }

    fn current_mode(&self) -> HarnessMode {
        if self.command_line.is_some() {
            HarnessMode::Command
        } else {
            HarnessMode::Normal
        }
    }

    fn render_command_line_overlay(
        &self,
        surface: &mut CellSurface,
    ) -> Option<super::scene::HardwareCursor> {
        let command_line = self.command_line.as_ref()?;
        let row = surface.rows.saturating_sub(1);
        let command_rect = Rect::new(0, row, surface.cols, 1);
        surface.fill_rect(command_rect, TUI_DEFAULT_FG, TUI_DEFAULT_BG);
        let (visible_text, cursor_col) =
            command_line.visible_text_and_cursor_col(surface.cols.max(1) as usize);
        surface.put_text(
            0,
            row,
            surface.cols,
            TUI_DEFAULT_FG,
            TUI_DEFAULT_BG,
            &visible_text,
        );
        Some(super::scene::HardwareCursor {
            col: cursor_col,
            row,
        })
    }

    fn handle_input(&mut self, bytes: &[u8]) -> bool {
        if bytes == [0x11] {
            return true;
        }

        if self.archive_viewer.is_some() {
            self.handle_archive_viewer_input(bytes);
            return false;
        }

        if self.handle_host_bracketed_paste(bytes) {
            return false;
        }

        if let Some(should_quit) = self.handle_command_line_input(bytes) {
            return should_quit;
        }

        if let Some(event) = mouse_event_for_bytes(bytes) {
            self.handle_mouse_event(event);
            return false;
        }

        if let Some(stroke) = key_stroke_for_bytes(bytes) {
            if self.handle_scroll_key(&stroke) {
                return false;
            }
            if self.core.handle_hjkl_navigation_shortcut(&stroke) {
                self.core.clear_pending_key_chord();
                return false;
            }
            match self.core.handle_shortcut_stroke(stroke) {
                ShortcutOutcome::NoMatch => {}
                ShortcutOutcome::Pending => return false,
                ShortcutOutcome::Triggered => return false,
            }
        } else if !bytes.is_empty() {
            self.core.clear_pending_key_chord();
        }

        self.core
            .send_bytes_to_current_terminal(bytes, "terminal input");
        false
    }

    fn handle_command_line_input(&mut self, bytes: &[u8]) -> Option<bool> {
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
        if bytes == [0x08] {
            self.command_line_backspace_or_cancel();
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

    fn handle_command_line_stroke(&mut self, stroke: KeyStroke) -> Option<bool> {
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

    fn start_command_line(&mut self, initial: &str) {
        self.command_line = Some(CommandLineState::with_input(initial));
    }

    fn cancel_command_line(&mut self) {
        self.command_line = None;
        self.core.clear_pending_key_chord();
    }

    fn command_line_backspace_or_cancel(&mut self) {
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

    fn insert_command_line_bytes(&mut self, bytes: &[u8]) -> bool {
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

    fn submit_command_line(&mut self) -> bool {
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
            Ok(TuiCommand::Quit) => return true,
            Ok(TuiCommand::Help) => self
                .core
                .set_note_text("commands: :open <dir>, :archive, :refresh, :reload, :quit"),
            Err(note) => self.core.set_note_text(note),
        }
        false
    }

    fn open_archive_viewer(&mut self) {
        self.archive_viewer = Some(ArchiveViewerState::load());
    }

    fn handle_archive_viewer_input(&mut self, bytes: &[u8]) {
        if bytes == [0x03] {
            self.archive_viewer = None;
            return;
        }

        let Some(stroke) = key_stroke_for_bytes(bytes) else {
            return;
        };
        let no_modifiers = stroke.modifiers == KeyModifiers::default();
        if no_modifiers {
            match stroke.key {
                KeyToken::Named(NamedKeyToken::Escape) => self.archive_viewer = None,
                KeyToken::Named(NamedKeyToken::Enter) => self.restore_selected_archive_session(),
                KeyToken::Named(NamedKeyToken::Up) => self.move_archive_selection(-1),
                KeyToken::Named(NamedKeyToken::Down) => self.move_archive_selection(1),
                KeyToken::Named(NamedKeyToken::PageUp) => self.page_archive_selection(-1),
                KeyToken::Named(NamedKeyToken::PageDown) => self.page_archive_selection(1),
                KeyToken::Named(NamedKeyToken::Home) => self.select_first_archive_session(),
                KeyToken::Named(NamedKeyToken::End) => self.select_last_archive_session(),
                KeyToken::Character(ref key) if key == "q" => self.archive_viewer = None,
                KeyToken::Character(ref key) if key == "j" => self.move_archive_selection(1),
                KeyToken::Character(ref key) if key == "k" => self.move_archive_selection(-1),
                KeyToken::Character(ref key) if key == "g" => self.select_first_archive_session(),
                KeyToken::Character(ref key) if key == "r" => self.reload_archive_viewer(),
                _ => {}
            }
            return;
        }

        let ctrl_only =
            stroke.modifiers.control && !stroke.modifiers.shift && !stroke.modifiers.alt;
        if ctrl_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "c") {
            self.archive_viewer = None;
            return;
        }

        let shift_only =
            stroke.modifiers.shift && !stroke.modifiers.control && !stroke.modifiers.alt;
        if shift_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "g") {
            self.select_last_archive_session();
        }
    }

    fn move_archive_selection(&mut self, delta: i32) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.move_selection(delta);
        }
    }

    fn page_archive_selection(&mut self, delta_pages: i32) {
        let visible_rows = archive_viewer_visible_rows_for_terminal();
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.page_selection(delta_pages, visible_rows);
        }
    }

    fn select_first_archive_session(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.select_first();
        }
    }

    fn select_last_archive_session(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.select_last();
        }
    }

    fn reload_archive_viewer(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.reload_sessions();
            viewer.note = Some(format!("{} archived sessions", viewer.sessions.len()));
        }
    }

    fn restore_selected_archive_session(&mut self) {
        let archived = self
            .archive_viewer
            .as_ref()
            .and_then(ArchiveViewerState::selected_session)
            .cloned();
        let Some(archived) = archived else {
            if let Some(viewer) = &mut self.archive_viewer {
                viewer.note = Some("no archived sessions".to_string());
            }
            return;
        };

        match self.core.restore_archived_session(&archived) {
            Ok(()) => self.archive_viewer = None,
            Err(error) => {
                if let Some(viewer) = &mut self.archive_viewer {
                    viewer.reload_sessions();
                    viewer.note = Some(error);
                }
            }
        }
    }

    fn handle_host_bracketed_paste(&mut self, bytes: &[u8]) -> bool {
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

    fn forward_host_paste(&mut self, paste: &[u8]) {
        if self.command_line.is_some() {
            self.insert_command_line_bytes(paste);
        } else {
            self.core.paste_bytes_to_current_terminal(paste);
        }
    }

    fn handle_scroll_key(&mut self, stroke: &crate::config::KeyStroke) -> bool {
        self.core.handle_terminal_scroll_key(stroke)
    }

    fn handle_mouse_event(&mut self, event: MouseEvent) {
        let (cols, rows) = terminal_size();
        let layout = compute_cell_layout(cols, rows, self.core.config.layout_widths());

        match event.kind {
            MouseEventKind::Wheel(direction) => self.handle_mouse_wheel(event, direction, &layout),
            MouseEventKind::LeftPress => self.handle_left_mouse_press(event, &layout),
            MouseEventKind::LeftDrag => self.handle_left_mouse_drag(event, &layout),
            MouseEventKind::LeftRelease => self.handle_left_mouse_release(),
            MouseEventKind::Other => {}
        }
    }

    fn handle_mouse_wheel(
        &mut self,
        event: MouseEvent,
        direction: WheelDirection,
        layout: &CellLayout,
    ) {
        let delta = match direction {
            WheelDirection::Up => TUI_WHEEL_LINES,
            WheelDirection::Down => -TUI_WHEEL_LINES,
        };

        if layout.sidebar.contains_cell(event.col, event.row) {
            let visible_rows = sidebar_content_rect(layout.sidebar).rows.max(0) as usize;
            let row_count = self.core.sidebar_rows().len();
            self.core
                .scroll_sidebar_from_wheel(delta, visible_rows, row_count);
            return;
        }

        if layout.terminal_card.contains_cell(event.col, event.row) {
            self.core.scroll_terminal_by_lines(delta);
        }
    }

    fn handle_left_mouse_press(&mut self, event: MouseEvent, layout: &CellLayout) {
        self.core.set_terminal_selection_in_progress(false);
        if self.handle_statusbar_click(event, layout) {
            return;
        }
        if self.handle_sidebar_click(event, layout) {
            return;
        }

        let point = self.terminal_selection_point_for_mouse(event, layout);
        self.core.clear_or_begin_terminal_selection(point);
    }

    fn handle_left_mouse_drag(&mut self, event: MouseEvent, layout: &CellLayout) {
        let Some(point) = self.terminal_selection_point_for_mouse(event, layout) else {
            return;
        };
        self.core.update_terminal_selection(point);
    }

    fn handle_left_mouse_release(&mut self) {
        self.core.finish_terminal_selection();
    }

    fn handle_statusbar_click(&mut self, event: MouseEvent, layout: &CellLayout) -> bool {
        if !layout.statusbar.contains_cell(event.col, event.row) {
            return false;
        }
        let sidebar_panel =
            (layout.sidebar.cols > 0 && layout.sidebar.rows > 0).then_some(layout.sidebar);
        if statusbar_new_project_rect(layout.statusbar, sidebar_panel, self.current_mode())
            .is_some_and(|rect| rect.contains_cell(event.col, event.row))
        {
            self.start_command_line("open ");
        }
        true
    }

    fn handle_sidebar_click(&mut self, event: MouseEvent, layout: &CellLayout) -> bool {
        let content = sidebar_content_rect(layout.sidebar);
        if !content.contains_cell(event.col, event.row) {
            return false;
        }
        let visible_rows = content.rows.max(0) as usize;
        let visible_row = (event.row - content.row).max(0) as usize;
        let rows = self.core.sidebar_rows();
        let Some(row_index) =
            self.core
                .sidebar_row_index_at_visible_row(&rows, visible_rows, visible_row)
        else {
            return true;
        };
        let Some(row_kind) = rows.get(row_index).map(|row| row.kind.clone()) else {
            return true;
        };
        self.core.activate_sidebar_row(&row_kind);
        true
    }

    fn terminal_selection_point_for_mouse(
        &self,
        event: MouseEvent,
        layout: &CellLayout,
    ) -> Option<TerminalSelectionPoint> {
        let (rows, cols) = self
            .core
            .current_terminal()
            .map(|terminal| terminal.screen().size())
            .unwrap_or((
                layout.terminal.rows.max(1) as u16,
                layout.terminal.cols.max(1) as u16,
            ));
        terminal_selection_point_for_cell_rect(layout.terminal, rows, cols, event.col, event.row)
    }

    fn process_background_events(&mut self) {
        if self.core.process_background_events() {
            self.needs_redraw = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_command_accepts_colon_prefix_and_spaces() {
        assert_eq!(
            parse_command(":open /tmp/project path").unwrap(),
            TuiCommand::Open(PathBuf::from("/tmp/project path"))
        );
    }

    #[test]
    fn parse_open_command_strips_matching_quotes() {
        assert_eq!(
            parse_command("open \"/tmp/project path\"").unwrap(),
            TuiCommand::Open(PathBuf::from("/tmp/project path"))
        );
    }

    #[test]
    fn parse_open_command_expands_home_prefix() {
        let home = home_dir_path();
        assert_eq!(
            parse_command("open ~/project").unwrap(),
            TuiCommand::Open(home.join("project"))
        );
    }

    #[test]
    fn parse_refresh_and_reload_commands() {
        assert_eq!(parse_command("refresh").unwrap(), TuiCommand::Refresh);
        assert_eq!(parse_command(":reload").unwrap(), TuiCommand::Reload);
    }

    #[test]
    fn parse_archive_command() {
        assert_eq!(parse_command("archive").unwrap(), TuiCommand::Archive);
        assert_eq!(parse_command(":archives").unwrap(), TuiCommand::Archive);
    }
    #[test]
    fn command_line_backspace_updates_utf8_cursor() {
        let mut command_line = CommandLineState::with_input("open café");
        assert!(command_line.backspace());
        assert_eq!(command_line.input, "open caf");
        command_line.move_left();
        command_line.insert_str("é");
        assert_eq!(command_line.input, "open caéf");
    }
}
