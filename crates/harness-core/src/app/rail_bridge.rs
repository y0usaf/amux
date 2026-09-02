//! Harness → sidechannel downstream payloads for the in-Pi right rail.
//!
//! The rail extension (pi-extension/) renders inside each Pi PTY. The harness
//! owns rail policy: width travels in a sticky `hello` line, the cross-session
//! summary travels in `digest` lines. Wire vocabulary is
//! additive JSON-lines; unknown types are ignored on both ends.

use serde_json::json;

use crate::state::Project;

/// Sticky hello line: rail width. `rail_width == 0` disables the rail.
pub(super) fn rail_hello_line(rail_width: u16) -> String {
    json!({
        "type": "hello",
        "railWidth": rail_width,
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
