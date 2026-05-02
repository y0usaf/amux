use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde_json::Value;

const PI_AGENT_DIR_ENV: &str = "PI_AGENT_DIR";
const DEFAULT_PI_AGENT_SESSIONS_REL: &str = ".pi/agent/sessions";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PiUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_cost: f64,
}

impl PiUsageTotals {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_creation_tokens)
            .saturating_add(self.cache_read_tokens)
    }

    fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(other.cache_creation_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.total_cost += other.total_cost;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PiUsageModelBreakdown {
    pub model_name: String,
    pub totals: PiUsageTotals,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PiUsageDay {
    pub date: String,
    pub totals: PiUsageTotals,
    pub models_used: Vec<String>,
    pub model_breakdowns: Vec<PiUsageModelBreakdown>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PiUsageReport {
    pub days: Vec<PiUsageDay>,
    pub totals: PiUsageTotals,
    pub files_scanned: usize,
    pub entries: usize,
    pub skipped_duplicates: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct UsageEntry {
    timestamp: String,
    date: String,
    model: Option<String>,
    totals: PiUsageTotals,
    total_tokens_for_dedupe: u64,
}

#[derive(Default)]
struct DayAccumulator {
    totals: PiUsageTotals,
    models: BTreeMap<String, PiUsageTotals>,
}

pub fn load_usage_report() -> PiUsageReport {
    let Some(path) = default_usage_path() else {
        return PiUsageReport::default();
    };
    load_usage_report_from_path(&path)
}

pub fn load_usage_report_from_path(path: &Path) -> PiUsageReport {
    if !path.is_dir() {
        return PiUsageReport::default();
    }

    let mut files = Vec::new();
    collect_jsonl_files(path, &mut files);
    files.sort();

    let mut report = PiUsageReport {
        files_scanned: files.len(),
        ..PiUsageReport::default()
    };
    let mut processed_hashes = HashSet::new();
    let mut by_day = BTreeMap::<String, DayAccumulator>::new();

    for file in files {
        for entry in usage_entries_from_file(&file) {
            let hash = format!("pi:{}:{}", entry.timestamp, entry.total_tokens_for_dedupe);
            if !processed_hashes.insert(hash) {
                report.skipped_duplicates = report.skipped_duplicates.saturating_add(1);
                continue;
            }

            report.entries = report.entries.saturating_add(1);
            report.totals.add(&entry.totals);

            let day = by_day.entry(entry.date).or_default();
            day.totals.add(&entry.totals);
            let model_name = entry.model.unwrap_or_else(|| "unknown".to_string());
            day.models.entry(model_name).or_default().add(&entry.totals);
        }
    }

    report.days = by_day
        .into_iter()
        .map(|(date, day)| {
            let model_breakdowns = day
                .models
                .into_iter()
                .map(|(model_name, totals)| PiUsageModelBreakdown { model_name, totals })
                .collect::<Vec<_>>();
            let models_used = model_breakdowns
                .iter()
                .map(|breakdown| breakdown.model_name.clone())
                .collect();
            PiUsageDay {
                date,
                totals: day.totals,
                models_used,
                model_breakdowns,
            }
        })
        .collect();
    report.days.sort_by(|a, b| b.date.cmp(&a.date));

    report
}

fn default_usage_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(PI_AGENT_DIR_ENV) {
        let path = path.to_string_lossy().trim().to_string();
        if !path.is_empty() {
            let resolved = resolve_path(Path::new(&path));
            if resolved.is_dir() {
                return Some(resolved);
            }
        }
    }

    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let path = home.join(DEFAULT_PI_AGENT_SESSIONS_REL);
    path.is_dir().then_some(path)
}

fn resolve_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_jsonl_files(&path, files);
        } else if file_type.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
}

fn usage_entries_from_file(path: &Path) -> Vec<UsageEntry> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(entry) = usage_entry_from_value(&value) {
            entries.push(entry);
        }
    }

    entries
}

fn usage_entry_from_value(value: &Value) -> Option<UsageEntry> {
    let kind = value.get("type").and_then(Value::as_str);
    if kind.is_some_and(|kind| kind != "message") {
        return None;
    }

    let timestamp = value.get("timestamp")?.as_str()?;
    let date = date_key_from_timestamp(timestamp)?;
    let message = value.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }

    let usage = message.get("usage")?;
    let input_tokens = number_as_u64(usage.get("input")?)?;
    let output_tokens = number_as_u64(usage.get("output")?)?;
    let cache_read_tokens = usage.get("cacheRead").and_then(number_as_u64).unwrap_or(0);
    let cache_creation_tokens = usage.get("cacheWrite").and_then(number_as_u64).unwrap_or(0);
    let total_tokens_for_dedupe = usage
        .get("totalTokens")
        .and_then(number_as_u64)
        .unwrap_or_else(|| {
            input_tokens
                .saturating_add(output_tokens)
                .saturating_add(cache_read_tokens)
                .saturating_add(cache_creation_tokens)
        });
    let total_cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .map(|model| format!("[pi] {model}"));

    Some(UsageEntry {
        timestamp: timestamp.to_string(),
        date,
        model,
        totals: PiUsageTotals {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            total_cost,
        },
        total_tokens_for_dedupe,
    })
}

