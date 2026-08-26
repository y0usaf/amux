#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AppAction {
    PreviousProject,
    NextProject,
    PreviousSession,
    NextSession,
    NewSession,
    RefreshSession,
    RefreshAllSessions,
    ArchiveSession,
    RemoveProject,
    CopySelection,
    PasteClipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub action: AppAction,
    pub name: &'static str,
    pub defaults: &'static [&'static str],
}

pub const ACTION_SPECS: &[ActionSpec] = &[
    ActionSpec {
        action: AppAction::PreviousProject,
        name: "project_prev",
        defaults: &["ctrl+left"],
    },
    ActionSpec {
        action: AppAction::NextProject,
        name: "project_next",
        defaults: &["ctrl+right"],
    },
    ActionSpec {
        action: AppAction::PreviousSession,
        name: "session_prev",
        defaults: &["ctrl+up"],
    },
    ActionSpec {
        action: AppAction::NextSession,
        name: "session_next",
        defaults: &["ctrl+down"],
    },
    ActionSpec {
        action: AppAction::NewSession,
        name: "new_session",
        defaults: &["ctrl+n"],
    },
    ActionSpec {
        action: AppAction::RefreshSession,
        name: "refresh_session",
        defaults: &["ctrl+r"],
    },
    ActionSpec {
        action: AppAction::RefreshAllSessions,
        name: "refresh_all_sessions",
        defaults: &["ctrl+shift+r"],
    },
    ActionSpec {
        action: AppAction::ArchiveSession,
        name: "archive_session",
        defaults: &["ctrl+delete"],
    },
    ActionSpec {
        action: AppAction::RemoveProject,
        name: "remove_project",
        defaults: &["ctrl+shift+delete", "ctrl+shift+d"],
    },
    ActionSpec {
        action: AppAction::CopySelection,
        name: "copy_selection",
        defaults: &["ctrl+shift+c"],
    },
    ActionSpec {
        action: AppAction::PasteClipboard,
        name: "paste_clipboard",
        defaults: &["ctrl+v", "shift+insert"],
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
