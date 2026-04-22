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

    sessions.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
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
    if !cwd.is_absolute() {
        return None;
    }
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
                if title_source_from_user_message(message).is_some() {
                    has_messages = true;
                }
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
        .filter_map(|entry| {
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() {
                return None;
            }
            (path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")).then_some(path)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set_home(value: &Path) -> Self {
            let lock = test_support::env_lock();
            let old = std::env::var_os("HOME");
            std::env::set_var("HOME", value);
            Self { _lock: lock, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "pi-harness-scan-tests-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn session_meta_requires_session_header_and_user_message_event() {
        let dir = TestDir::new();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n",
                "{\"type\":\"session_info\",\"name\":\"Named\",\"timestamp\":\"2024-01-01T00:00:01Z\"}\n"
            ),
        )
        .unwrap();

        assert!(session_meta_from_path(&path).is_none());
    }

    #[test]
    fn session_meta_rejects_relative_cwd() {
        let dir = TestDir::new();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"abc\",\"cwd\":\".\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n"
            ),
        )
        .unwrap();

        assert!(session_meta_from_path(&path).is_none());
    }

    #[test]
    fn session_meta_ignores_assistant_only_sessions() {
        let dir = TestDir::new();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"content\":\"ignored\"}}\n"
            ),
        )
        .unwrap();

        assert!(session_meta_from_path(&path).is_none());
    }

    #[test]
    fn session_meta_uses_first_user_text_from_text_blocks_and_max_timestamp() {
        let dir = TestDir::new();
        let path = dir.path().join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:03Z\",\"message\":{\"role\":\"assistant\",\"content\":\"ignored\"}}\n",
                "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:02Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool\",\"text\":\"ignored\"},{\"type\":\"text\",\"text\":\"first line\"},{\"type\":\"text\",\"text\":\"second line\"}]}}\n",
                "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:05Z\",\"message\":{\"role\":\"user\",\"content\":\"later\"}}\n"
            ),
        )
        .unwrap();

        let meta = session_meta_from_path(&path).unwrap();
        assert_eq!(meta.session_id, "abc");
        assert_eq!(meta.cwd, PathBuf::from("/tmp/project"));
        assert_eq!(meta.name, None);
        assert_eq!(meta.first_user_message, "first line\nsecond line");
        assert_eq!(
            meta.updated_at_ms,
            parse_rfc3339_ms("2024-01-01T00:00:05Z").unwrap()
        );
    }

    #[test]
    fn first_jsonl_value_skips_blank_and_non_json_lines() {
        let mut input = std::io::Cursor::new(
            b"\nnot-json\n {still not json}\n{\"type\":\"session\",\"id\":\"ok\"}\n",
        );
        let value = first_jsonl_value(&mut input).unwrap();
        assert_eq!(value.get("id").and_then(Value::as_str), Some("ok"));
    }

    #[test]
    fn extract_text_from_blocks_joins_only_text_blocks() {
        let blocks = vec![
            serde_json::json!({"type": "tool", "text": "ignored"}),
            serde_json::json!({"type": "text", "text": "hello"}),
            serde_json::json!({"type": "text", "text": "world"}),
            serde_json::json!({"type": "text", "value": "ignored"}),
        ];

        assert_eq!(extract_text_from_blocks(&blocks), "hello\nworld");
    }

    #[test]
    fn scan_live_sessions_filters_by_project_and_sorts_newest_first() {
        let home = TestDir::new();
        let _guard = EnvGuard::set_home(home.path());
        let project = home.path().join("work/project");
        fs::create_dir_all(&project).unwrap();
        let live_dir = live_project_dir(&project).unwrap();
        fs::create_dir_all(&live_dir).unwrap();

        fs::write(
            live_dir.join("a.jsonl"),
            "{\"type\":\"session\",\"id\":\"a\",\"cwd\":\"/tmp/placeholder\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n"
                .replace("/tmp/placeholder", &project.to_string_lossy())
                + "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"older\"}}\n",
        )
        .unwrap();
        fs::write(
            live_dir.join("b.jsonl"),
            "{\"type\":\"session\",\"id\":\"b\",\"cwd\":\"/tmp/placeholder\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n"
                .replace("/tmp/placeholder", &project.to_string_lossy())
                + concat!(
                    "{\"type\":\"session_info\",\"name\":\"Named\",\"timestamp\":\"2024-01-01T00:00:02Z\"}\n",
                    "{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:03Z\",\"message\":{\"role\":\"user\",\"content\":\"newer\"}}\n"
                ),
        )
        .unwrap();
        fs::write(
            live_dir.join("foreign.jsonl"),
            "{\"type\":\"session\",\"id\":\"foreign\",\"cwd\":\"/tmp/other-project\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:04Z\",\"message\":{\"role\":\"user\",\"content\":\"ignored\"}}\n",
        )
        .unwrap();

        let sessions = scan_live_sessions(&project);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "b");
        assert_eq!(sessions[0].name, "Named");
        assert_eq!(sessions[1].session_id, "a");
        assert_eq!(sessions[1].name, "older");
        assert!(sessions.iter().all(|session| session.cwd == project));
    }

    #[test]
    fn scan_live_sessions_breaks_equal_timestamp_ties_by_session_id() {
        let home = TestDir::new();
        let _guard = EnvGuard::set_home(home.path());
        let project = home.path().join("work/project");
        fs::create_dir_all(&project).unwrap();
        let live_dir = live_project_dir(&project).unwrap();
        fs::create_dir_all(&live_dir).unwrap();

        for session_id in ["b", "a"] {
            fs::write(
                live_dir.join(format!("{session_id}.jsonl")),
                format!(
                    "{{\"type\":\"session\",\"id\":\"{}\",\"cwd\":\"{}\",\"timestamp\":\"2024-01-01T00:00:00Z\"}}\n{{\"type\":\"message\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}\n",
                    session_id,
                    project.to_string_lossy(),
                    session_id,
                ),
            )
            .unwrap();
        }

        let sessions = scan_live_sessions(&project);
        let ids: Vec<_> = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn jsonl_files_in_dir_skips_non_files() {
        let dir = TestDir::new();
        fs::create_dir_all(dir.path().join("nested.jsonl")).unwrap();
        fs::write(dir.path().join("real.jsonl"), "{}").unwrap();

        let files = jsonl_files_in_dir(dir.path());
        assert_eq!(files, vec![dir.path().join("real.jsonl")]);
    }
}
