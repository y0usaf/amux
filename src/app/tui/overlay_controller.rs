use crate::config::{KeyModifiers, KeyToken, NamedKeyToken};

use super::input::{mouse_event_for_bytes, MouseEventKind, WheelDirection};
use super::keyboard::key_stroke_for_bytes;
use super::overlays::archive::{archive_viewer_visible_rows_for_terminal, ArchiveViewerState};
use super::overlays::help::{
    help_overlay_lines, help_overlay_visible_rows_for_terminal, HelpOverlayState,
};
use super::overlays::usage::{
    usage_overlay_line_count, usage_overlay_visible_rows_for_terminal, UsageOverlayState,
};
use super::{TuiApp, TUI_WHEEL_LINES};

impl TuiApp {
    pub(super) fn open_archive_viewer(&mut self) {
        self.archive_viewer = Some(ArchiveViewerState::load());
    }

    pub(super) fn open_help_overlay(&mut self) {
        self.help_overlay = Some(HelpOverlayState::default());
    }

    pub(super) fn open_usage_overlay(&mut self) {
        self.usage_overlay = Some(UsageOverlayState::load());
    }

    pub(super) fn toggle_help_overlay(&mut self) {
        if self.help_overlay.is_some() {
            self.help_overlay = None;
        } else {
            self.open_help_overlay();
        }
    }

    pub(super) fn handle_help_overlay_input(&mut self, bytes: &[u8]) {
        if bytes == [0x03] {
            self.help_overlay = None;
            return;
        }

        if let Some(event) = mouse_event_for_bytes(bytes) {
            if let MouseEventKind::Wheel(direction) = event.kind {
                let delta = match direction {
                    WheelDirection::Up => -TUI_WHEEL_LINES,
                    WheelDirection::Down => TUI_WHEEL_LINES,
                };
                self.scroll_help_overlay(delta);
            }
            return;
        }

        let Some(stroke) = key_stroke_for_bytes(bytes) else {
            return;
        };
        let no_modifiers = stroke.modifiers == KeyModifiers::default();
        if no_modifiers {
            match stroke.key {
                KeyToken::Named(NamedKeyToken::Escape) => self.help_overlay = None,
                KeyToken::Named(NamedKeyToken::Up) => self.scroll_help_overlay(-1),
                KeyToken::Named(NamedKeyToken::Down) => self.scroll_help_overlay(1),
                KeyToken::Named(NamedKeyToken::PageUp) => self.page_help_overlay(-1),
                KeyToken::Named(NamedKeyToken::PageDown) => self.page_help_overlay(1),
                KeyToken::Named(NamedKeyToken::Home) => self.set_help_scroll(0),
                KeyToken::Named(NamedKeyToken::End) => self.set_help_scroll(usize::MAX),
                KeyToken::Character(ref key) if key == "q" => self.help_overlay = None,
                KeyToken::Character(ref key) if key == "j" => self.scroll_help_overlay(1),
                KeyToken::Character(ref key) if key == "k" => self.scroll_help_overlay(-1),
                KeyToken::Character(ref key) if key == "g" => self.set_help_scroll(0),
                _ => {}
            }
            return;
        }

        let ctrl_only =
            stroke.modifiers.control && !stroke.modifiers.shift && !stroke.modifiers.alt;
        if ctrl_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "c") {
            self.help_overlay = None;
            return;
        }

