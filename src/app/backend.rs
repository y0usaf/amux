use std::collections::HashMap;
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
use crate::state::{PersistedState, Project, ScannedSession, Session};
use crate::terminal::{
    TerminalController, TerminalSelectionPoint, TerminalSelectionRange, TerminalStatus,
};
use crate::util::{normalize_project_path, now_millis};

use super::clipboard_image::{clipboard_image_path, clipboard_image_path_from_arboard};
use super::layout::CellRect;
use super::rail_bridge;
use super::sidebar::{
    build_sidebar_rows, clamp_sidebar_scroll_value, ensure_sidebar_selection_visible_for_state,
    scroll_sidebar_by_rows_value, selected_sidebar_selection_span_for_state,
    sidebar_viewport_items, sticky_sidebar_anchor_row, SidebarRow, SidebarRowKind,
    SidebarSelectionSpan, SidebarStatusKind, SidebarViewportItem,
};
use super::sidecar_reducer::{apply_snapshot_to_session, reconcile_terminal_note};
use super::status::status_text_for_session;
use super::terminal_manager::TerminalManager;
use super::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShortcutOutcome {
    NoMatch,
    Pending,
    Triggered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatusNoteKind {
    Ok,
    Error,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChromeView {
    pub(super) project: String,
    pub(super) status: String,
    pub(super) status_kind: Option<StatusNoteKind>,
    pub(super) session: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StatusNote {
    text: String,
    kind: StatusNoteKind,
    expires_at_ms: u64,
}

#[derive(Default)]
struct SessionLocator {
    by_harness_session_id: HashMap<String, (usize, usize)>,
    by_pi_session_id: HashMap<String, (usize, usize)>,
    by_session_file: HashMap<PathBuf, (usize, usize)>,
}

impl SessionLocator {
    fn rebuild(projects: &[Project]) -> Self {
        let mut locator = Self::default();
        for (project_index, project) in projects.iter().enumerate() {
            for (session_index, session) in project.sessions.iter().enumerate() {
                let location = (project_index, session_index);
                locator
                    .by_harness_session_id
                    .entry(session.local_id.clone())
                    .or_insert(location);
                if let Some(pi_session_id) = session.pi_session_id.as_ref() {
                    locator
                        .by_pi_session_id
                        .entry(pi_session_id.clone())
                        .or_insert(location);
                }
                if let Some(session_file) = session.session_file.as_ref() {
                    locator
                        .by_session_file
                        .entry(session_file.clone())
                        .or_insert(location);
                }
            }
        }
        locator
    }

    fn locate(&self, snapshot: &PiSidecarSnapshot) -> Option<(usize, usize)> {
        let mut matched = None;
        if let Some(harness_session_id) = snapshot.harness_session_id.as_ref() {
            matched = earliest_session_location(
                matched,
                self.by_harness_session_id.get(harness_session_id).copied(),
            );
        }
        matched = earliest_session_location(
            matched,
            self.by_pi_session_id.get(&snapshot.session_id).copied(),
        );
        if let Some(session_file) = snapshot.session_file.as_ref() {
            matched =
                earliest_session_location(matched, self.by_session_file.get(session_file).copied());
        }
        matched
    }

    fn refresh_session(
        &mut self,
        project_index: usize,
        session_index: usize,
        session: &Session,
        prev_pi_session_id: Option<String>,
        prev_session_file: Option<PathBuf>,
    ) {
        let location = (project_index, session_index);
        self.by_harness_session_id
            .insert(session.local_id.clone(), location);

        if let Some(prev_pi_session_id) = prev_pi_session_id.as_ref() {
            if self.by_pi_session_id.get(prev_pi_session_id) == Some(&location) {
                self.by_pi_session_id.remove(prev_pi_session_id);
            }
        }
        if let Some(prev_session_file) = prev_session_file.as_ref() {
            if self.by_session_file.get(prev_session_file) == Some(&location) {
                self.by_session_file.remove(prev_session_file);
            }
        }
        if let Some(pi_session_id) = session.pi_session_id.as_ref() {
            self.by_pi_session_id
                .insert(pi_session_id.clone(), location);
        }
        if let Some(session_file) = session.session_file.as_ref() {
            self.by_session_file.insert(session_file.clone(), location);
        }
    }
}

impl StatusNote {
    fn new(text: String, kind: StatusNoteKind) -> Self {
        Self {
            text,
            kind,
            expires_at_ms: now_millis().saturating_add(5_000),
        }
    }

    fn is_active(&self, now_ms: u64) -> bool {
        now_ms < self.expires_at_ms
    }
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
    terminals_dirty: bool,
    clipboard: Option<Clipboard>,
    note: Option<StatusNote>,
    rail_digest: Option<String>,
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
            terminals_dirty: true,
            clipboard: None,
            note: None,
            rail_digest: None,
        };
        core.workspace.reload_projects_from_disk();
        core.sidecar
            .set_hello(rail_bridge::rail_hello_line(core.config.right_rail_width()));
        if !core.terminal_manager.has_sidecar_extension() {
            core.set_note_error("sidecar extension not found");
        }
        Ok(core)
    }
}

fn earliest_session_location(
    current: Option<(usize, usize)>,
    next: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.min(next)),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

pub(super) struct FrameModel<'a> {
    pub(super) chrome: ChromeView,
    pub(super) sidebar_rows: Vec<SidebarRow>,
    pub(super) sidebar_viewport: Vec<SidebarViewportItem>,
    pub(super) terminal_screen: Option<&'a vt100::Screen>,
    pub(super) terminal_selection: Option<TerminalSelectionRange>,
    pub(super) terminal_max_scrollback: usize,
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
        self.set_note_error(note);
    }

    pub(super) fn set_note_ok(&mut self, note: impl Into<String>) {
        self.note = Some(StatusNote::new(note.into(), StatusNoteKind::Ok));
    }

    pub(super) fn set_note_error(&mut self, note: impl Into<String>) {
        self.note = Some(StatusNote::new(note.into(), StatusNoteKind::Error));
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

    pub(super) fn flush_pending_persist(&mut self, force: bool) {
        self.workspace.flush_persisted_state(force);
    }

    pub(super) fn chrome_view(&self) -> ChromeView {
        let now_ms = now_millis();
        let note = self.note.as_ref().filter(|note| note.is_active(now_ms));
        ChromeView {
            project: self
                .current_project()
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "pi-harness".to_string()),
            status: note.map(|note| note.text.clone()).unwrap_or_else(|| {
                status_text_for_session(
                    self.current_project().is_some(),
                    self.current_session(),
                    self.current_terminal_status(),
                )
            }),
            status_kind: note.map(|note| note.kind),
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
        let errors = {
            let (workspace, terminal_manager) = (&self.workspace, &mut self.terminal_manager);
            let selected_session_id = workspace
                .current_session()
                .map(|session| session.local_id.as_str());
            terminal_manager.sync(workspace.projects(), selected_session_id)
        };
        self.terminals_dirty = false;
        for error in errors {
            self.set_note_text(error);
        }
    }

    fn mark_terminals_dirty(&mut self) {
        self.terminals_dirty = true;
    }

    fn ensure_terminals_synced(&mut self) {
        if self.terminals_dirty {
            self.sync_terminals();
        }
    }

    pub(super) fn drain_terminal_events(&mut self) -> bool {
        let (workspace, terminal_manager) = (&self.workspace, &mut self.terminal_manager);
        let selected_session_id = workspace
            .current_session()
            .map(|session| session.local_id.as_str());
        terminal_manager.drain_events(selected_session_id)
    }

    pub(super) fn selection_changed(&mut self) {
        let selected_session_id = self
            .workspace
            .current_session()
            .map(|session| session.local_id.as_str());
        self.terminal_manager
            .sync_selected_terminal_scroll(selected_session_id);
        self.sync_sidebar_to_selection();
    }

    pub(super) fn selection_changed_with_terminal_sync(&mut self) {
        self.mark_terminals_dirty();
        self.sync_sidebar_to_selection();
    }

    pub(super) fn reload_projects_from_disk(&mut self) {
        self.workspace.reload_projects_from_disk();
        self.mark_terminals_dirty();
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
                self.set_note_ok(format!("{action} {}", result.path.display()));
            }
            Err(note) => self.set_note_text(note),
        }
    }

    pub(super) fn refresh_project_from_scan(&mut self, project_index: usize) {
        self.workspace.refresh_project_from_scan(project_index);
        self.selection_changed_with_terminal_sync();
    }

    pub(super) fn cleanup_archive(&mut self) {
        let evicted = pi::evict_old_archived_sessions(30);
        if evicted > 0 {
            self.set_note_ok(format!(
                "removed {evicted} archived sessions older than 30 days"
            ));
        } else {
            self.set_note_ok("no archived sessions older than 30 days");
        }
    }

    pub(super) fn archive_selected_session(&mut self) {
        let target = match self.workspace.archive_target() {
            Ok(target) => target,
            Err(note) => {
                self.set_note_text(note.to_string());
                return;
            }
        };

        let terminal_stopped = match self.terminal_manager.stop_and_remove(&target.session_id) {
            Ok(stopped) => stopped,
            Err(error) => {
                self.set_note_text(error);
                return;
            }
        };

        if let Some(path) = target.session_file.as_ref() {
            if let Err(error) = pi::archive_session_file(path) {
                if terminal_stopped {
                    self.mark_terminals_dirty();
                }
                self.set_note_text(error);
                return;
            }
        }

        self.workspace.remove_archived_session(&target);
        self.selection_changed_with_terminal_sync();
    }

    pub(super) fn restore_archived_session(
        &mut self,
        archived: &ScannedSession,
    ) -> Result<(), String> {
        let project_path = normalize_project_path(&archived.cwd);
        pi::restore_session_file(&archived.session_file, &project_path)?;

        let project_is_open = self
            .workspace
            .projects()
            .iter()
            .any(|project| project.path == project_path);
        let mut open_note = None;
        if !project_is_open {
            if project_path.is_dir() {
                if let Err(note) = self.workspace.open_project_path(project_path.clone()) {
                    open_note = Some(note);
                }
            } else {
                open_note = Some(format!("project path missing: {}", project_path.display()));
            }
        }

        self.workspace.reload_projects_from_disk();
        self.workspace.restore_selection(
            Some(project_path.to_string_lossy().into_owned()),
            Some(archived.session_id.clone()),
        );
        self.selection_changed_with_terminal_sync();
        self.persist_selection();

        let restored = format!("restored {} → {}", archived.name, project_path.display());
        self.set_note_ok(match open_note {
            Some(note) => format!("{restored} ({note})"),
            None => restored,
        });
        Ok(())
    }

    pub(super) fn new_session(&mut self) {
        self.workspace.new_session();
        self.selection_changed_with_terminal_sync();
    }

    pub(super) fn select_project(&mut self, index: usize) {
        if self.workspace.select_project(index) {
            self.selection_changed();
        }
    }

    pub(super) fn select_session_in_project(&mut self, project_index: usize, session_index: usize) {
        if self
            .workspace
            .select_session_in_project(project_index, session_index)
        {
            self.selection_changed();
        }
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) {
        if self.workspace.cycle_projects(delta) {
            self.selection_changed();
        }
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) {
        if self.workspace.cycle_sessions(delta) {
            self.selection_changed();
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
        self.mark_terminals_dirty();
        self.ensure_terminals_synced();

        if let Some((project_index, session_index)) =
            self.workspace.selected_terminal_restart_target()
        {
            self.restart_terminal_for_session(project_index, session_index);
        }
    }

    pub(super) fn refresh_all_sessions(&mut self) {
        self.reload_projects_from_disk();
        self.ensure_terminals_synced();
        let project_count = self.workspace.projects().len();
        for project_index in 0..project_count {
            self.restart_idle_terminals_for_project(project_index);
        }
    }

    fn apply_sidecar_snapshot(
        &mut self,
        snapshot: PiSidecarSnapshot,
        locator: &mut SessionLocator,
    ) -> bool {
        if !snapshot.is_valid() {
            return false;
        }

        let Some((project_index, session_index)) = locator.locate(&snapshot) else {
            return false;
        };
        let selected = self.workspace.selected_project_index() == project_index
            && self.workspace.selected_session_index() == Some(session_index);
        let selected_session_key = self
            .current_session()
            .map(|session| session.local_id.clone());
        let (update, prev_pi_session_id, prev_session_file) = {
            let session = &mut self.workspace.projects_mut()[project_index].sessions[session_index];
            let prev_pi_session_id = session.pi_session_id.clone();
            let prev_session_file = session.session_file.clone();
            let update = apply_snapshot_to_session(session, &snapshot, selected, now_millis());
            (update, prev_pi_session_id, prev_session_file)
        };

        if update.identity_changed {
            self.mark_terminals_dirty();
        }

        let mut rebuilt = false;
        if update.reordered {
            self.workspace.projects_mut()[project_index].sort_sessions();
            self.restore_selection(None, selected_session_key.clone());
            rebuilt = true;
        }
        if update.promote_project {
            self.promote_project_to_front(project_index);
            rebuilt = true;
        }
        if rebuilt {
            *locator = SessionLocator::rebuild(self.workspace.projects());
        } else if let Some(session) = self
            .workspace
            .projects()
            .get(project_index)
            .and_then(|project| project.sessions.get(session_index))
        {
            locator.refresh_session(
                project_index,
                session_index,
                session,
                prev_pi_session_id,
                prev_session_file,
            );
        }

        true
    }

    pub(super) fn process_background_events(&mut self) -> bool {
        let mut changed = self.drain_terminal_events();
        let mut snapshots = Vec::new();
        while let Some(snapshot) = { self.sidecar.try_recv() } {
            snapshots.push(snapshot);
        }
        if !snapshots.is_empty() {
            let mut locator = SessionLocator::rebuild(self.workspace.projects());
            let mut sidecar_changed = false;
            for snapshot in snapshots {
                sidecar_changed |= self.apply_sidecar_snapshot(snapshot, &mut locator);
            }
            if sidecar_changed {
                self.persist_selection();
            }
            changed |= sidecar_changed;
        }

        let current_note = self.note.take();
        let note = reconcile_terminal_note(
            current_note.as_ref().map(|note| note.text.as_str()),
            self.current_terminal_status(),
        );
        self.note = match note {
            Some(note_text)
                if current_note
                    .as_ref()
                    .is_some_and(|note| note.text == note_text) =>
            {
                current_note
            }
            Some(note_text) => Some(StatusNote::new(note_text, StatusNoteKind::Error)),
            None => None,
        };
        self.sync_rail_digest();
        changed
    }

    /// Broadcast the cross-session digest whenever its JSON changes; runs
    /// every event-loop wake, so selection moves are also captured.
    fn sync_rail_digest(&mut self) {
        let digest = rail_bridge::rail_digest_line(
            self.workspace.projects(),
            self.workspace.selected_project_index(),
            self.workspace.selected_session_index(),
        );
        if self.rail_digest.as_deref() != Some(digest.as_str()) {
            self.sidecar.broadcast(&digest);
            self.rail_digest = Some(digest);
        }
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

    pub(super) fn visible_sidebar_has_spinner(&self, visible_sidebar_rows: usize) -> bool {
        let rows = self.sidebar_rows();
        let sticky_sidebar_anchor =
            self.sticky_sidebar_anchor_row_index(&rows, visible_sidebar_rows);
        sidebar_viewport_items(
            &rows,
            self.sidebar_scroll,
            visible_sidebar_rows,
            sticky_sidebar_anchor,
        )
        .iter()
        .filter_map(|item| rows.get(item.row_index))
        .any(|row| {
            matches!(
                row.status,
                Some(SidebarStatusKind::Active | SidebarStatusKind::Queued)
            )
        })
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
    ) -> FrameModel<'_> {
        self.ensure_terminals_synced();
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
        let (terminal_screen, terminal_selection, terminal_max_scrollback) = self
            .current_terminal()
            .map(|terminal| {
                (
                    Some(terminal.screen()),
                    terminal.selection_range(),
                    terminal.max_scrollback(),
                )
            })
            .unwrap_or((None, None, 0));

        FrameModel {
            chrome: self.chrome_view(),
            sidebar_rows,
            sidebar_viewport,
            terminal_screen,
            terminal_selection,
            terminal_max_scrollback,
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
            self.set_note_ok("pasted image");
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
    #[test]
    fn status_note_expires_after_default_ttl() {
        let note = StatusNote::new("ok".to_string(), StatusNoteKind::Ok);
        assert!(note.is_active(note.expires_at_ms.saturating_sub(1)));
        assert!(!note.is_active(note.expires_at_ms));
    }
}
