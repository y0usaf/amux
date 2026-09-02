//! fx-format session scanner. One directory per session under the flat
//! sessions root; facts come from `session.json`, names from `display.json`
//! with a bounded fallback scan of `events.jsonl` for the first committed
//! user turn (fx's analogue of pi's first user message).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::files::{sessions_root, SESSIONS_DIR_NAME};
use crate::state::ScannedSession;
use crate::util::{normalize_project_path, session_name_from_text};

/// Cap the first-turn fallback so huge event logs cost at most this much IO.
const FIRST_TURN_READ_LIMIT: u64 = 1 << 20;

pub fn scan_live_sessions(project_path: &Path) -> Vec<ScannedSession> {
    scan_sessions_in(sessions_root().as_deref(), Some(project_path))
}

pub fn scan_archived_sessions() -> Vec<ScannedSession> {
    scan_sessions_in(
        super::store::default_agent_dir()
            .map(|dir| dir.join("archive"))
            .as_deref(),
        None,
    )
}

/// Delete archived session dirs whose mtime predates the cutoff. Returns the
/// number deleted.
pub fn evict_old_archived_sessions(max_age_days: u64) -> usize {
    let Some(dir) = super::store::default_agent_dir().map(|dir| dir.join("archive")) else {
        return 0;
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return 0;
    };
    let cutoff_ms = now.as_millis() as u64 - max_age_days * 24 * 3600 * 1000;

    let mut deleted = 0;
    for session_dir in session_dirs(&dir) {
        let Ok(meta) = fs::metadata(&session_dir) else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) else {
            continue;
        };
        if elapsed.as_millis() as u64 <= cutoff_ms && fs::remove_dir_all(&session_dir).is_ok() {
            deleted += 1;
        }
    }
    deleted
}

fn session_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn scan_sessions_in(root: Option<&Path>, project_path: Option<&Path>) -> Vec<ScannedSession> {
    let Some(root) = root else {
        return Vec::new();
    };
    let normalized_project = project_path.map(normalize_project_path);
    let mut sessions = Vec::new();
    for dir in session_dirs(root) {
        let dir_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // Skip non-session directories (fx keeps scratch dirs in the store).
        if dir_name == SESSIONS_DIR_NAME || dir_name.starts_with('.') {
            continue;
        }
        let Some(session) = scanned_session_from_dir(&dir) else {
            continue;
        };
        if let Some(project) = &normalized_project {
            if normalize_project_path(&session.cwd) != *project {
                continue;
            }
        }
        sessions.push(session);
    }
    sessions.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    sessions
}

fn scanned_session_from_dir(dir: &Path) -> Option<ScannedSession> {
    let manifest_path = dir.join("session.json");
    let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path).ok()?).ok()?;

    let session_id = manifest.get("id")?.as_str()?.to_string();
    if session_id.is_empty() {
        return None;
    }
    let cwd = PathBuf::from(manifest.get("workspace_root")?.as_str()?);
    if !cwd.is_absolute() {
        return None;
    }
    let created_at_ms = manifest.get("created_at_ms")?.as_u64()?;
    let updated_at_ms = manifest.get("updated_at_ms").and_then(Value::as_u64);

    let display: Option<Value> = fs::read_to_string(dir.join("display.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let name = display
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|name| !name.trim().is_empty());

    let name_source = name.clone().unwrap_or_else(|| {
        display
            .as_ref()
            .and_then(|value| value.get("preview"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| first_user_turn_text(&dir.join("events.jsonl")).unwrap_or_default())
    });

    Some(ScannedSession {
        session_id,
        session_file: manifest_path,
        cwd,
        created_at_ms,
        updated_at_ms: updated_at_ms.unwrap_or(created_at_ms).max(created_at_ms),
        name: name.unwrap_or_else(|| session_name_from_text(&name_source)),
        interrupted: false,
    })
}

/// Bounded forward scan of the event log for the first
/// `history_turn_committed` payload's user text.
fn first_user_turn_text(events_path: &Path) -> Option<String> {
    use std::io::Read;
    let file = fs::File::open(events_path).ok()?;
    let mut limited = file.take(FIRST_TURN_READ_LIMIT);
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).ok()?;
    for line in buf.split(|byte| *byte == b'\n') {
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if value.get("kind")?.as_str()? != "history_turn_committed" {
            continue;
        }
        let text = value
            .pointer("/payload/turn/user/text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Some(text.to_string());
    }
    None
}
