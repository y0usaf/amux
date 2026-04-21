use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;
use std::{collections::HashSet, num::NonZeroU32};

use arboard::Clipboard;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
use arboard::{LinuxClipboardKind, SetExtLinux};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy, OwnedDisplayHandle};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::config::AppConfig;
use crate::pi::{self, PiSidecarSnapshot};
use crate::render::{Color, Frame, TextRenderer};
use crate::sidecar::SidecarListener;
use crate::state::{merge_scanned_sessions, PersistedState, Project, Session};
use crate::terminal::{
    terminal_selection_span, TerminalController, TerminalSelectionPoint, TerminalSelectionRange,
    TerminalStatus, TerminalTarget,
};
use crate::util::{normalize_project_path, now_millis};

mod actions;
mod input;
mod layout;
mod sidebar;
mod theme;

use input::{take_wheel_lines, take_zoom_steps, terminal_selection_point_for_position};
use layout::{Rect, SIDEBAR_PAD_Y, TERMINAL_PAD};
use sidebar::{
    sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind,
    SIDEBAR_SPINNER_FRAME_MS,
};
use theme::{
    status_color, terminal_cell_colors, BG, BORDER, FONT_SIZE, MUTED, SURFACE, SURFACE_ALT,
    TERM_BG, TEXT, UI_SCALE_DEFAULT, UI_SCALE_STEP, WARNING,
};

const WINDOW_W: f64 = 1280.0;
const WINDOW_H: f64 = 840.0;

pub struct App {
    proxy: EventLoopProxy<()>,
    initial_project_paths: Vec<PathBuf>,
    config: AppConfig,
    persisted: PersistedState,
    window: Option<Rc<Window>>,
    context: Option<Context<OwnedDisplayHandle>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    text: Option<TextRenderer>,
    terminals: HashMap<String, TerminalController>,
    sidecar: SidecarListener,
    sidecar_extension_path: Option<PathBuf>,
    sidecar_socket_path: PathBuf,
    projects: Vec<Project>,
    selected_project: usize,
    selected_session: Option<usize>,
    sidebar_scroll: usize,
    sidebar_sync_to_selection: bool,
    sidebar_wheel_remainder: f64,
    terminal_wheel_remainder: f64,
    zoom_wheel_remainder: f64,
    ui_scale: f32,
    modifiers: ModifiersState,
    cursor_pos: (f64, f64),
    terminal_selection_in_progress: bool,
    clipboard: Option<Clipboard>,
    note: Option<String>,
    needs_redraw: bool,
}

impl App {
    pub fn new(
        proxy: EventLoopProxy<()>,
        initial_project_paths: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        let sidecar_socket_path = pi::socket_path();
        let sidecar = SidecarListener::start(proxy.clone(), sidecar_socket_path.clone())?;
        let persisted = PersistedState::load_default().unwrap_or_default();
        let mut config = AppConfig::load_default().unwrap_or_default();
        let loaded_ui_scale = config.ui_scale.or(persisted.ui_scale);
        let ui_scale = theme::clamp_ui_scale(loaded_ui_scale.unwrap_or(UI_SCALE_DEFAULT));
        let save_config = loaded_ui_scale.is_some() && config.ui_scale != Some(ui_scale);
        config.ui_scale = Some(ui_scale);
        if save_config {
            let _ = config.save_default();
        }

        let mut app = Self {
            proxy,
            initial_project_paths,
            config,
            persisted,
            window: None,
            context: None,
            surface: None,
            text: None,
            terminals: HashMap::new(),
            sidecar,
            sidecar_extension_path: pi::extension_path(),
            sidecar_socket_path,
            projects: Vec::new(),
            selected_project: 0,
            selected_session: None,
            sidebar_scroll: 0,
            sidebar_sync_to_selection: true,
            sidebar_wheel_remainder: 0.0,
            terminal_wheel_remainder: 0.0,
            zoom_wheel_remainder: 0.0,
            ui_scale,
            modifiers: ModifiersState::default(),
            cursor_pos: (0.0, 0.0),
            terminal_selection_in_progress: false,
            clipboard: None,
            note: None,
            needs_redraw: true,
        };

        app.reload_projects_from_disk();
        if app.sidecar_extension_path.is_none() {
            app.note = Some("sidecar extension not found".to_string());
        }
        Ok(app)
    }

    fn current_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    fn current_project_mut(&mut self) -> Option<&mut Project> {
        self.projects.get_mut(self.selected_project)
    }

