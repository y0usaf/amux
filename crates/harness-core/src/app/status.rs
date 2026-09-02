use crate::state::Session;
use crate::terminal::TerminalStatus;

pub(super) fn status_text_for_session(
    has_project: bool,
    session: Option<&Session>,
    terminal_status: Option<&TerminalStatus>,
) -> String {
    if let Some(session) = session {
        if let Some(status) = session.runtime.status.as_deref() {
            let mut status = if status == "tool" {
                session
                    .runtime
                    .tool_name
                    .as_deref()
                    .map(|tool| format!("tool · {tool}"))
                    .unwrap_or_else(|| status.to_string())
            } else {
                status.to_string()
            };
            if session.runtime.queued {
                status.push_str(" · queued");
            }
            return status;
        }
        if session.runtime.interrupted {
            return "interrupted".to_string();
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
