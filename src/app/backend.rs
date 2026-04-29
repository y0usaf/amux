use std::path::PathBuf;

use arboard::Clipboard;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
use arboard::{LinuxClipboardKind, SetExtLinux};

use crate::config::{
    AppAction, AppConfig, KeyChordState, KeyStroke, KeyToken, Keymap, KeymapMatch, NamedKeyToken,
};
use crate::notify::Notify;
use crate::pi::{self, PiSidecarSnapshot};
use crate::sidecar::SidecarListener;
use crate::state::{PersistedState, Project, Session};
use crate::terminal::{
    TerminalController, TerminalSelectionPoint, TerminalSelectionRange, TerminalStatus,
};
use crate::util::now_millis;

use super::clipboard_image::{clipboard_image_path, clipboard_image_path_from_arboard};
use super::layout::CellRect;
use super::sidebar::{
    build_sidebar_rows, clamp_sidebar_scroll_value, ensure_sidebar_selection_visible_for_state,
    scroll_sidebar_by_rows_value, selected_sidebar_selection_span_for_state, sidebar_has_spinner,
    sidebar_viewport_items, sticky_sidebar_anchor_row, SidebarRow, SidebarRowKind,
    SidebarSelectionSpan, SidebarViewportItem,
};
use super::sidecar_reducer::{apply_snapshot_to_session, reconcile_terminal_note};
use super::status::status_text_for_session;
use super::terminal_manager::TerminalManager;
use super::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ShortcutOutcome {
    NoMatch,
    Pending,
    Triggered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChromeView {
    pub(super) project: String,
    pub(super) status: String,
    pub(super) session: String,
}

pub(super) struct HarnessCore {
    pub(super) config: AppConfig,
    keymap: Keymap,
    key_chord_state: KeyChordState,
    sidecar: SidecarListener,
    terminal_manager: TerminalManager,
    workspace: Workspace,
    sidebar_scroll: usize,
    sidebar_sync_to_selection: bool,
    terminal_selection_in_progress: bool,
    clipboard: Option<Clipboard>,
    note: Option<String>,
}

impl HarnessCore {
    pub(super) fn new(notify: Notify, initial_project_paths: Vec<PathBuf>) -> anyhow::Result<Self> {
        let sidecar_socket_path = pi::socket_path();
        let sidecar = SidecarListener::start(notify.clone(), sidecar_socket_path.clone())?;
        let terminal_manager =
            TerminalManager::new(notify, pi::extension_path(), sidecar_socket_path.clone());
        let persisted = PersistedState::load_default().unwrap_or_default();
        let config = AppConfig::load_default().unwrap_or_default();
        let keymap = config.keymap();
        let workspace = Workspace::new(initial_project_paths, persisted);

        let mut core = Self {
            config,
            keymap,
            key_chord_state: KeyChordState::default(),
            sidecar,
            terminal_manager,
            workspace,
            sidebar_scroll: 0,
            sidebar_sync_to_selection: true,
            terminal_selection_in_progress: false,
            clipboard: None,
            note: None,
        };
        core.workspace.reload_projects_from_disk();
        if !core.terminal_manager.has_sidecar_extension() {
            core.note = Some("sidecar extension not found".to_string());
        }
        Ok(core)
    }
}

pub(super) struct FrameModel {
    pub(super) chrome: ChromeView,
    pub(super) sidebar_rows: Vec<SidebarRow>,
    pub(super) sidebar_viewport: Vec<SidebarViewportItem>,
    pub(super) terminal_screen: vt100::Screen,
    pub(super) terminal_selection: Option<TerminalSelectionRange>,
}

#[cfg(test)]
pub(super) fn advance_shortcut_match(
    keymap: &Keymap,
    state: &mut KeyChordState,
    stroke: Option<KeyStroke>,
    clear_on_unhandled_press: bool,
) -> Option<KeymapMatch> {
    match stroke {
        Some(stroke) => Some(keymap.advance(state, stroke)),
        None if clear_on_unhandled_press => {
            state.clear();
            None
        }
        None => None,
    }
}

pub(super) fn terminal_selection_point_for_cell_rect(
    rect: CellRect,
    rows: u16,
    cols: u16,
    col: i32,
    row: i32,
) -> Option<TerminalSelectionPoint> {
    if rows == 0 || cols == 0 || !rect.contains_cell(col, row) {
        return None;
    }
    Some(TerminalSelectionPoint {
        row: (row - rect.row).clamp(0, i32::from(rows.saturating_sub(1))) as u16,
        col: (col - rect.col).clamp(0, i32::from(cols)) as u16,
    })
}

impl HarnessCore {
    pub(super) fn set_terminal_selection_in_progress(&mut self, in_progress: bool) {
        self.terminal_selection_in_progress = in_progress;
    }

    pub(super) fn set_note_text(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    pub(super) fn clear_pending_key_chord(&mut self) {
        self.key_chord_state.clear();
    }

    pub(super) fn advance_keymap(&mut self, stroke: KeyStroke) -> KeymapMatch {
        let keymap = self.keymap.clone();
        keymap.advance(&mut self.key_chord_state, stroke)
    }

    pub(super) fn handle_shortcut_stroke(&mut self, stroke: KeyStroke) -> ShortcutOutcome {
        match self.advance_keymap(stroke) {
            KeymapMatch::NoMatch => ShortcutOutcome::NoMatch,
            KeymapMatch::Pending => ShortcutOutcome::Pending,
            KeymapMatch::Triggered(action) => {
                self.run_action(action);
                ShortcutOutcome::Triggered
            }
        }
    }

    pub(super) fn handle_hjkl_navigation_shortcut(&mut self, stroke: &KeyStroke) -> bool {
        let ctrl_only =
            stroke.modifiers.control && !stroke.modifiers.shift && !stroke.modifiers.alt;
        if !ctrl_only {
            return false;
        }

        let action = match stroke.key {
            KeyToken::Character(ref key) if key == "h" => Some(AppAction::PreviousProject),
            KeyToken::Character(ref key) if key == "l" => Some(AppAction::NextProject),
            KeyToken::Character(ref key) if key == "k" => Some(AppAction::PreviousSession),
            KeyToken::Character(ref key) if key == "j" => Some(AppAction::NextSession),
            _ => None,
        };
        let Some(action) = action else {
            return false;
        };
        self.run_action(action);
        true
    }

    pub(super) fn current_project(&self) -> Option<&Project> {
        self.workspace.current_project()
    }

    pub(super) fn current_session(&self) -> Option<&Session> {
        self.workspace.current_session()
    }

    pub(super) fn current_session_visible_in_sidebar(&self) -> bool {
        self.workspace.current_session_visible_in_sidebar()
    }

    pub(super) fn persist_selection(&mut self) {
        self.workspace.persist_selection();
    }

    pub(super) fn chrome_view(&self) -> ChromeView {
        ChromeView {
            project: self
                .current_project()
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "pi-harness".to_string()),
            status: self
                .note
                .as_deref()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    status_text_for_session(
                        self.current_project().is_some(),
                        self.current_session(),
                        self.current_terminal_status(),
                    )
                }),
            session: self
                .current_session()
                .map(|session| session.name.clone())
                .unwrap_or_default(),
        }
    }

    pub(super) fn current_terminal(&self) -> Option<&TerminalController> {
        self.terminal_manager.current(
            self.current_session()
                .map(|session| session.local_id.as_str()),
        )
    }

    pub(super) fn current_terminal_mut(&mut self) -> Option<&mut TerminalController> {
        let session_id = self.current_session()?.local_id.clone();
        self.terminal_manager.current_mut(Some(&session_id))
    }

    pub(super) fn current_terminal_status(&self) -> Option<&TerminalStatus> {
        self.terminal_manager.status(
            self.current_session()
                .map(|session| session.local_id.as_str()),
        )
    }

    pub(super) fn clear_current_terminal_selection(&mut self) -> bool {
        self.current_terminal_mut()
            .is_some_and(TerminalController::clear_selection)
    }

    pub(super) fn begin_terminal_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        let changed = self
            .current_terminal_mut()
            .is_some_and(|terminal| terminal.begin_selection(point));
        self.set_terminal_selection_in_progress(true);
        changed
    }

    pub(super) fn update_terminal_selection(&mut self, point: TerminalSelectionPoint) -> bool {
        if !self.terminal_selection_in_progress {
            return false;
        }
        self.current_terminal_mut()
            .is_some_and(|terminal| terminal.update_selection(point))
    }

    pub(super) fn clear_or_begin_terminal_selection(
        &mut self,
        point: Option<TerminalSelectionPoint>,
    ) -> bool {
        self.set_terminal_selection_in_progress(false);
        match point {
            Some(point) => self.begin_terminal_selection(point),
            None => self.clear_current_terminal_selection(),
        }
    }

    pub(super) fn finish_terminal_selection(&mut self) -> bool {
        if !self.terminal_selection_in_progress {
            return false;
        }
        self.set_terminal_selection_in_progress(false);
        self.copy_current_terminal_selection()
    }

    pub(super) fn scroll_terminal_by_lines(&mut self, lines: i32) -> bool {
        self.current_terminal_mut()
            .is_some_and(|terminal| terminal.scroll_by_lines(lines))
    }

    pub(super) fn handle_terminal_scroll_key(&mut self, stroke: &KeyStroke) -> bool {
        let shift_only =
            stroke.modifiers.shift && !stroke.modifiers.control && !stroke.modifiers.alt;
        if !shift_only {
            return false;
        }

        let Some(terminal) = self.current_terminal_mut() else {
            return false;
        };
        match stroke.key {
            KeyToken::Named(NamedKeyToken::PageUp) => {
                let page = i32::from(terminal.screen().size().0.saturating_sub(2).max(1));
                terminal.scroll_by_lines(page)
            }
            KeyToken::Named(NamedKeyToken::PageDown) => {
                let page = i32::from(terminal.screen().size().0.saturating_sub(2).max(1));
                terminal.scroll_by_lines(-page)
            }
            KeyToken::Named(NamedKeyToken::Home) => terminal.scroll_by_lines(i32::MAX),
            KeyToken::Named(NamedKeyToken::End) => terminal.scroll_to_bottom(),
            _ => false,
        }
    }

    pub(super) fn send_bytes_to_current_terminal(&mut self, bytes: &[u8], context: &str) -> bool {
        let Some(terminal) = self.current_terminal_mut() else {
            return false;
        };
        match terminal.send_bytes(bytes) {
            Ok(changed) => changed,
            Err(error) => {
                self.set_note_text(format!("{context}: {error}"));
                false
            }
        }
    }

    pub(super) fn paste_bytes_to_current_terminal(&mut self, bytes: &[u8]) -> bool {
        let Some(terminal) = self.current_terminal_mut() else {
            return false;
        };
        match terminal.paste_bytes(bytes) {
            Ok(pasted) => pasted,
            Err(error) => {
                self.set_note_text(format!("terminal paste: {error}"));
                false
            }
        }
    }

    pub(super) fn resize_terminals(&mut self, rows: u16, cols: u16) {
        self.terminal_manager.resize_all(rows.max(1), cols.max(1));
    }

    pub(super) fn remove_terminal_for_session_id(&mut self, session_id: &str) {
        self.terminal_manager.remove(session_id);
    }

    pub(super) fn restart_terminal_for_session(
        &mut self,
        project_index: usize,
        session_index: usize,
    ) {
        let Some(project) = self.workspace.projects().get(project_index).cloned() else {
            return;
        };
        let Some(session) = project.sessions.get(session_index).cloned() else {
            return;
        };

        if let Some(error) = self
            .terminal_manager
            .restart_terminal_for_session(&project, &session)
        {
            self.set_note_text(error);
        }
    }

    pub(super) fn restart_idle_terminals_for_project(&mut self, project_index: usize) {
        let Some(project) = self.workspace.projects().get(project_index).cloned() else {
            return;
        };

        for error in self
            .terminal_manager
            .restart_idle_terminals_for_project(&project)
        {
            self.set_note_text(error);
        }
    }

    pub(super) fn sync_terminals(&mut self) {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        let projects = self.workspace.projects().to_vec();
        let errors = self
            .terminal_manager
            .sync(&projects, selected_session_id.as_deref());
        for error in errors {
            self.set_note_text(error);
        }
    }

    pub(super) fn drain_terminal_events(&mut self) -> bool {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        self.terminal_manager
            .drain_events(selected_session_id.as_deref())
    }

    pub(super) fn selection_changed(&mut self) {
        self.sync_sidebar_to_selection();
    }

    pub(super) fn selection_changed_with_terminal_sync(&mut self) {
        self.sync_sidebar_to_selection();
        self.sync_terminals();
    }

    pub(super) fn reload_projects_from_disk(&mut self) {
        self.workspace.reload_projects_from_disk();
        self.selection_changed();
    }

    pub(super) fn restore_selection(
        &mut self,
        project_key: Option<String>,
        session_key: Option<String>,
    ) {
        self.workspace.restore_selection(project_key, session_key);
        self.selection_changed();
    }

    pub(super) fn promote_project_to_front(&mut self, project_index: usize) {
        self.workspace.promote_project_to_front(project_index);
        self.selection_changed();
    }

    pub(super) fn remove_selected_project(&mut self) {
        if self.workspace.remove_selected_project() {
            self.selection_changed_with_terminal_sync();
        }
    }

    pub(super) fn open_project_path(&mut self, path: PathBuf) {
        match self.workspace.open_project_path(path) {
            Ok(result) => {
                self.selection_changed_with_terminal_sync();
                let action = if result.added { "opened" } else { "selected" };
                self.set_note_text(format!("{action} {}", result.path.display()));
            }
            Err(note) => self.set_note_text(note),
        }
    }

    pub(super) fn refresh_project_from_scan(&mut self, project_index: usize) {
        self.workspace.refresh_project_from_scan(project_index);
        self.selection_changed();
    }

    pub(super) fn archive_selected_session(&mut self) {
        let target = match self.workspace.archive_target() {
            Ok(target) => target,
            Err(note) => {
                self.set_note_text(note.to_string());
                return;
            }
        };

        if let Some(path) = target.session_file.as_ref() {
            if let Err(error) = pi::archive_session_file(path) {
                self.set_note_text(error);
                return;
            }
        }

        self.workspace.remove_archived_session(&target);
        self.remove_terminal_for_session_id(&target.session_id);
        self.selection_changed_with_terminal_sync();
    }

    pub(super) fn new_session(&mut self) {
        self.workspace.new_session();
        self.selection_changed_with_terminal_sync();
    }

    pub(super) fn select_project(&mut self, index: usize) {
        if self.workspace.select_project(index) {
            self.selection_changed_with_terminal_sync();
        }
    }

    pub(super) fn select_session_in_project(&mut self, project_index: usize, session_index: usize) {
        if self
            .workspace
            .select_session_in_project(project_index, session_index)
        {
            self.selection_changed_with_terminal_sync();
        }
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) {
        if self.workspace.cycle_projects(delta) {
            self.selection_changed_with_terminal_sync();
        }
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) {
        if self.workspace.cycle_sessions(delta) {
            self.selection_changed_with_terminal_sync();
        }
    }

    pub(super) fn refresh_current_session(&mut self) {
        let project_index = match self.workspace.refresh_current_session_project_index() {
            Ok(project_index) => project_index,
            Err(note) => {
                self.set_note_text(note.to_string());
                return;
            }
        };

        self.refresh_project_from_scan(project_index);
        self.sync_terminals();

        if let Some((project_index, session_index)) =
            self.workspace.selected_terminal_restart_target()
        {
            self.restart_terminal_for_session(project_index, session_index);
        }
    }

    pub(super) fn refresh_all_sessions(&mut self) {
        self.reload_projects_from_disk();
        self.sync_terminals();
        let project_count = self.workspace.projects().len();
        for project_index in 0..project_count {
            self.restart_idle_terminals_for_project(project_index);
        }
    }

    pub(super) fn apply_sidecar_snapshot(&mut self, snapshot: PiSidecarSnapshot) -> bool {
        if !snapshot.is_valid() {
            return false;
        }

        let mut matched = None;
        for (project_index, project) in self.workspace.projects().iter().enumerate() {
            for (session_index, session) in project.sessions.iter().enumerate() {
                if session.matches_identity(
                    snapshot.harness_session_id.as_deref(),
                    &snapshot.session_id,
                    snapshot.session_file.as_deref(),
                ) {
                    matched = Some((project_index, session_index));
                    break;
                }
            }
            if matched.is_some() {
                break;
            }
        }

        let Some((project_index, session_index)) = matched else {
            return false;
        };
        let selected = self.workspace.selected_project_index() == project_index
            && self.workspace.selected_session_index() == Some(session_index);
        let selected_session_key = self
            .current_session()
            .map(|session| session.local_id.clone());
        let update = {
            let session = &mut self.workspace.projects_mut()[project_index].sessions[session_index];
            apply_snapshot_to_session(session, &snapshot, selected, now_millis())
        };

        if update.reordered {
            self.workspace.projects_mut()[project_index].sort_sessions();
            self.restore_selection(None, selected_session_key);
        }
        if update.promote_project {
            self.promote_project_to_front(project_index);
        }
        self.persist_selection();
        true
    }

    pub(super) fn process_background_events(&mut self) -> bool {
        let mut changed = self.drain_terminal_events();
        while let Some(snapshot) = { self.sidecar.try_recv() } {
            changed |= self.apply_sidecar_snapshot(snapshot);
        }

        let note = reconcile_terminal_note(self.note.as_deref(), self.current_terminal_status());
        self.note = note;
        changed
    }

    pub(super) fn run_action(&mut self, action: AppAction) {
        match action {
            AppAction::PreviousProject => self.cycle_projects(-1),
            AppAction::NextProject => self.cycle_projects(1),
            AppAction::PreviousSession => self.cycle_sessions(-1),
            AppAction::NextSession => self.cycle_sessions(1),
            AppAction::NewSession => self.new_session(),
            AppAction::RefreshSession => self.refresh_current_session(),
            AppAction::RefreshAllSessions => self.refresh_all_sessions(),
            AppAction::ArchiveSession => self.archive_selected_session(),
            AppAction::RemoveProject => self.remove_selected_project(),
            AppAction::CopySelection => {
                let _ = self.copy_current_terminal_selection();
            }
            AppAction::PasteClipboard => {
                let _ = self.paste_clipboard();
            }
        }
    }

    pub(super) fn activate_sidebar_row(&mut self, row_kind: &SidebarRowKind) {
        match *row_kind {
            SidebarRowKind::Project(index) => self.select_project(index),
            SidebarRowKind::Session {
                project_index,
                session_index,
            } => self.select_session_in_project(project_index, session_index),
            SidebarRowKind::Label => {}
        }
    }

    pub(super) fn has_sidebar_spinner(&self) -> bool {
        sidebar_has_spinner(self.workspace.projects())
    }

    pub(super) fn sidebar_rows(&self) -> Vec<SidebarRow> {
        build_sidebar_rows(
            self.workspace.projects(),
            self.workspace.selected_project_index(),
            self.workspace.selected_session_index(),
            self.current_session_visible_in_sidebar(),
        )
    }

    pub(super) fn selected_sidebar_selection_span(
        &self,
        rows: &[SidebarRow],
    ) -> Option<SidebarSelectionSpan> {
        selected_sidebar_selection_span_for_state(
            self.workspace.projects().is_empty(),
            self.workspace.selected_project_index(),
            self.workspace.selected_session_index(),
            self.current_session_visible_in_sidebar(),
            rows,
        )
    }

    pub(super) fn sticky_sidebar_anchor_row_index(
        &self,
        rows: &[SidebarRow],
        visible_rows: usize,
    ) -> Option<usize> {
        self.selected_sidebar_selection_span(rows)
            .and_then(|span| sticky_sidebar_anchor_row(self.sidebar_scroll, visible_rows, span))
    }

    pub(super) fn sidebar_row_index_at_visible_row(
        &self,
        rows: &[SidebarRow],
        visible_rows: usize,
        visible_row: usize,
    ) -> Option<usize> {
        if visible_row >= visible_rows {
            return None;
        }

        let sticky_row = self.sticky_sidebar_anchor_row_index(rows, visible_rows);
        let row_index = match sticky_row {
            Some(anchor_row) if visible_row == 0 => anchor_row,
            Some(_) => self.sidebar_scroll + visible_row.saturating_sub(1),
            None => self.sidebar_scroll + visible_row,
        };
        rows.get(row_index).map(|_| row_index)
    }

    pub(super) fn clamp_sidebar_scroll(&mut self, row_count: usize, visible_rows: usize) {
        let scroll = clamp_sidebar_scroll_value(self.sidebar_scroll, row_count, visible_rows);
        self.sidebar_scroll = scroll;
    }

    pub(super) fn ensure_sidebar_selection_visible(
        &mut self,
        rows: &[SidebarRow],
        visible_rows: usize,
    ) {
        let scroll = ensure_sidebar_selection_visible_for_state(
            self.sidebar_scroll,
            self.workspace.projects().is_empty(),
            self.workspace.selected_project_index(),
            self.workspace.selected_session_index(),
            self.current_session_visible_in_sidebar(),
            rows,
            visible_rows,
        );
        self.sidebar_scroll = scroll;
    }

    pub(super) fn scroll_sidebar_by_rows(
        &mut self,
        delta_rows: i32,
        visible_rows: usize,
        row_count: usize,
    ) -> bool {
        let (next, changed) =
            scroll_sidebar_by_rows_value(self.sidebar_scroll, delta_rows, visible_rows, row_count);
        self.sidebar_scroll = next;
        self.sidebar_sync_to_selection = false;
        changed
    }

    pub(super) fn scroll_sidebar_from_wheel(
        &mut self,
        wheel_lines: i32,
        visible_rows: usize,
        row_count: usize,
    ) -> bool {
        self.scroll_sidebar_by_rows(-wheel_lines, visible_rows, row_count)
    }

    pub(super) fn sync_sidebar_to_selection(&mut self) {
        self.sidebar_sync_to_selection = true;
    }

    pub(super) fn prepare_frame(
        &mut self,
        terminal_rows: u16,
        terminal_cols: u16,
        visible_sidebar_rows: usize,
    ) -> FrameModel {
        self.sync_terminals();
        self.resize_terminals(terminal_rows, terminal_cols);

        let sidebar_rows = self.sidebar_rows();
        if self.sidebar_sync_to_selection {
            self.ensure_sidebar_selection_visible(&sidebar_rows, visible_sidebar_rows);
            self.sidebar_sync_to_selection = false;
        } else {
            self.clamp_sidebar_scroll(sidebar_rows.len(), visible_sidebar_rows);
        }
        let sticky_sidebar_anchor =
            self.sticky_sidebar_anchor_row_index(&sidebar_rows, visible_sidebar_rows);
        let sidebar_viewport = sidebar_viewport_items(
            &sidebar_rows,
            self.sidebar_scroll,
            visible_sidebar_rows,
            sticky_sidebar_anchor,
        );
        let terminal_selection = self
            .current_terminal()
            .and_then(TerminalController::selection_range);
        let terminal_screen = self
            .current_terminal()
            .map(|terminal| terminal.screen().clone())
            .unwrap_or_else(|| {
                vt100::Parser::new(terminal_rows.max(1), terminal_cols.max(1), 0)
                    .screen()
                    .clone()
            });

        FrameModel {
            chrome: self.chrome_view(),
            sidebar_rows,
            sidebar_viewport,
            terminal_screen,
            terminal_selection,
        }
    }

    pub(super) fn clipboard_mut(&mut self) -> Option<&mut Clipboard> {
        let slot = &mut self.clipboard;
        if slot.is_none() {
            *slot = Some(Clipboard::new().ok()?);
        }
        slot.as_mut()
    }

    pub(super) fn copy_text_to_clipboard(&mut self, text: String) -> bool {
        let Some(clipboard) = self.clipboard_mut() else {
            self.set_note_text("clipboard unavailable");
            return false;
        };

        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        ))]
        {
            if clipboard
                .set()
                .clipboard(LinuxClipboardKind::Clipboard)
                .text(text.as_str())
                .is_err()
            {
                self.set_note_text("clipboard unavailable");
                return false;
            }
            let _ = clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text.as_str());
            true
        }

        #[cfg(not(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )))]
        {
            if clipboard.set_text(text).is_ok() {
                true
            } else {
                self.set_note_text("clipboard unavailable");
                false
            }
        }
    }

    pub(super) fn copy_current_terminal_selection(&mut self) -> bool {
        let Some(text) = self
            .current_terminal()
            .and_then(TerminalController::selection_text)
        else {
            return false;
        };
        self.copy_text_to_clipboard(text)
    }

    pub(super) fn paste_clipboard(&mut self) -> bool {
        let mut arboard_image_error = None;
        let text = if let Some(clipboard) = self.clipboard_mut() {
            match clipboard_image_path_from_arboard(clipboard) {
                Ok(Some(path)) => return self.paste_image_path_to_current_terminal(path),
                Ok(None) => {}
                Err(error) => arboard_image_error = Some(error),
            }
            clipboard.get_text().ok().filter(|text| !text.is_empty())
        } else {
            None
        };
        if let Some(text) = text {
            return self.paste_text_to_current_terminal(&text);
        }

        if let Some(error) = arboard_image_error.as_ref() {
            self.set_note_text(format!("clipboard image: {error}"));
        }

        match clipboard_image_path() {
            Ok(Some(path)) => self.paste_image_path_to_current_terminal(path),
            Ok(None) => {
                if arboard_image_error.is_none() {
                    self.set_note_text("clipboard empty");
                }
                false
            }
            Err(error) => {
                self.set_note_text(format!("clipboard image: {error}"));
                false
            }
        }
    }

    fn paste_image_path_to_current_terminal(&mut self, path: PathBuf) -> bool {
        let text = path.display().to_string();
        let pasted = self.paste_text_to_current_terminal(&text);
        if pasted {
            self.set_note_text(format!("pasted image path: {}", path.display()));
        }
        pasted
    }

    fn paste_text_to_current_terminal(&mut self, text: &str) -> bool {
        let Some(terminal) = self.current_terminal_mut() else {
            return false;
        };
        match terminal.paste_text(text) {
            Ok(pasted) => pasted,
            Err(error) => {
                self.set_note_text(format!("terminal paste: {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn stroke(text: &str) -> KeyStroke {
        KeyStroke::parse(text).unwrap()
    }

    #[test]
    pub(super) fn unmapped_pressed_event_clears_pending_chord() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "new_session".into(),
            crate::config::ConfigKeybind::Single("ctrl+p n".into()),
        );

        let keymap = config.keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, Some(stroke("ctrl+p")), true),
            Some(KeymapMatch::Pending)
        );
        assert_eq!(state.pending(), &[stroke("ctrl+p")]);

        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, None, true),
            None
        );
        assert!(state.pending().is_empty());
        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, Some(stroke("n")), true),
            Some(KeymapMatch::NoMatch)
        );
    }

    #[test]
    pub(super) fn repeat_or_release_does_not_clear_pending_chord() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "new_session".into(),
            crate::config::ConfigKeybind::Single("ctrl+p n".into()),
        );

        let keymap = config.keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, Some(stroke("ctrl+p")), true),
            Some(KeymapMatch::Pending)
        );

        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, None, false),
            None
        );
        assert_eq!(state.pending(), &[stroke("ctrl+p")]);
    }

    #[test]
    pub(super) fn shortcut_match_triggers_actions_for_mapped_strokes() {
        let keymap = AppConfig::default().keymap();
        let mut state = KeyChordState::default();

        assert_eq!(
            advance_shortcut_match(&keymap, &mut state, Some(stroke("ctrl+left")), true),
            Some(KeymapMatch::Triggered(AppAction::PreviousProject))
        );
    }

    #[test]
    pub(super) fn terminal_selection_point_for_cell_rect_checks_bounds() {
        let rect = CellRect::new(2, 3, 4, 2);

        assert_eq!(
            terminal_selection_point_for_cell_rect(rect, 2, 4, 2, 3),
            Some(TerminalSelectionPoint { row: 0, col: 0 })
        );
        assert_eq!(
            terminal_selection_point_for_cell_rect(rect, 2, 4, 5, 4),
            Some(TerminalSelectionPoint { row: 1, col: 3 })
        );
        assert_eq!(
            terminal_selection_point_for_cell_rect(rect, 0, 4, 2, 3),
            None
        );
        assert_eq!(
            terminal_selection_point_for_cell_rect(rect, 2, 0, 2, 3),
            None
        );
        assert_eq!(
            terminal_selection_point_for_cell_rect(rect, 2, 4, 6, 4),
            None
        );
    }
}