    fn current_session(&self) -> Option<&Session> {
        let index = self.selected_session?;
        self.current_project()?.sessions.get(index)
    }

    fn current_session_mut(&mut self) -> Option<&mut Session> {
        let index = self.selected_session?;
        self.current_project_mut()?.sessions.get_mut(index)
    }

    fn current_session_visible_in_sidebar(&self) -> bool {
        self.current_session()
            .is_some_and(Session::should_render_in_sidebar)
    }

    fn ensure_default_session_for_project(&mut self, project_index: usize) -> Option<usize> {
        let project = self.projects.get_mut(project_index)?;
        if project.sessions.is_empty() {
            project.sessions.push(Session::new_draft());
        }
        Some(0)
    }

    fn prepare_selection_change(&mut self) -> Option<(usize, usize)> {
        self.discard_selected_ephemeral_session()
    }

    fn discard_selected_ephemeral_session(&mut self) -> Option<(usize, usize)> {
        let project_index = self.selected_project;
        let session_index = self.selected_session?;
        let session_id = {
            let session = self
                .projects
                .get(project_index)?
                .sessions
                .get(session_index)?;
            if !session.is_ephemeral_draft() {
                return None;
            }
            session.local_id.clone()
        };

        self.projects
            .get_mut(project_index)?
            .sessions
            .remove(session_index);
        self.selected_session = None;
        self.terminals.remove(&session_id);
        Some((project_index, session_index))
    }

    fn adjust_session_index_after_removal(
        project_index: usize,
        session_index: usize,
        removed: Option<(usize, usize)>,
    ) -> usize {
        match removed {
            Some((removed_project, removed_session))
                if removed_project == project_index && removed_session < session_index =>
            {
                session_index - 1
            }
            _ => session_index,
        }
    }

    fn terminal_target_for_session(
        &self,
        project_index: usize,
        session_index: usize,
    ) -> Option<(String, TerminalTarget)> {
        let project = self.projects.get(project_index)?;
        let session = project.sessions.get(session_index)?;
        Some((
            session.local_id.clone(),
            TerminalTarget {
                pi_binary: None,
                sidecar_extension_path: self.sidecar_extension_path.clone(),
                sidecar_socket_path: self.sidecar_socket_path.clone(),
                harness_session_id: session.local_id.clone(),
                cwd: project.path.clone(),
                session_file: session.session_file.clone(),
            },
        ))
    }

    fn current_terminal(&self) -> Option<&TerminalController> {
        let session = self.current_session()?;
        self.terminals.get(&session.local_id)
    }

    fn current_terminal_mut(&mut self) -> Option<&mut TerminalController> {
        let session_id = self.current_session()?.local_id.clone();
        self.terminals.get_mut(&session_id)
    }

    fn current_terminal_status(&self) -> Option<&TerminalStatus> {
        Some(self.current_terminal()?.status())
    }

    fn resize_terminals(&mut self, rows: u16, cols: u16) {
        for terminal in self.terminals.values_mut() {
            terminal.resize(rows, cols);
        }
    }

    fn persist_selection(&mut self) {
        self.persisted.projects = self
            .projects
            .iter()
            .map(|project| project.selection_key())
            .collect();
        self.persisted.selected_project = self
            .current_project()
            .map(|project| project.selection_key());
        self.persisted.selected_session = self
            .current_session()
            .and_then(Session::persisted_selection_key);
        let _ = self.persisted.save_default();
    }

