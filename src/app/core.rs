use std::collections::HashMap;
use std::path::PathBuf;

use winit::event_loop::EventLoopProxy;
use winit::keyboard::ModifiersState;

use crate::config::KeyChordState;
use crate::pi;
use crate::render::TextRenderer;
use crate::sidecar::SidecarListener;

use super::theme::{self, FONT_SIZE, UI_SCALE_DEFAULT, UI_SCALE_STEP};
use super::App;

impl App {
    pub fn new(
        proxy: EventLoopProxy<()>,
        initial_project_paths: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        let sidecar_socket_path = pi::socket_path();
        let sidecar = SidecarListener::start(proxy.clone(), sidecar_socket_path.clone())?;
        let persisted = super::PersistedState::load_default().unwrap_or_default();
        let mut config = super::AppConfig::load_default().unwrap_or_default();
        let loaded_ui_scale = config.ui_scale.or(persisted.ui_scale);
        let ui_scale = theme::clamp_ui_scale(loaded_ui_scale.unwrap_or(UI_SCALE_DEFAULT));
        let save_config = loaded_ui_scale.is_some() && config.ui_scale != Some(ui_scale);
        config.ui_scale = Some(ui_scale);
        if save_config {
            let _ = config.save_default();
        }
        let keymap = config.keymap();

        let mut app = Self {
            proxy,
            initial_project_paths,
            config,
            keymap,
            key_chord_state: KeyChordState::default(),
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

    pub(super) fn set_note(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    pub(super) fn font_size_for_scale_factor(&self, scale_factor: f64, ui_scale: f32) -> f32 {
        FONT_SIZE * scale_factor as f32 * ui_scale
    }

    pub(super) fn load_text_renderer(&self, font_size: f32) -> anyhow::Result<TextRenderer> {
        TextRenderer::with_font_family(self.config.font_family(), font_size)
            .or_else(|_| TextRenderer::load(font_size))
    }

    pub(super) fn set_ui_scale(&mut self, ui_scale: f32) -> bool {
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

    pub(super) fn zoom_in(&mut self) -> bool {
        self.set_ui_scale(self.ui_scale + UI_SCALE_STEP)
    }

    pub(super) fn zoom_out(&mut self) -> bool {
        self.set_ui_scale(self.ui_scale - UI_SCALE_STEP)
    }

    pub(super) fn zoom_reset(&mut self) -> bool {
        self.set_ui_scale(UI_SCALE_DEFAULT)
    }

    pub(super) fn request_redraw(&mut self) {
        self.needs_redraw = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn update_window_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let title = match (self.current_project(), self.current_session()) {
            (Some(project), Some(session)) => {
                format!("pi-harness — {} — {}", project.name, session.name)
            }
            (Some(project), None) => format!("pi-harness — {}", project.name),
            _ => "pi-harness".to_string(),
        };
        window.set_title(&title);
    }
}
