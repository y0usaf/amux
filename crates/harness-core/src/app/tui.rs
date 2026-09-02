use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::notify::Notify;

use super::backend::{HarnessCore, ShortcutOutcome};
use super::cell_surface::CellSurface;
use super::glyphs::GlyphSet;
use super::layout::{compute_cell_layout, sidebar_content_rect, CellLayout, CellRect as Rect};
use super::scene::{
    harness_scene_layout, render_harness_scene, HarnessMode, ScenePalette, TerminalCursorMode,
};
use super::sidebar::SIDEBAR_ANIMATION_FRAME_MS;
use super::theme::DerivedTheme;

mod ansi;
mod input;
mod keyboard;
mod raw;

use ansi::AnsiRenderer;
use input::mouse_event_for_bytes;
use keyboard::{decode_key_input, is_ctrl_char, is_ctrl_question};
use raw::{spawn_stdin_reader, terminal_size, RawTerminal};

const TUI_WHEEL_LINES: i32 = 3;
const TUI_BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const TUI_BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug)]
enum TuiEvent {
    Input(Vec<u8>),
    Wake,
}

pub fn run(initial_project_paths: Vec<PathBuf>) -> anyhow::Result<()> {
    if let Some(mode) = daemon_mode() {
        return mode.run();
    }
    let (tx, rx) = mpsc::channel();
    let wake_pending = Arc::new(AtomicBool::new(false));
    let notify = tui_notify(tx.clone(), wake_pending.clone());
    let mut app = TuiApp::new(notify, wake_pending, initial_project_paths)?;
    let _raw_terminal = RawTerminal::enter()?;
    spawn_stdin_reader(tx);
    app.run(rx)
}

/// `--daemon` daemonizes; `--daemon-foreground` runs in the current process.
fn daemon_mode() -> Option<DaemonMode> {
    if std::env::args().any(|arg| arg == "--daemon") {
        Some(DaemonMode::Background)
    } else if std::env::args().any(|arg| arg == "--daemon-foreground") {
        Some(DaemonMode::Foreground)
    } else {
        None
    }
}

enum DaemonMode {
    Background,
    Foreground,
}

impl DaemonMode {
    fn run(self) -> anyhow::Result<()> {
        let result = match self {
            Self::Background => crate::daemon::server::run_daemonized(),
            Self::Foreground => crate::daemon::server::run_foreground(),
        };
        result.map_err(|error| anyhow::anyhow!("harness daemon: {error}"))
    }
}

fn tui_notify(tx: mpsc::Sender<TuiEvent>, wake_pending: Arc<AtomicBool>) -> Notify {
    Arc::new(move || {
        if !wake_pending.swap(true, Ordering::AcqRel) {
            if tx.send(TuiEvent::Wake).is_err() {
                wake_pending.store(false, Ordering::Release);
            }
        }
    })
}

mod command;
mod command_line;
mod command_mode;
mod mouse;
mod overlay_controller;
mod overlays;
mod paste;

use command_line::{command_line_start_col, CommandLineState};
use overlays::archive::{render_archive_viewer, ArchiveViewerState};
use overlays::help::{render_help_overlay, HelpOverlayState};
use overlays::usage::{render_usage_overlay, UsageOverlayState};

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
    wake_pending: Arc<AtomicBool>,
    surface: CellSurface,
}

impl TuiApp {
    fn new(
        notify: Notify,
        wake_pending: Arc<AtomicBool>,
        initial_project_paths: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
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
            wake_pending,
            surface: CellSurface::new(
                1,
                1,
                DerivedTheme::fallback().text,
                DerivedTheme::fallback().term_bg,
            ),
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

            self.core.flush_pending_persist(false);
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

        self.core.flush_pending_persist(true);
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
            TuiEvent::Wake => {
                self.wake_pending.store(false, Ordering::Release);
                false
            }
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
        self.core.sync_rail_width(cols);
        let mode = self.current_mode();
        // Capture the style as a Copy value before prepare_frame borrows core
        // mutably, so the config borrow does not outlive the frame model.
        let glyph_style = self.core.config.glyph_style();
        let rail_cols = i32::from(self.core.config.right_rail_columns(cols));
        let frame_model = self.core.prepare_frame(
            layout.terminal.rows.max(1) as u16,
            layout.terminal.cols.max(1) as u16,
            visible_sidebar_rows,
        );

        let theme = self.theme;
        let glyphs = GlyphSet::for_style(glyph_style);
        let mut palette = ScenePalette::themed(theme);
        palette.glyphs = glyphs;
        palette.border = theme.border;
        palette.muted = theme.muted;
        palette.statusbar_bg = theme.status_bg;
        palette.statusbar_fg = theme.status_fg;
        let mut surface = std::mem::take(&mut self.surface);
        surface.reset(i32::from(cols), i32::from(rows), palette.fg, palette.bg);

        let mut hardware_cursor = render_harness_scene(
            &mut surface,
            harness_scene_layout(&layout, rail_cols),
            &frame_model,
            None,
            &palette,
            TerminalCursorMode::Hardware,
            mode,
            crate::util::now_millis(),
        );
        if let Some(command_cursor) = self.render_command_line_overlay(&mut surface, &layout) {
            hardware_cursor = Some(command_cursor);
        }
        if let Some(viewer) = &mut self.archive_viewer {
            render_archive_viewer(&mut surface, viewer, &self.theme, &glyphs);
            hardware_cursor = None;
        }
        if let Some(help) = &mut self.help_overlay {
            render_help_overlay(&mut surface, help, &self.core.config, &self.theme);
            hardware_cursor = None;
        }
        if let Some(usage) = &mut self.usage_overlay {
            render_usage_overlay(&mut surface, usage, &self.theme, &glyphs);
            hardware_cursor = None;
        }
        self.surface = renderer.render(stdout, surface, hardware_cursor)?;
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
        if is_ctrl_char(bytes, 'q') {
            return true;
        }

        if is_ctrl_question(bytes) {
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

        let mut terminal_input_bytes = None;
        if let Some(key_input) = decode_key_input(bytes) {
            if self.handle_scroll_key(&key_input.stroke) {
                return false;
            }

            match self.core.handle_shortcut_stroke(key_input.stroke) {
                ShortcutOutcome::NoMatch => {
                    terminal_input_bytes = Some(key_input.terminal_bytes);
                }
                ShortcutOutcome::Pending => return false,
                ShortcutOutcome::Triggered => return false,
            }
        } else if !bytes.is_empty() {
            self.core.clear_pending_key_chord();
        }

        let terminal_bytes = terminal_input_bytes.as_deref().unwrap_or(bytes);
        self.core
            .send_bytes_to_current_terminal(terminal_bytes, "terminal input");
        false
    }

    fn process_background_events(&mut self) {
        if self.core.process_background_events() {
            self.needs_redraw = true;
        }
        if let Some(roles) = self.core.take_theme() {
            self.theme = DerivedTheme::from_roles(roles);
            self.needs_redraw = true;
        }
    }
}
