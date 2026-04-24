use std::rc::Rc;
use std::time::Duration;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{WindowAttributes, WindowId};

use super::input::{take_wheel_lines, take_zoom_steps, terminal_selection_point_for_position};
use super::sidebar::{SidebarRowKind, SIDEBAR_SPINNER_FRAME_MS};
use super::theme::UI_SCALE_STEP;
use super::App;

const WINDOW_W: f64 = 1280.0;
const WINDOW_H: f64 = 840.0;

impl App {
    fn handle_cursor_moved(&mut self) {
        if !self.terminal_selection_in_progress {
            return;
        }
        if let (Some(window), Some(text)) = (&self.window, &self.text) {
            let size = window.inner_size();
            let layout = self.compute_layout(size.width as i32, size.height as i32, text);
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

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        self.clear_pending_key_chord();
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
                let visible_rows =
                    self.sidebar_visible_rows(layout.sidebar, text, layout.spacing.panel_pad);
                let row_count = self.sidebar_rows().len();
                let lines = take_wheel_lines(&delta, cell_h, &mut self.sidebar_wheel_remainder);
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
                let lines = take_wheel_lines(&delta, cell_h, &mut self.terminal_wheel_remainder);
                if self
                    .current_terminal_mut()
                    .is_some_and(|terminal| terminal.scroll_by_lines(lines))
                {
                    self.request_redraw();
                }
            }
        }
    }

    fn handle_left_mouse_press(&mut self) {
        self.clear_pending_key_chord();
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
            let sidebar_rows = self.sidebar_rows();
            let sidebar_row_index = if layout
                .sidebar
                .contains(self.cursor_pos.0, self.cursor_pos.1)
            {
                let local_y =
                    self.cursor_pos.1 as i32 - layout.sidebar.y - layout.spacing.panel_pad;
                let visible_row = (local_y / text.metrics.cell_height).max(0) as usize;
                self.sidebar_row_index_at_visible_row(
                    &sidebar_rows,
                    self.sidebar_visible_rows(layout.sidebar, text, layout.spacing.panel_pad),
                    visible_row,
                )
            } else {
                None
            };
            Some((terminal_point, sidebar_rows, sidebar_row_index))
        } else {
            None
        };

        if let Some((terminal_point, sidebar_rows, sidebar_row_index)) = click {
            if let Some(point) = terminal_point {
                if let Some(terminal) = self.current_terminal_mut() {
                    terminal.begin_selection(point);
                    self.terminal_selection_in_progress = true;
                }
            } else {
                self.clear_current_terminal_selection();
            }

            if let Some(row_index) = sidebar_row_index {
                if let Some(row) = sidebar_rows.get(row_index) {
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

    fn handle_left_mouse_release(&mut self) {
        if self.terminal_selection_in_progress {
            self.terminal_selection_in_progress = false;
            self.copy_current_terminal_selection();
            self.request_redraw();
        }
    }

    fn handle_keyboard_input(&mut self, event: KeyEvent) {
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.process_background_events();
                self.render();
            }
            WindowEvent::Resized(_) => self.request_redraw(),
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
                let previous_hover = self.hovered_sidebar_row_index_for_cursor();
                self.cursor_pos = (position.x, position.y);
                self.handle_cursor_moved();
                if previous_hover != self.hovered_sidebar_row_index_for_cursor() {
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let previous_hover = self.hovered_sidebar_row_index_for_cursor();
                self.cursor_pos = (-1.0, -1.0);
                if previous_hover.is_some() {
                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => self.handle_mouse_wheel(delta),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.handle_left_mouse_press(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.handle_left_mouse_release(),
            WindowEvent::KeyboardInput { event, .. } => self.handle_keyboard_input(event),
            _ => {}
        }
    }
}