        let shift_only =
            stroke.modifiers.shift && !stroke.modifiers.control && !stroke.modifiers.alt;
        if shift_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "g") {
            self.set_help_scroll(usize::MAX);
        }
    }

    pub(super) fn scroll_help_overlay(&mut self, delta: i32) {
        let scroll = self
            .help_overlay
            .as_ref()
            .map(|help| help.scroll)
            .unwrap_or_default();
        let next = if delta < 0 {
            scroll.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            scroll.saturating_add(delta as usize)
        };
        self.set_help_scroll(next);
    }

    pub(super) fn page_help_overlay(&mut self, delta_pages: i32) {
        let visible_rows = help_overlay_visible_rows_for_terminal().max(1);
        let delta = delta_pages.saturating_mul(visible_rows as i32);
        self.scroll_help_overlay(delta);
    }

    pub(super) fn set_help_scroll(&mut self, scroll: usize) {
        if let Some(help) = &mut self.help_overlay {
            let lines = help_overlay_lines(&self.core.config);
            let visible_rows = help_overlay_visible_rows_for_terminal().max(1);
            help.scroll = scroll.min(lines.len().saturating_sub(visible_rows));
        }
    }

    pub(super) fn handle_usage_overlay_input(&mut self, bytes: &[u8]) {
        if bytes == [0x03] {
            self.usage_overlay = None;
            return;
        }

        if let Some(event) = mouse_event_for_bytes(bytes) {
            if let MouseEventKind::Wheel(direction) = event.kind {
                let delta = match direction {
                    WheelDirection::Up => -TUI_WHEEL_LINES,
                    WheelDirection::Down => TUI_WHEEL_LINES,
                };
                self.scroll_usage_overlay(delta);
            }
            return;
        }

        let Some(stroke) = key_stroke_for_bytes(bytes) else {
            return;
        };
        let no_modifiers = stroke.modifiers == KeyModifiers::default();
        if no_modifiers {
            match stroke.key {
                KeyToken::Named(NamedKeyToken::Escape) => self.usage_overlay = None,
                KeyToken::Named(NamedKeyToken::Up) => self.scroll_usage_overlay(-1),
                KeyToken::Named(NamedKeyToken::Down) => self.scroll_usage_overlay(1),
                KeyToken::Named(NamedKeyToken::PageUp) => self.page_usage_overlay(-1),
                KeyToken::Named(NamedKeyToken::PageDown) => self.page_usage_overlay(1),
                KeyToken::Named(NamedKeyToken::Home) => self.set_usage_scroll(0),
                KeyToken::Named(NamedKeyToken::End) => self.set_usage_scroll(usize::MAX),
                KeyToken::Character(ref key) if key == "q" => self.usage_overlay = None,
                KeyToken::Character(ref key) if key == "j" => self.scroll_usage_overlay(1),
                KeyToken::Character(ref key) if key == "k" => self.scroll_usage_overlay(-1),
                KeyToken::Character(ref key) if key == "g" => self.set_usage_scroll(0),
                KeyToken::Character(ref key) if key == "r" => self.reload_usage_overlay(),
                _ => {}
            }
            return;
        }

        let ctrl_only =
            stroke.modifiers.control && !stroke.modifiers.shift && !stroke.modifiers.alt;
        if ctrl_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "c") {
            self.usage_overlay = None;
            return;
        }

        let shift_only =
            stroke.modifiers.shift && !stroke.modifiers.control && !stroke.modifiers.alt;
        if shift_only && matches!(stroke.key, KeyToken::Character(ref key) if key == "g") {
            self.set_usage_scroll(usize::MAX);
        }
    }

    pub(super) fn scroll_usage_overlay(&mut self, delta: i32) {
        let scroll = self
            .usage_overlay
            .as_ref()
            .map(|usage| usage.scroll)
            .unwrap_or_default();
        let next = if delta < 0 {
            scroll.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            scroll.saturating_add(delta as usize)
        };
        self.set_usage_scroll(next);
    }

    pub(super) fn page_usage_overlay(&mut self, delta_pages: i32) {
        let visible_rows = usage_overlay_visible_rows_for_terminal().max(1);
        let delta = delta_pages.saturating_mul(visible_rows as i32);
        self.scroll_usage_overlay(delta);
    }

    pub(super) fn set_usage_scroll(&mut self, scroll: usize) {
        if let Some(usage) = &mut self.usage_overlay {
            let visible_rows = usage_overlay_visible_rows_for_terminal().max(1);
            usage.scroll =
                scroll.min(usage_overlay_line_count(&usage.report).saturating_sub(visible_rows));
        }
    }

    pub(super) fn reload_usage_overlay(&mut self) {
        if let Some(usage) = &mut self.usage_overlay {
            usage.reload();
        }
    }

    pub(super) fn handle_archive_viewer_input(&mut self, bytes: &[u8]) {
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

    pub(super) fn move_archive_selection(&mut self, delta: i32) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.move_selection(delta);
        }
    }

    pub(super) fn page_archive_selection(&mut self, delta_pages: i32) {
        let visible_rows = archive_viewer_visible_rows_for_terminal();
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.page_selection(delta_pages, visible_rows);
        }
    }

    pub(super) fn select_first_archive_session(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.select_first();
        }
    }

    pub(super) fn select_last_archive_session(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.select_last();
        }
    }

    pub(super) fn reload_archive_viewer(&mut self) {
        if let Some(viewer) = &mut self.archive_viewer {
            viewer.reload_sessions();
            viewer.note = Some(format!("{} archived sessions", viewer.sessions.len()));
        }
    }

    pub(super) fn restore_selected_archive_session(&mut self) {
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
}
