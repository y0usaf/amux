use std::io;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::notify::Notify;

use super::backend::{HarnessCore, ShortcutOutcome};
use super::cell_surface::CellSurface;
use super::layout::{compute_cell_layout, sidebar_content_rect, CellLayout, CellRect as Rect};
use super::scene::{
    harness_scene_layout, render_harness_scene, HarnessMode, ScenePalette, TerminalCursorMode,
};
use super::sidebar::SIDEBAR_ANIMATION_FRAME_MS;
use super::theme::{DerivedTheme, TerminalPalette};

mod ansi;
mod input;
mod raw;

use ansi::AnsiRenderer;
use input::{key_stroke_for_bytes, mouse_event_for_bytes};
use raw::{spawn_stdin_reader, terminal_size, RawTerminal};

const TUI_WHEEL_LINES: i32 = 3;
const TUI_BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const TUI_BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

const TUI_THEME_QUERY_INTERVAL: Duration = Duration::from_secs(1);
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
    app.inherit_terminal_theme();
    spawn_stdin_reader(tx);
    app.run(rx)
}

fn tui_notify(tx: mpsc::Sender<TuiEvent>) -> Notify {
    Arc::new(move || {
        let _ = tx.send(TuiEvent::Wake);
    })
}

mod command;
mod command_line;
mod command_mode;
mod mouse;
mod overlay_controller;
mod overlays;
mod paste;
mod theme_query;

use command_line::{command_line_start_col, CommandLineState};
use overlays::archive::{render_archive_viewer, ArchiveViewerState};
use overlays::help::{render_help_overlay, HelpOverlayState};
use overlays::usage::{render_usage_overlay, UsageOverlayState};
use theme_query::{
    apply_terminal_palette_response, is_terminal_palette_response, parse_terminal_palette_response,
};

struct TuiApp {
    core: HarnessCore,
    last_size: Option<(u16, u16)>,
    needs_redraw: bool,
    host_bracketed_paste: Option<Vec<u8>>,
    next_animation_redraw: Option<Instant>,
    command_line: Option<CommandLineState>,
    archive_viewer: Option<ArchiveViewerState>,
    help_overlay: Option<HelpOverlayState>,
    usage_overlay: Option<UsageOverlayState>,
    theme: DerivedTheme,
    terminal_palette: TerminalPalette,
    last_theme_query: Instant,
}

impl TuiApp {
    fn new(notify: Notify, initial_project_paths: Vec<PathBuf>) -> anyhow::Result<Self> {
        let core = HarnessCore::new(notify, initial_project_paths)?;
        let mut app = Self {
            core,
            last_size: None,
            needs_redraw: true,
            next_animation_redraw: None,
            host_bracketed_paste: None,
            command_line: None,
            archive_viewer: None,
            help_overlay: None,
            usage_overlay: None,
            theme: DerivedTheme::fallback(),
            terminal_palette: TerminalPalette::fallback(),
            last_theme_query: Instant::now(),
        };
        app.core.sync_terminals();
        Ok(app)
    }

    fn inherit_terminal_theme(&mut self) {
        if let Ok(response) = raw::query_terminal_palette_response(Duration::from_millis(120)) {
            if let Some(palette) = parse_terminal_palette_response(&response) {
                self.terminal_palette = palette;
                self.theme = DerivedTheme::from_terminal_palette(palette);
            }
            self.last_theme_query = Instant::now();
        }
    }

    fn request_terminal_theme_now(&mut self) {
        self.last_theme_query = Instant::now();
        let _ = raw::request_terminal_palette_query();
    }

    fn maybe_query_terminal_theme(&mut self) {
        if self.last_theme_query.elapsed() >= TUI_THEME_QUERY_INTERVAL {
            self.request_terminal_theme_now();
        }
    }



