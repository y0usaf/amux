use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::pi::{live_project_dir, ScannedSession};
use crate::util::{normalize_project_path, session_name_from_text};

pub fn scan_live_sessions(project_path: &Path) -> Vec<ScannedSession> {
    let Some(dir) = live_project_dir(project_path) else {
        return Vec::new();
    };

    let normalized_project_path = normalize_project_path(project_path);
    let mut sessions = Vec::new();
    for path in jsonl_files_in_dir(&dir) {
        let Some(meta) = session_meta_from_path(&path) else {
            continue;
        };
        if normalize_project_path(&meta.cwd) != normalized_project_path {
            continue;
        }
        sessions.push(ScannedSession {
            session_id: meta.session_id,
            session_file: path,
            cwd: meta.cwd,
            created_at_ms: meta.created_at_ms,
            updated_at_ms: meta.updated_at_ms.max(meta.created_at_ms),
            name: meta
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| session_name_from_text(&meta.first_user_message)),
        });
    }

    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    sessions
}

#[derive(Clone)]
struct ScanMeta {
    session_id: String,
    cwd: PathBuf,
    created_at_ms: u64,
    updated_at_ms: u64,
    name: Option<String>,
    first_user_message: String,
}

fn session_meta_from_path(path: &Path) -> Option<ScanMeta> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let header = first_jsonl_value(&mut reader)?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }

    let session_id = header.get("id")?.as_str()?.to_string();
    let cwd = PathBuf::from(header.get("cwd")?.as_str()?);
    let created_at_ms = parse_rfc3339_ms(header.get("timestamp")?.as_str()?)?;

    let mut meta = ScanMeta {
        session_id,
        cwd,
        created_at_ms,
        updated_at_ms: created_at_ms,
        name: None,
        first_user_message: String::new(),
    };
    let mut has_messages = false;

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if let Some(ts) = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
        {
            meta.updated_at_ms = meta.updated_at_ms.max(ts);
        }

        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "session_info" => {
                meta.name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|name| !name.trim().is_empty());
            }
            "message" => {
                let Some(message) = value.get("message") else {
                    continue;
                };
                has_messages = true;
                if meta.first_user_message.is_empty() {
                    if let Some(text) = title_source_from_user_message(message) {
                        if !text.trim().is_empty() {
                            meta.first_user_message = text;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    has_messages.then_some(meta)
}

fn title_source_from_user_message(message: &Value) -> Option<String> {
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let content = message.get("content")?;
    Some(match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => extract_text_from_blocks(blocks),
        _ => String::new(),
    })
}

fn extract_text_from_blocks(blocks: &[Value]) -> String {
    let mut out = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(text);
    }
    out
}

fn jsonl_files_in_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    files
}

fn first_jsonl_value(reader: &mut impl BufRead) -> Option<Value> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return Some(value);
        }
    }
}

fn parse_rfc3339_ms(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|dt| u64::try_from(dt.timestamp_millis()).ok())
}