fn number_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| {
            value.as_f64().and_then(|number| {
                (number.is_finite()
                    && number >= 0.0
                    && number <= u64::MAX as f64
                    && number.fract() == 0.0)
                    .then_some(number as u64)
            })
        })
}

fn date_key_from_timestamp(timestamp: &str) -> Option<String> {
    let date_time = DateTime::parse_from_rfc3339(timestamp).ok()?;
    Some(date_time.date_naive().format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "pi-harness-usage-tests-{}-{}",
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
    fn usage_entry_matches_expected_shape() {
        let value = serde_json::json!({
            "type": "message",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": {
                "role": "assistant",
                "model": "claude-opus-4-5",
                "usage": {
                    "input": 100,
                    "output": 50,
                    "cacheRead": 10,
                    "cacheWrite": 20,
                    "totalTokens": 180,
                    "cost": { "total": 0.05 }
                }
            }
        });

        let entry = usage_entry_from_value(&value).unwrap();

        assert_eq!(entry.date, "2026-01-01");
        assert_eq!(entry.model.as_deref(), Some("[pi] claude-opus-4-5"));
        assert_eq!(entry.totals.input_tokens, 100);
        assert_eq!(entry.totals.output_tokens, 50);
        assert_eq!(entry.totals.cache_read_tokens, 10);
        assert_eq!(entry.totals.cache_creation_tokens, 20);
        assert_eq!(entry.total_tokens_for_dedupe, 180);
        assert_eq!(entry.totals.total_cost, 0.05);
    }

    #[test]
    fn usage_entry_rejects_non_assistant_or_missing_input_output() {
        let user = serde_json::json!({
            "type": "message",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": { "role": "user", "usage": { "input": 1, "output": 2 } }
        });
        let missing_output = serde_json::json!({
            "type": "message",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": { "role": "assistant", "usage": { "input": 1, "totalTokens": 1 } }
        });
        let tool_use = serde_json::json!({
            "type": "tool_use",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": { "role": "assistant", "usage": { "input": 1, "output": 2 } }
        });

        assert!(usage_entry_from_value(&user).is_none());
        assert!(usage_entry_from_value(&missing_output).is_none());
        assert!(usage_entry_from_value(&tool_use).is_none());
    }

    #[test]
    fn usage_entry_calculates_total_tokens_when_missing() {
        let value = serde_json::json!({
            "timestamp": "2026-01-01T00:00:01Z",
            "message": {
                "role": "assistant",
                "usage": { "input": 100, "output": 50, "cacheRead": 10, "cacheWrite": 20 }
            }
        });

        let entry = usage_entry_from_value(&value).unwrap();

        assert_eq!(entry.total_tokens_for_dedupe, 180);
    }

    #[test]
    fn load_usage_report_aggregates_by_day_model_and_dedupes() {
        let dir = TestDir::new();
        let project_dir = dir.path().join("--tmp-project--");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(
            project_dir.join("session.jsonl"),
            concat!(
                "{\"type\":\"session\",\"id\":\"s1\",\"cwd\":\"/tmp\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2026-01-02T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-sonnet-4\",\"usage\":{\"input\":100,\"output\":50,\"cacheRead\":10,\"cacheWrite\":20,\"totalTokens\":180,\"cost\":{\"total\":0.05}}}}\n",
                "{\"type\":\"message\",\"timestamp\":\"2026-01-02T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-sonnet-4\",\"usage\":{\"input\":100,\"output\":50,\"cacheRead\":10,\"cacheWrite\":20,\"totalTokens\":180,\"cost\":{\"total\":0.05}}}}\n",
                "{\"type\":\"message\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-5\",\"usage\":{\"input\":7,\"output\":8,\"cost\":{\"total\":0.01}}}}\n"
            ),
        )
        .unwrap();

        let report = load_usage_report_from_path(dir.path());

        assert_eq!(report.files_scanned, 1);
        assert_eq!(report.entries, 2);
        assert_eq!(report.skipped_duplicates, 1);
        assert_eq!(report.totals.input_tokens, 107);
        assert_eq!(report.totals.output_tokens, 58);
        assert_eq!(report.totals.cache_read_tokens, 10);
        assert_eq!(report.totals.cache_creation_tokens, 20);
        assert!((report.totals.total_cost - 0.06).abs() < f64::EPSILON);
        assert_eq!(report.days.len(), 2);
        assert_eq!(report.days[0].date, "2026-01-02");
        assert_eq!(report.days[0].models_used, vec!["[pi] claude-sonnet-4"]);
        assert_eq!(report.days[1].date, "2026-01-01");
    }
}
