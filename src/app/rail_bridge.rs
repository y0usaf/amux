//! Harness → sidechannel downstream payloads for the in-Pi right rail.
//!
//! The rail extension (pi-extension/) renders inside each Pi PTY. The harness
//! owns rail policy: width and palette travel in a sticky `hello` line, the
//! cross-session summary travels in `digest` lines. Wire vocabulary is
//! additive JSON-lines; unknown types are ignored on both ends.

use serde_json::json;

use crate::render::Color;
use crate::state::Project;

use super::theme::{
    ACCENT, ACCENT_2, BORDER, ERROR, HEADING, MUTED, RUNNING, SUCCESS, TEXT, WARNING,
};

fn hex(color: Color) -> String {
    let (r, g, b) = color.rgb_components();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Sticky hello line: rail width plus the harness palette so the rail can
/// match the left sidebar exactly. `rail_width == 0` disables the rail.
pub(super) fn rail_hello_line(rail_width: u16) -> String {
    json!({
        "type": "hello",
        "railWidth": rail_width,
        "palette": {
            "text": hex(TEXT),
            "muted": hex(MUTED),
            "heading": hex(HEADING),
            "accent": hex(ACCENT),
            "accent2": hex(ACCENT_2),
            "running": hex(RUNNING),
            "warning": hex(WARNING),
            "error": hex(ERROR),
            "success": hex(SUCCESS),
            "border": hex(BORDER),
        },
    })
    .to_string()
}

/// Cross-session digest line. Every connected extension receives the same
/// digest and identifies its own entry by `key` == its session env key.
pub(super) fn rail_digest_line(
    projects: &[Project],
    selected_project: usize,
    selected_session: Option<usize>,
) -> String {
    let mut sessions = Vec::new();
    for (project_index, project) in projects.iter().enumerate() {
        for (session_index, session) in project.sessions.iter().enumerate() {
            if !session.should_render_in_sidebar() {
                continue;
            }
            let selected =
                project_index == selected_project && selected_session == Some(session_index);
            sessions.push(json!({
                "key": session.local_id,
                "name": session.name,
                "project": project.name,
                "stage": session
                    .runtime
                    .status
                    .clone()
                    .unwrap_or_else(|| "idle".to_string()),
                "queued": session.runtime.queued,
                "interrupted": session.runtime.interrupted,
                "unread": session.runtime.unread,
                "selected": selected,
            }));
        }
    }
    json!({ "type": "digest", "sessions": sessions }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Session;
    use std::path::PathBuf;

    #[test]
    fn hello_line_is_versionless_additive_json() {
        let line = rail_hello_line(44);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["railWidth"], 44);
        let text = value["palette"]["text"].as_str().unwrap();
        assert!(text.starts_with('#') && text.len() == 7);
    }

    #[test]
    fn digest_marks_selected_session_and_skips_hidden_drafts() {
        let mut project = Project::new(PathBuf::from("/tmp/demo"));
        let mut running = Session::new_draft();
        running.name = "run".into();
        running.pi_session_id = Some("pi-1".into());
        running.draft = false;
        running.runtime.running = true;
        running.runtime.status = Some("tool".into());
        let hidden_draft = Session::new_draft();
        project.sessions = vec![running, hidden_draft];

        let line = rail_digest_line(&[project], 0, Some(0));
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        let sessions = value["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1); // ephemeral draft is filtered out
        assert_eq!(sessions[0]["stage"], "tool");
        assert_eq!(sessions[0]["selected"], true);
    }
}
