use crate::state::Session;
use crate::terminal::TerminalStatus;

pub(super) fn status_text_for_session(
    has_project: bool,
    session: Option<&Session>,
    terminal_status: Option<&TerminalStatus>,
) -> String {
    if let Some(session) = session {
        if let Some(status) = session.runtime.status.as_deref() {
            if session.runtime.queued {
                return format!("{} · queued", status);
            }
            return status.to_string();
        }
        if session.draft {
            return "new session".to_string();
        }

        return terminal_status_label(terminal_status).to_string();
    }

    if has_project {
        return "select a session".to_string();
    }

    "open a project".to_string()
}

pub(super) fn terminal_status_label(status: Option<&TerminalStatus>) -> &'static str {
    match status {
        Some(TerminalStatus::Launching) => "launching",
        Some(TerminalStatus::Running) => "running",
        Some(TerminalStatus::Exited(_)) => "exited",
        Some(TerminalStatus::Error(_)) => "error",
        Some(TerminalStatus::Empty) | None => "idle",
    }
}

#[cfg(test)]
mod tests {
    use super::status_text_for_session;
    use crate::state::Session;

    #[test]
    fn status_text_ignores_tool_name_when_runtime_status_exists() {
        let mut session = Session::new_draft();
        session.runtime.status = Some("thinking".into());
        session.runtime.tool_name = Some("Clipboard".into());

        assert_eq!(
            status_text_for_session(true, Some(&session), None),
            "thinking"
        );
    }

    #[test]
    fn status_text_ignores_stale_tool_name_without_runtime_status() {
        let mut session = Session::new_draft();
        session.draft = false;
        session.runtime.tool_name = Some("Clipboard".into());

        assert_eq!(status_text_for_session(true, Some(&session), None), "idle");
    }
}
