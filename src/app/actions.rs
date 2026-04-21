use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidecarOrderUpdate {
    None,
    Touch,
    Promote,
}

pub(super) fn should_bind_sidecar_session(session: &Session, snapshot: &PiSidecarSnapshot) -> bool {
    session.pi_session_id.is_some()
        || session.session_file.is_some()
        || session.runtime.running
        || snapshot.stage.is_active()
        || snapshot.queued
}

pub(super) fn sidecar_order_update(
    prev_running: bool,
    running: bool,
    prev_trackable: bool,
    trackable: bool,
) -> SidecarOrderUpdate {
    if running {
        if !trackable {
            SidecarOrderUpdate::None
        } else if !prev_running || !prev_trackable {
            SidecarOrderUpdate::Promote
        } else {
            SidecarOrderUpdate::None
        }
    } else if prev_running {
        if !trackable {
            SidecarOrderUpdate::None
        } else if !prev_trackable {
            SidecarOrderUpdate::Promote
        } else {
            SidecarOrderUpdate::Touch
        }
    } else {
        SidecarOrderUpdate::None
    }
}

pub(super) fn normalize_unique_project_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::with_capacity(paths.len());

    for path in paths.into_iter().map(|path| normalize_project_path(&path)) {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }

    unique
}

impl App {
    pub(super) fn reload_projects_from_disk(&mut self) {
        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());

        let mut project_paths: Vec<PathBuf> = if !self.persisted.projects.is_empty() {
            self.persisted.projects.iter().map(PathBuf::from).collect()
        } else if !self.initial_project_paths.is_empty() {
            self.initial_project_paths.clone()
        } else {
            vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
        };

        project_paths = normalize_unique_project_paths(project_paths);

