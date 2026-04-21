#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AppAction {
    PreviousProject,
    NextProject,
    PreviousSession,
    NextSession,
    OpenProjectPicker,
    NewSession,
    RefreshSession,
    RefreshAllSessions,
    ArchiveSession,
    RemoveProject,
    CopySelection,
    PasteClipboard,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub action: AppAction,
    pub name: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub defaults: &'static [&'static str],
}

pub const ACTION_SPECS: &[ActionSpec] = &[
    ActionSpec {
        action: AppAction::PreviousProject,
        name: "project_prev",
        label: "previous project",
        group: "navigation",
        defaults: &["ctrl+left"],
    },
    ActionSpec {
        action: AppAction::NextProject,
        name: "project_next",
        label: "next project",
        group: "navigation",
        defaults: &["ctrl+right"],
    },
    ActionSpec {
        action: AppAction::PreviousSession,
        name: "session_prev",
        label: "previous session",
        group: "navigation",
        defaults: &["ctrl+up"],
    },
    ActionSpec {
        action: AppAction::NextSession,
        name: "session_next",
        label: "next session",
        group: "navigation",
        defaults: &["ctrl+down"],
    },
    ActionSpec {
        action: AppAction::OpenProjectPicker,
        name: "open_project_picker",
        label: "open project picker",
        group: "project",
        defaults: &["ctrl+o"],
    },
    ActionSpec {
        action: AppAction::NewSession,
        name: "new_session",
        label: "new session",
        group: "session",
        defaults: &["ctrl+n"],
    },
    ActionSpec {
        action: AppAction::RefreshSession,
        name: "refresh_session",
        label: "refresh session",
        group: "session",
        defaults: &["ctrl+r"],
    },
    ActionSpec {
        action: AppAction::RefreshAllSessions,
        name: "refresh_all_sessions",
        label: "refresh all sessions",
        group: "session",
        defaults: &["ctrl+shift+r"],
    },
    ActionSpec {
        action: AppAction::ArchiveSession,
        name: "archive_session",
        label: "archive session",
        group: "session",
        defaults: &["ctrl+delete"],
    },
    ActionSpec {
        action: AppAction::RemoveProject,
        name: "remove_project",
        label: "remove project",
        group: "project",
        defaults: &["ctrl+shift+delete", "ctrl+shift+d"],
    },
    ActionSpec {
        action: AppAction::CopySelection,
        name: "copy_selection",
        label: "copy selection",
        group: "terminal",
        defaults: &["ctrl+shift+c", "cmd+c"],
    },
    ActionSpec {
        action: AppAction::PasteClipboard,
        name: "paste_clipboard",
        label: "paste clipboard",
        group: "terminal",
        defaults: &["ctrl+v", "cmd+v", "shift+insert"],
    },
    ActionSpec {
        action: AppAction::ZoomIn,
        name: "zoom_in",
        label: "zoom in",
        group: "view",
        defaults: &["ctrl+equal", "ctrl+plus", "cmd+equal", "cmd+plus"],
    },
    ActionSpec {
        action: AppAction::ZoomOut,
        name: "zoom_out",
        label: "zoom out",
        group: "view",
        defaults: &["ctrl+minus", "cmd+minus"],
    },
    ActionSpec {
        action: AppAction::ZoomReset,
        name: "zoom_reset",
        label: "zoom reset",
        group: "view",
        defaults: &["ctrl+0", "cmd+0"],
    },
];

pub fn action_spec(action: AppAction) -> &'static ActionSpec {
    ACTION_SPECS
        .iter()
        .find(|spec| spec.action == action)
        .expect("action spec")
}

pub(super) fn action_spec_by_name(name: &str) -> Option<&'static ActionSpec> {
    ACTION_SPECS.iter().find(|spec| spec.name == name)
}