    fn set_note(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    fn font_size_for_scale_factor(&self, scale_factor: f64, ui_scale: f32) -> f32 {
        FONT_SIZE * scale_factor as f32 * ui_scale
    }

    fn load_text_renderer(&self, font_size: f32) -> anyhow::Result<TextRenderer> {
        TextRenderer::with_font_family(self.config.font_family(), font_size)
            .or_else(|_| TextRenderer::load(font_size))
    }

    fn set_ui_scale(&mut self, ui_scale: f32) -> bool {
        let next = theme::clamp_ui_scale(ui_scale);
        if (next - self.ui_scale).abs() < f32::EPSILON {
            return false;
        }

        if let Some(window) = self.window.as_ref() {
            let font_size = self.font_size_for_scale_factor(window.scale_factor(), next);
            match self.load_text_renderer(font_size) {
                Ok(text) => self.text = Some(text),
                Err(error) => {
                    self.set_note(format!("font: {error}"));
                    return false;
                }
            }
        }

        self.ui_scale = next;
        self.config.ui_scale = Some(next);
        let _ = self.config.save_default();
        true
    }

    fn zoom_in(&mut self) -> bool {
        self.set_ui_scale(self.ui_scale + UI_SCALE_STEP)
    }

    fn zoom_out(&mut self) -> bool {
        self.set_ui_scale(self.ui_scale - UI_SCALE_STEP)
    }

    fn zoom_reset(&mut self) -> bool {
        self.set_ui_scale(UI_SCALE_DEFAULT)
    }

    fn request_redraw(&mut self) {
        self.needs_redraw = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn update_window_title(&self) {
        let Some(window) = &self.window else { return };
        let title = match (self.current_project(), self.current_session()) {
            (Some(project), Some(session)) => {
                format!("pi-harness — {} — {}", project.name, session.name)
            }
            (Some(project), None) => format!("pi-harness — {}", project.name),
            _ => "pi-harness".to_string(),
        };
        window.set_title(&title);
    }

    fn render(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        let (layout, sidebar_visible_rows) = {
            let Some(text) = self.text.as_ref() else {
                return;
            };
            let layout = self.compute_layout(size.width as i32, size.height as i32, text);
            let sidebar_visible_rows = self.sidebar_visible_rows(layout.sidebar, text);
            (layout, sidebar_visible_rows)
        };
        self.sync_terminals();
        self.resize_terminals(layout.terminal_rows, layout.terminal_cols);

        let sidebar_rows = self.sidebar_rows();
        if self.sidebar_sync_to_selection {
            self.ensure_sidebar_selection_visible(&sidebar_rows, sidebar_visible_rows);
            self.sidebar_sync_to_selection = false;
        } else {
            self.clamp_sidebar_scroll(sidebar_rows.len(), sidebar_visible_rows);
        }

        let topbar_title = match (self.current_project(), self.current_session()) {
            (Some(project), Some(session)) => format!("{} / {}", project.name, session.name),
            (Some(project), None) => project.name.clone(),
            (None, _) => "pi-harness".to_string(),
        };
        let status = self.status_text();
        let topbar_status = self.note.clone().unwrap_or(status);
        let topbar_status_fg = if self.note.is_some() {
            WARNING
        } else {
            status_color(self.current_session(), self.current_terminal_status())
        };
        let terminal_selection = self
            .current_terminal()
            .and_then(TerminalController::selection_range);
        let screen = self
            .current_terminal()
            .map(|terminal| terminal.screen().clone())
            .unwrap_or_else(|| {
                vt100::Parser::new(layout.terminal_rows, layout.terminal_cols, 0)
                    .screen()
                    .clone()
            });

        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(text) = self.text.as_mut() else {
            return;
        };

        let _ = surface.resize(
            NonZeroU32::new(size.width).unwrap(),
            NonZeroU32::new(size.height).unwrap(),
        );
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let width = buffer.width().get() as usize;
        let height = buffer.height().get() as usize;

        let pixels: &mut [u32] = &mut buffer;
        let mut frame = Frame::new(pixels, width, height);
        frame.clear(BG);

        frame.rect(
            layout.topbar.x,
            layout.topbar.y,
            layout.topbar.w,
            layout.topbar.h,
            SURFACE,
        );
        frame.stroke_rect(
            layout.topbar.x,
            layout.topbar.y,
            layout.topbar.w,
            layout.topbar.h,
            BORDER,
        );

        frame.rect(
            layout.sidebar.x,
            layout.sidebar.y,
            layout.sidebar.w,
            layout.sidebar.h,
            SURFACE_ALT,
        );
        frame.stroke_rect(
            layout.sidebar.x,
            layout.sidebar.y,
            layout.sidebar.w,
            layout.sidebar.h,
            BORDER,
        );

        frame.rect(
            layout.terminal_card.x,
            layout.terminal_card.y,
            layout.terminal_card.w,
            layout.terminal_card.h,
            SURFACE,
        );
        frame.stroke_rect(
            layout.terminal_card.x,
            layout.terminal_card.y,
            layout.terminal_card.w,
            layout.terminal_card.h,
            BORDER,
        );
        frame.rect(
            layout.terminal.x,
            layout.terminal.y,
            layout.terminal.w,
            layout.terminal.h,
            TERM_BG,
        );

        let sidebar_status_now_ms = now_millis();

        render_topbar_frame(
            &mut frame,
            text,
            layout.topbar,
            &topbar_title,
            &topbar_status,
            topbar_status_fg,
        );
        render_sidebar_frame(
            &mut frame,
            text,
            layout.sidebar,
            &sidebar_rows,
            self.sidebar_scroll,
            sidebar_status_now_ms,
        );
        render_terminal_frame(
            &mut frame,
            text,
            layout.terminal,
            &screen,
            terminal_selection,
        );
        render_terminal_scrollback(
            &mut frame,
            layout.terminal,
            &screen,
            (text.metrics.cell_height / 2).max(4),
        );

        let _ = buffer.present();
        self.needs_redraw = false;
    }

    fn status_text(&self) -> String {
        if let Some(session) = self.current_session() {
            if let Some(tool) = session.runtime.tool_name.as_deref() {
                return format!("tool: {}", tool);
            }
            if let Some(status) = session.runtime.status.as_deref() {
                if session.runtime.queued {
                    return format!("{} · queued", status);
                }
                return status.to_string();
            }
            if session.draft {
                return "new session".to_string();
            }
        } else if self.current_project().is_some() {
            return "select a session".to_string();
        } else {
            return "open a project".to_string();
        }

        match self.current_terminal_status() {
            Some(TerminalStatus::Launching) => "launching".to_string(),
            Some(TerminalStatus::Running) => "running".to_string(),
            Some(TerminalStatus::Exited(_)) => "exited".to_string(),
            Some(TerminalStatus::Error(_)) => "error".to_string(),
            Some(TerminalStatus::Empty) | None => "idle".to_string(),
        }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) && self.has_sidebar_spinner() {
            self.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        event_loop.set_control_flow(ControlFlow::Wait);
        let attrs = WindowAttributes::default()
            .with_title("pi-harness")
            .with_inner_size(LogicalSize::new(WINDOW_W, WINDOW_H));
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        let context = Context::new(event_loop.owned_display_handle()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        let scale_factor = window.scale_factor();

        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.text = Some(
            self.load_text_renderer(self.font_size_for_scale_factor(scale_factor, self.ui_scale))
                .expect("load monospace font"),
        );
        self.sync_terminals();
        self.update_window_title();
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        self.process_background_events();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.has_sidebar_spinner() {
            event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(
                SIDEBAR_SPINNER_FRAME_MS,
            )));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if self.needs_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_some_and(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.process_background_events();
                self.render();
            }
            WindowEvent::Resized(_) => {
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let font_size = self.font_size_for_scale_factor(scale_factor, self.ui_scale);
                match self.load_text_renderer(font_size) {
                    Ok(text) => self.text = Some(text),
                    Err(error) => self.set_note(format!("font: {error}")),
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);
                if self.terminal_selection_in_progress {
                    if let (Some(window), Some(text)) = (&self.window, &self.text) {
                        let size = window.inner_size();
                        let layout =
                            self.compute_layout(size.width as i32, size.height as i32, text);
                        let (rows, cols) = self
                            .current_terminal()
                            .map(|terminal| terminal.screen().size())
                            .unwrap_or((layout.terminal_rows, layout.terminal_cols));
                        if let Some(point) = terminal_selection_point_for_position(
                            layout.terminal,
                            text,
                            rows,
                            cols,
                            self.cursor_pos.0,
                            self.cursor_pos.1,
                        ) {
                            if self
                                .current_terminal_mut()
                                .is_some_and(|terminal| terminal.update_selection(point))
                            {
                                self.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.primary_shortcut_active() {
                    let steps = take_zoom_steps(&delta, &mut self.zoom_wheel_remainder);
                    if self.set_ui_scale(self.ui_scale + UI_SCALE_STEP * steps as f32) {
                        self.request_redraw();
                    }
                    return;
                }

                if let (Some(window), Some(text)) = (&self.window, &self.text) {
                    let size = window.inner_size();
                    let layout = self.compute_layout(size.width as i32, size.height as i32, text);
                    let cell_h = text.metrics.cell_height;

                    if layout
                        .sidebar
                        .contains(self.cursor_pos.0, self.cursor_pos.1)
                    {
                        let visible_rows = self.sidebar_visible_rows(layout.sidebar, text);
                        let row_count = self.sidebar_rows().len();
                        let lines =
                            take_wheel_lines(&delta, cell_h, &mut self.sidebar_wheel_remainder);
                        if lines != 0 {
                            self.sidebar_sync_to_selection = false;
                            if self.scroll_sidebar_by_rows(-lines, visible_rows, row_count) {
                                self.request_redraw();
                            }
                        }
                    } else if layout
                        .terminal_card
                        .contains(self.cursor_pos.0, self.cursor_pos.1)
                    {
                        let lines =
                            take_wheel_lines(&delta, cell_h, &mut self.terminal_wheel_remainder);
                        if self
                            .current_terminal_mut()
                            .is_some_and(|terminal| terminal.scroll_by_lines(lines))
                        {
                            self.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.terminal_selection_in_progress = false;
                let click = if let (Some(window), Some(text)) = (&self.window, &self.text) {
                    let size = window.inner_size();
                    let layout = self.compute_layout(size.width as i32, size.height as i32, text);
                    let (rows, cols) = self
                        .current_terminal()
                        .map(|terminal| terminal.screen().size())
                        .unwrap_or((layout.terminal_rows, layout.terminal_cols));
                    let terminal_point = layout
                        .terminal
                        .contains(self.cursor_pos.0, self.cursor_pos.1)
                        .then(|| {
                            terminal_selection_point_for_position(
                                layout.terminal,
                                text,
                                rows,
                                cols,
                                self.cursor_pos.0,
                                self.cursor_pos.1,
                            )
                        })
                        .flatten();
                    let sidebar_row_index = if layout
                        .sidebar
                        .contains(self.cursor_pos.0, self.cursor_pos.1)
                    {
                        let local_y = self.cursor_pos.1 as i32
                            - layout.sidebar.y
                            - SIDEBAR_PAD_Y * text.metrics.cell_height;
                        Some(
                            self.sidebar_scroll
                                + (local_y / text.metrics.cell_height).max(0) as usize,
                        )
                    } else {
                        None
                    };
                    Some((terminal_point, sidebar_row_index))
                } else {
                    None
                };

                if let Some((terminal_point, sidebar_row_index)) = click {
                    if let Some(point) = terminal_point {
                        if let Some(terminal) = self.current_terminal_mut() {
                            terminal.begin_selection(point);
                            self.terminal_selection_in_progress = true;
                        }
                    } else {
                        self.clear_current_terminal_selection();
                    }

                    if let Some(row_index) = sidebar_row_index {
                        let rows = self.sidebar_rows();
                        if let Some(row) = rows.get(row_index) {
                            match row.kind {
                                SidebarRowKind::ActionOpenProject => self.open_project_picker(),
                                SidebarRowKind::Project(index) => self.select_project(index),
                                SidebarRowKind::Session {
                                    project_index,
                                    session_index,
                                } => self.select_session_in_project(project_index, session_index),
                                SidebarRowKind::Label => {}
                            }
                        }
                    }
                }
                self.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if self.terminal_selection_in_progress {
                    self.terminal_selection_in_progress = false;
                    self.copy_current_terminal_selection();
                    self.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.zoom_shortcut_consumed(&event) {
                    self.request_redraw();
                    return;
                }
                if self.terminal_clipboard_shortcut_consumed(&event) {
                    self.request_redraw();
                    return;
                }
                if self.shortcut_consumed(&event) {
                    self.request_redraw();
                    return;
                }
                let modifiers = self.modifiers;
                if let Some(terminal) = self.current_terminal_mut() {
                    if let Err(error) = terminal.handle_key(&event, modifiers) {
                        self.set_note(format!("terminal input: {error}"));
                    }
                }
                self.request_redraw();
            }
            _ => {}
        }
    }
}

fn render_topbar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    title: &str,
    status: &str,
    status_fg: Color,
) {
    let max_cells = ((rect.w - layout::PANEL_PAD * 2).max(0) / text.metrics.cell_width) as usize;
    let title_text = text.truncate_with_ellipsis(title, max_cells.max(8));
    let status_text = text.truncate_with_ellipsis(status, max_cells.max(8));
    let title_x = centered_text_x(rect, text, &title_text);
    let status_x = centered_text_x(rect, text, &status_text);
    let y = rect.y + layout::PANEL_PAD;

    frame.text(text, title_x, y, TEXT, &title_text);
    frame.text(
        text,
        status_x,
        y + text.metrics.cell_height,
        status_fg,
        &status_text,
    );
}

fn render_sidebar_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    rows: &[SidebarRow],
    scroll: usize,
    now_ms: u64,
) {
    let cell_w = text.metrics.cell_width;
    let cell_h = text.metrics.cell_height;
    let start_x = rect.x + layout::SIDEBAR_PAD_X * cell_w;
    let start_y = rect.y + SIDEBAR_PAD_Y * cell_h;
    let visible_rows = ((rect.h - SIDEBAR_PAD_Y * 2 * cell_h).max(0) / cell_h) as usize;
    let shows_scrollbar = visible_rows > 0 && rows.len() > visible_rows;
    let scrollbar_reserve_px = if shows_scrollbar { 8 } else { 0 };

    for (index, row) in rows.iter().skip(scroll).take(visible_rows).enumerate() {
        let y = start_y + index as i32 * cell_h;
        if let Some(bg) = row.bg {
            frame.rect(rect.x + 6, y, rect.w - 12, cell_h, bg);
        }

        let status = row.status.map(|status| {
            (
                sidebar_status_glyph(status, now_ms),
                sidebar_status_color(status),
            )
        });
        let reserved_px = if status.is_some() { cell_w * 2 } else { 0 };
        let line = text.truncate_with_ellipsis(
            &row.text,
            (((rect.w - layout::SIDEBAR_PAD_X * 2 * cell_w - scrollbar_reserve_px - reserved_px)
                .max(0)
                / cell_w) as usize)
                .max(match row.kind {
                    SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => 8,
                    SidebarRowKind::Label | SidebarRowKind::Session { .. } => 0,
                }),
        );
        let x = match row.kind {
            SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => {
                centered_text_x(rect, text, &line)
            }
            SidebarRowKind::Label | SidebarRowKind::Session { .. } => start_x,
        };
        frame.text(text, x, y, row.fg, &line);

        if let Some((glyph, color)) = status {
            let glyph_x = rect.x + rect.w
                - layout::SIDEBAR_PAD_X * cell_w
                - scrollbar_reserve_px
                - text.measure_text(glyph);
            frame.text(text, glyph_x, y, color, glyph);
        }
    }

    render_vertical_scrollbar(
        frame,
        rect.x + rect.w - 5,
        start_y,
        (visible_rows as i32 * cell_h).max(0),
        visible_rows,
        rows.len(),
        scroll,
        (cell_h / 2).max(4),
    );
}

fn render_vertical_scrollbar(
    frame: &mut Frame<'_>,
    track_x: i32,
    track_y: i32,
    track_h: i32,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    min_thumb_h: i32,
) {
    if track_h <= 0 || visible_items == 0 || total_items <= visible_items {
        return;
    }

    frame.rect(track_x, track_y, 1, track_h, BORDER);

    let thumb_h = ((track_h as i64 * visible_items as i64) / total_items as i64)
        .max(i64::from(min_thumb_h.max(1))) as i32;
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_y = track_y
        + (((track_h - thumb_h).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    frame.rect(track_x, thumb_y, 1, thumb_h.min(track_h), MUTED);
}

fn terminal_max_scrollback(screen: &vt100::Screen) -> usize {
    let mut snapshot = screen.clone();
    snapshot.set_scrollback(usize::MAX);
    snapshot.scrollback()
}

fn render_terminal_scrollback(
    frame: &mut Frame<'_>,
    rect: Rect,
    screen: &vt100::Screen,
    min_thumb_h: i32,
) {
    let visible_rows = usize::from(screen.size().0);
    let max_scroll = terminal_max_scrollback(screen);
    if visible_rows == 0 || max_scroll == 0 {
        return;
    }

    render_vertical_scrollbar(
        frame,
        rect.x + rect.w + TERMINAL_PAD - 5,
        rect.y,
        rect.h,
        visible_rows,
        visible_rows.saturating_add(max_scroll),
        max_scroll.saturating_sub(screen.scrollback()),
        min_thumb_h,
    );
}

fn centered_text_x(rect: Rect, text: &TextRenderer, value: &str) -> i32 {
    rect.x + ((rect.w - text.measure_text(value)).max(0) / 2)
}

fn render_terminal_frame(
    frame: &mut Frame<'_>,
    text: &mut TextRenderer,
    rect: Rect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
) {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor_visible = screen.scrollback() == 0 && !screen.hide_cursor();

    for row in 0..rows {
        let y = rect.y + i32::from(row) * text.metrics.cell_height;
        if y >= rect.y + rect.h {
            break;
        }
        let row_selection = terminal_selection_span(selection, row, cols);
        for col in 0..cols {
            let x = rect.x + i32::from(col) * text.metrics.cell_width;
            if x >= rect.x + rect.w {
                break;
            }
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }

            let col_span = if col + 1 < cols
                && screen
                    .cell(row, col + 1)
                    .is_some_and(|next| next.is_wide_continuation())
            {
                2
            } else {
                1
            };

            let selected = row_selection.is_some_and(|(start, width)| {
                let end = start + width;
                let cell_end = col + col_span;
                end > col && start < cell_end
            });
            let cursor_here = cursor_visible && row == cursor_row && col == cursor_col;
            let (fg, bg) = terminal_cell_colors(cell, cursor_here, selected);
            frame.rect(
                x,
                y,
                text.metrics.cell_width * i32::from(col_span),
                text.metrics.cell_height,
                bg,
            );
            if cell.underline() {
                frame.hline(
                    x,
                    x + text.metrics.cell_width * i32::from(col_span) - 1,
                    y + text.metrics.cell_height - 2,
                    fg,
                );
            }
            let contents = if cell.contents().is_empty() {
                " "
            } else {
                cell.contents()
            };
            frame.text(text, x, y, fg, contents);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pi::PiSessionStage;

    fn snapshot(stage: PiSessionStage, queued: bool) -> PiSidecarSnapshot {
        PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            session_id: "pi-session-1".into(),
            harness_session_id: Some("local-session-1".into()),
            session_file: Some(std::path::PathBuf::from("/tmp/pi-session-1.jsonl")),
            session_name: Some("Test session".into()),
            stage,
            queued,
            tool_name: None,
            ts_ms: 1,
        }
    }

    #[test]
    fn idle_snapshot_does_not_bind_empty_draft() {
        let session = Session::new_draft();
        assert!(!actions::should_bind_sidecar_session(
            &session,
            &snapshot(PiSessionStage::Idle, false)
        ));
    }

    #[test]
    fn active_snapshot_binds_new_session() {
        let session = Session::new_draft();
        assert!(actions::should_bind_sidecar_session(
            &session,
            &snapshot(PiSessionStage::Thinking, false)
        ));
    }

    #[test]
    fn queued_snapshot_binds_new_session() {
        let session = Session::new_draft();
        assert!(actions::should_bind_sidecar_session(
            &session,
            &snapshot(PiSessionStage::Idle, true)
        ));
    }

    #[test]
    fn unmaterialized_sidecar_session_does_not_reorder() {
        assert_eq!(
            actions::sidecar_order_update(false, true, false, false),
            actions::SidecarOrderUpdate::None
        );
        assert_eq!(
            actions::sidecar_order_update(true, false, false, false),
            actions::SidecarOrderUpdate::None
        );
    }

    #[test]
    fn sidecar_session_promotes_once_materialized() {
        assert_eq!(
            actions::sidecar_order_update(false, true, false, true),
            actions::SidecarOrderUpdate::Promote
        );
        assert_eq!(
            actions::sidecar_order_update(true, true, false, true),
            actions::SidecarOrderUpdate::Promote
        );
        assert_eq!(
            actions::sidecar_order_update(true, false, false, true),
            actions::SidecarOrderUpdate::Promote
        );
        assert_eq!(
            actions::sidecar_order_update(true, false, true, true),
            actions::SidecarOrderUpdate::Touch
        );
    }

    #[test]
    fn sidebar_status_prefers_active_over_notification() {
        let mut session = Session::new_draft();
        session.runtime.running = true;
        session.runtime.unread = true;

        assert_eq!(
            sidebar::session_sidebar_status(&session),
            Some(sidebar::SidebarStatusKind::Active)
        );
    }

    #[test]
    fn notification_status_uses_full_braille_glyph() {
        assert_eq!(
            sidebar::sidebar_status_glyph(sidebar::SidebarStatusKind::Notification, 0),
            sidebar::SIDEBAR_NOTIFICATION_GLYPH
        );
    }

    #[test]
    fn project_path_normalization_preserves_order() {
        let paths = actions::normalize_unique_project_paths(vec![
            PathBuf::from("/tmp/project-b"),
            PathBuf::from("/tmp/project-a"),
            PathBuf::from("/tmp/project-b"),
        ]);

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/tmp/project-b"),
                PathBuf::from("/tmp/project-a")
            ]
        );
    }
}