        let mut next_projects = Vec::with_capacity(project_paths.len());
        for path in project_paths {
            let mut project = self
                .projects
                .iter()
                .find(|project| project.path == path)
                .cloned()
                .unwrap_or_else(|| Project::new(path.clone()));
            project.path = path.clone();
            project.name = crate::util::project_name_from_path(&path);
            merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&path));
            project.sort_sessions();
            next_projects.push(project);
        }
        self.projects = next_projects;

        self.restore_selection(selected_project_key, selected_session_key);
        self.persist_selection();
    }

    pub(super) fn restore_selection(
        &mut self,
        project_key: Option<String>,
        session_key: Option<String>,
    ) {
        if self.projects.is_empty() {
            self.selected_project = 0;
            self.selected_session = None;
            self.sync_sidebar_to_selection();
            return;
        }

        let persisted_project = project_key.or_else(|| self.persisted.selected_project.clone());
        self.selected_project = persisted_project
            .as_deref()
            .and_then(|key| {
                self.projects
                    .iter()
                    .position(|project| project.selection_key() == key)
            })
            .unwrap_or(0);

        let desired_session = session_key.or_else(|| self.persisted.selected_session.clone());
        self.selected_session = desired_session.as_deref().and_then(|key| {
            self.projects[self.selected_project]
                .sessions
                .iter()
                .position(|session| session.selection_key() == key || session.local_id == key)
        });

        if self.selected_session.is_none() {
            self.selected_session = if self.projects[self.selected_project].sessions.is_empty() {
                self.ensure_default_session_for_project(self.selected_project)
            } else {
                Some(0)
            };
        }
        self.sync_sidebar_to_selection();
    }

    pub(super) fn open_project_picker(&mut self) {
        let start_dir = self
            .current_project()
            .map(|project| project.path.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        match pick_project_directory(&start_dir) {
            Ok(Some(path)) => self.add_project(&path),
            Ok(None) | Err(_) => {}
        }
    }

    pub(super) fn add_project(&mut self, path: &Path) {
        let path = normalize_project_path(path);
        if !path.exists() || !path.is_dir() {
            self.set_note(format!("invalid project path: {}", path.display()));
            return;
        }
        if self.projects.iter().any(|project| project.path == path) {
            self.set_note("project already added");
            return;
        }

        let _ = self.prepare_selection_change();
        let mut project = Project::new(path.clone());
        merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&path));
        project.sort_sessions();
        self.projects.push(project);
        self.selected_project = self.projects.len().saturating_sub(1);
        self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn promote_project_to_front(&mut self, project_index: usize) {
        if project_index == 0 || project_index >= self.projects.len() {
            return;
        }

        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());
        let project = self.projects.remove(project_index);
        self.projects.insert(0, project);
        self.restore_selection(selected_project_key, selected_session_key);
    }

    pub(super) fn remove_selected_project(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        self.projects.remove(self.selected_project);
        if self.projects.is_empty() {
            self.selected_project = 0;
            self.selected_session = None;
        } else {
            self.selected_project = self.selected_project.min(self.projects.len() - 1);
            self.selected_session = self.ensure_default_session_for_project(self.selected_project);
        }
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn archive_selected_session(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };
        let Some(session_index) = self.selected_session else {
            self.set_note("no session selected");
            return;
        };
        let Some(session) = project.sessions.get(session_index) else {
            return;
        };

        if session.runtime.running {
            self.set_note("cannot archive running session");
            return;
        }

        if let Some(path) = session.session_file.as_ref() {
            if let Err(error) = pi::archive_session_file(path) {
                self.set_note(error);
                return;
            }
        }

        let project_index = self.selected_project;
        let session_id = session.local_id.clone();
        let next_selected_session = {
            let project = &mut self.projects[project_index];
            project.sessions.remove(session_index);
            if project.sessions.is_empty() {
                None
            } else {
                Some(session_index.min(project.sessions.len() - 1))
            }
        };
        self.terminals.remove(&session_id);
        self.selected_session = next_selected_session
            .or_else(|| self.ensure_default_session_for_project(project_index));
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn new_session(&mut self) {
        let _ = self.prepare_selection_change();
        let Some(project) = self.current_project_mut() else {
            return;
        };
        project.sessions.insert(0, Session::new_draft());
        self.selected_session = Some(0);
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn select_project(&mut self, index: usize) {
        if index >= self.projects.len() {
            return;
        }
        let _ = self.prepare_selection_change();
        self.selected_project = index;
        self.selected_session = if self.projects[index].sessions.is_empty() {
            self.ensure_default_session_for_project(index)
        } else {
            Some(0)
        };
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn select_session(&mut self, index: usize) {
        let project_index = self.selected_project;
        self.select_session_in_project(project_index, index);
    }

    pub(super) fn select_session_in_project(&mut self, project_index: usize, session_index: usize) {
        let Some(project) = self.projects.get(project_index) else {
            return;
        };
        if session_index >= project.sessions.len() {
            return;
        }
        let removed = self.prepare_selection_change();
        let session_index =
            Self::adjust_session_index_after_removal(project_index, session_index, removed);
        let Some(project) = self.projects.get(project_index) else {
            return;
        };
        if session_index >= project.sessions.len() {
            return;
        }
        self.selected_project = project_index;
        self.selected_session = Some(session_index);
        if let Some(session) = self.current_session_mut() {
            session.runtime.unread = false;
        }
        self.sync_sidebar_to_selection();
        self.persist_selection();
        self.sync_terminals();
    }

    pub(super) fn cycle_projects(&mut self, delta: i32) {
        if self.projects.is_empty() {
            return;
        }
        let len = self.projects.len() as i32;
        let next = (self.selected_project as i32 + delta).rem_euclid(len) as usize;
        self.select_project(next);
    }

    pub(super) fn cycle_sessions(&mut self, delta: i32) {
        let Some(project) = self.current_project() else {
            return;
        };
        if project.sessions.is_empty() {
            return;
        }
        let current = self.selected_session.unwrap_or(0) as i32;
        let len = project.sessions.len() as i32;
        let next = (current + delta).rem_euclid(len) as usize;
        self.select_session(next);
    }

    pub(super) fn refresh_project_from_scan(&mut self, project_index: usize) {
        let Some(project_path) = self
            .projects
            .get(project_index)
            .map(|project| project.path.clone())
        else {
            return;
        };

        let selected_project_key = self
            .current_project()
            .map(|project| project.selection_key());
        let selected_session_key = self
            .current_session()
            .map(|session| session.selection_key());

        if let Some(project) = self.projects.get_mut(project_index) {
            merge_scanned_sessions(&mut project.sessions, pi::scan_live_sessions(&project_path));
            project.sort_sessions();
        }

        self.restore_selection(selected_project_key, selected_session_key);
        self.persist_selection();
    }

    pub(super) fn restart_terminal_for_session(
        &mut self,
        project_index: usize,
        session_index: usize,
    ) {
        if self
            .projects
            .get(project_index)
            .and_then(|project| project.sessions.get(session_index))
            .is_some_and(|session| session.runtime.running)
        {
            return;
        }

        let Some((session_id, target)) =
            self.terminal_target_for_session(project_index, session_index)
        else {
            return;
        };

        let proxy = self.proxy.clone();
        let restart_result: anyhow::Result<()> = (|| {
            let terminal = self
                .terminals
                .entry(session_id)
                .or_insert_with(|| TerminalController::new(proxy.clone()));
            let _ = terminal.attach(None)?;
            let _ = terminal.attach(Some(target))?;
            Ok(())
        })();
        if let Err(error) = restart_result {
            self.set_note(format!("terminal: {error}"));
        }
    }

    pub(super) fn restart_idle_terminals_for_project(&mut self, project_index: usize) {
        let Some(project) = self.projects.get(project_index) else {
            return;
        };

        let session_ids: Vec<String> = project
            .sessions
            .iter()
            .filter(|session| !session.runtime.running)
            .map(|session| session.local_id.clone())
            .collect();

        for session_id in session_ids {
            let Some(terminal) = self.terminals.get_mut(&session_id) else {
                continue;
            };
            if let Err(error) = terminal.restart() {
                self.set_note(format!("terminal: {error}"));
            }
        }
    }

    pub(super) fn refresh_current_session(&mut self) {
        if self.selected_session.is_none() {
            self.set_note("no session selected");
            return;
        }

        if self
            .current_session()
            .is_some_and(|session| session.runtime.running)
        {
            self.set_note("cannot refresh running session");
            return;
        }

        let project_index = self.selected_project;
        self.refresh_project_from_scan(project_index);
        self.sync_terminals();

        if let Some(session_index) = self.selected_session {
            self.restart_terminal_for_session(self.selected_project, session_index);
        }
    }

    pub(super) fn refresh_all_sessions(&mut self) {
        self.reload_projects_from_disk();
        self.sync_terminals();
        for project_index in 0..self.projects.len() {
            self.restart_idle_terminals_for_project(project_index);
        }
    }

    pub(super) fn sync_terminals(&mut self) {
        let mut active_ids = HashSet::new();
        let proxy = self.proxy.clone();

        for project_index in 0..self.projects.len() {
            let session_count = self.projects[project_index].sessions.len();
            for session_index in 0..session_count {
                let Some((session_id, target)) =
                    self.terminal_target_for_session(project_index, session_index)
                else {
                    continue;
                };
                active_ids.insert(session_id.clone());

                let attach_result = {
                    let terminal = self
                        .terminals
                        .entry(session_id)
                        .or_insert_with(|| TerminalController::new(proxy.clone()));
                    terminal.attach(Some(target))
                };
                if let Err(error) = attach_result {
                    self.set_note(format!("terminal: {error}"));
                }
            }
        }

        let stale_ids: Vec<_> = self
            .terminals
            .keys()
            .filter(|session_id| !active_ids.contains(*session_id))
            .cloned()
            .collect();
        for session_id in stale_ids {
            self.terminals.remove(&session_id);
        }

        self.update_window_title();
    }

    pub(super) fn apply_sidecar_snapshot(&mut self, snapshot: PiSidecarSnapshot) {
        if !snapshot.is_valid() {
            return;
        }

        let mut matched = None;
        for (project_index, project) in self.projects.iter().enumerate() {
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
            return;
        };
        let selected =
            self.selected_project == project_index && self.selected_session == Some(session_index);
        let selected_session_key = self
            .current_session()
            .map(|session| session.local_id.clone());
        let timestamp = snapshot.ts_ms.max(now_millis());

        let mut reordered = false;
        let mut promote_project = false;
        let session = &mut self.projects[project_index].sessions[session_index];
        let prev_running = session.runtime.running;
        let prev_trackable = session.counts_for_activity_ordering();
        let should_bind = should_bind_sidecar_session(session, &snapshot);
        let mut title_changed = false;

        if should_bind {
            session
                .pi_session_id
                .get_or_insert(snapshot.session_id.clone());
            if let Some(path) = snapshot.session_file.clone() {
                session.session_file = Some(path);
            }
            if let Some(name) = snapshot
                .session_name
                .as_ref()
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
            {
                if session.should_adopt_name(name) && session.name != name {
                    session.name = name.to_string();
                    title_changed = true;
                }
            }
            session.draft = false;
        }

        session.runtime.running = snapshot.stage.is_active();
        session.runtime.status = snapshot.stage.as_runtime_status().map(ToOwned::to_owned);
        session.runtime.queued = snapshot.queued;
        session.runtime.tool_name = snapshot.tool_name.clone();

        let trackable = session.counts_for_activity_ordering();
        match sidecar_order_update(
            prev_running,
            session.runtime.running,
            prev_trackable,
            trackable,
        ) {
            SidecarOrderUpdate::Promote => {
                session.promote_at(timestamp);
                reordered = true;
                promote_project = true;
            }
            SidecarOrderUpdate::Touch => {
                session.touch_at(timestamp);
                reordered = true;
            }
            SidecarOrderUpdate::None => {
                if title_changed && snapshot.ts_ms > session.updated_at_ms {
                    session.updated_at_ms = snapshot.ts_ms;
                    reordered = true;
                }
            }
        }

        if prev_running && !session.runtime.running && trackable && !selected {
            session.runtime.unread = true;
        }

        if reordered {
            self.projects[project_index].sort_sessions();
            self.restore_selection(None, selected_session_key);
        }
        if promote_project {
            self.promote_project_to_front(project_index);
        }
        self.persist_selection();
        self.update_window_title();
    }

    pub(super) fn process_background_events(&mut self) {
        let selected_session_id = self
            .current_session()
            .map(|session| session.local_id.clone());
        let mut changed = false;
        for (session_id, terminal) in self.terminals.iter_mut() {
            let terminal_changed = terminal.drain_events();
            if terminal_changed && selected_session_id.as_deref() == Some(session_id.as_str()) {
                changed = true;
            }
        }
        while let Some(snapshot) = self.sidecar.try_recv() {
            self.apply_sidecar_snapshot(snapshot);
            changed = true;
        }

        match self.current_terminal_status() {
            Some(TerminalStatus::Error(error)) => self.note = Some(error.clone()),
            Some(TerminalStatus::Exited(status)) => {
                self.note = Some(format!("terminal exited: {status}"))
            }
            Some(TerminalStatus::Empty | TerminalStatus::Launching | TerminalStatus::Running)
            | None => {}
        }

        if changed {
            self.request_redraw();
        }
    }
}

fn pick_project_directory(start_dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut filename_arg = OsString::from("--filename=");
    filename_arg.push(start_dir.as_os_str());
    if !start_dir.as_os_str().is_empty() && !start_dir.to_string_lossy().ends_with('/') {
        filename_arg.push("/");
    }

    let output = Command::new("zenity")
        .arg("--file-selection")
        .arg("--directory")
        .arg("--title=Open project")
        .arg(filename_arg)
        .output()
        .map_err(|error| format!("spawn zenity: {error}"))?;

    match output.status.code() {
        Some(0) => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(path)))
            }
        }
        Some(1) => Ok(None),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                Err(format!("zenity exited with {}", output.status))
            } else {
                Err(stderr)
            }
        }
    }
}