    fn handle_terminal_palette_response(&mut self, bytes: &[u8]) -> bool {
        if !is_terminal_palette_response(bytes) {
            return false;
        }
        if apply_terminal_palette_response(bytes, &mut self.terminal_palette) {
            self.theme = DerivedTheme::from_terminal_palette(self.terminal_palette);
            self.needs_redraw = true;
        }
        true
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

            self.maybe_query_terminal_theme();
            let animation_active = self.visible_sidebar_animation_active();
            self.schedule_animation_redraw(animation_active);

            if self.needs_redraw {
                self.render(&mut stdout, &mut renderer)?;
                self.needs_redraw = false;
            }

            let timeout = self.animation_timeout(animation_active);

            match rx.recv_timeout(timeout) {
                Ok(event) => {
                    if self.handle_event(event) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.visible_sidebar_animation_active() {
                        self.needs_redraw = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }

    fn visible_sidebar_animation_active(&self) -> bool {
        if self.archive_viewer.is_some()
            || self.help_overlay.is_some()
            || self.usage_overlay.is_some()
        {
            return false;
        }
        let (cols, rows) = terminal_size();
        let layout = compute_cell_layout(cols, rows, self.core.config.layout_widths());
        let visible_sidebar_rows = sidebar_content_rect(layout.sidebar).rows.max(0) as usize;
        self.core.visible_sidebar_has_spinner(visible_sidebar_rows)
    }

    fn schedule_animation_redraw(&mut self, active: bool) {
        if !active {
            self.next_animation_redraw = None;
            return;
        }

        let now = Instant::now();
        let frame = Duration::from_millis(SIDEBAR_ANIMATION_FRAME_MS);
        let next = self.next_animation_redraw.get_or_insert(now);
        if now >= *next {
            self.needs_redraw = true;
            while *next <= now {
                *next += frame;
            }
        }
    }

    fn animation_timeout(&self, active: bool) -> Duration {
        if active {
            self.next_animation_redraw
                .map(|next| next.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|| Duration::from_millis(SIDEBAR_ANIMATION_FRAME_MS))
        } else {
            Duration::from_millis(80)
        }
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

        let theme = self.theme;
        let mut palette = ScenePalette::themed(theme);
        palette.border = theme.border;
        palette.muted = theme.muted;
        palette.statusbar_bg = theme.status_bg;
        palette.statusbar_fg = theme.status_fg;
        let mut surface =
            CellSurface::new(i32::from(cols), i32::from(rows), palette.fg, palette.bg);
        surface.fill_rect(
            Rect::new(0, 0, i32::from(cols), i32::from(rows)),
            palette.fg,
            palette.bg,
        );

        let mut hardware_cursor = render_harness_scene(
            &mut surface,
            harness_scene_layout(&layout),
            &frame_model,
            None,
            &palette,
            TerminalCursorMode::Hardware,
            self.current_mode(),
            crate::util::now_millis(),
        );
        if let Some(command_cursor) = self.render_command_line_overlay(&mut surface, &layout) {
            hardware_cursor = Some(command_cursor);
        }
        if let Some(viewer) = &mut self.archive_viewer {
            render_archive_viewer(&mut surface, viewer, &self.theme);
            hardware_cursor = None;
        }
        if let Some(help) = &mut self.help_overlay {
            render_help_overlay(&mut surface, help, &self.core.config, &self.theme);
            hardware_cursor = None;
        }
        if let Some(usage) = &mut self.usage_overlay {
            render_usage_overlay(&mut surface, usage, &self.theme);
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
        layout: &CellLayout,
    ) -> Option<super::scene::HardwareCursor> {
        let command_line = self.command_line.as_ref()?;
        let row = surface.rows.saturating_sub(1);
        let start_col = command_line_start_col(layout, surface.cols);
        let cols = surface.cols.saturating_sub(start_col);
        if cols <= 0 {
            return None;
        }

        let command_rect = Rect::new(start_col, row, cols, 1);
        let bg = self.theme.warning;
        let fg = self.theme.border;
        surface.fill_rect(command_rect, fg, bg);
        let (visible_text, cursor_col) = command_line.visible_text_and_cursor_col(cols as usize);
        if let Some(rest) = visible_text.strip_prefix(':') {
            surface.put_text(start_col, row, 1, fg, bg, ":");
            surface.put_text(start_col + 1, row, cols.saturating_sub(1), fg, bg, rest);
        } else {
            surface.put_text(start_col, row, cols, fg, bg, &visible_text);
        }

        Some(super::scene::HardwareCursor {
            col: start_col + cursor_col,
            row,
        })
    }

    fn handle_input(&mut self, bytes: &[u8]) -> bool {
        if self.handle_terminal_palette_response(bytes) {
            return false;
        }

        if bytes == [0x11] {
            return true;
        }

        if bytes == [0x07] {
            self.toggle_help_overlay();
            return false;
        }

        if self.help_overlay.is_some() {
            self.handle_help_overlay_input(bytes);
            return false;
        }
        if self.usage_overlay.is_some() {
            self.handle_usage_overlay_input(bytes);
            return false;
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

    fn process_background_events(&mut self) {
        if self.core.process_background_events() {
            self.needs_redraw = true;
        }
    }
}
