use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;

use super::files::sessions_root;

pub use super::usage_types::{PiUsageDay, PiUsageModelBreakdown, PiUsageReport, PiUsageTotals};

#[derive(Clone, Debug, PartialEq)]
struct UsageEntry {
    timestamp_ms: i64,
    date: String,
    model: Option<String>,
    totals: PiUsageTotals,
    total_tokens_for_dedupe: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileCacheKey {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone)]
struct CachedUsageEntries {
    key: FileCacheKey,
    entries: Arc<[UsageEntry]>,
}

static USAGE_ENTRIES_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedUsageEntries>>> = OnceLock::new();

fn usage_entries_cache() -> &'static Mutex<HashMap<PathBuf, CachedUsageEntries>> {
    USAGE_ENTRIES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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
        let entries = usage_entries_from_file(&file);
        for entry in entries.iter() {
            if !processed_hashes.insert((entry.timestamp_ms, entry.total_tokens_for_dedupe)) {
                report.skipped_duplicates = report.skipped_duplicates.saturating_add(1);
                continue;
            }

            report.entries = report.entries.saturating_add(1);
            report.totals.add(&entry.totals);

            let day = by_day.entry(entry.date.clone()).or_default();
            day.totals.add(&entry.totals);
            let model_name = entry.model.clone().unwrap_or_else(|| "unknown".to_string());
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
    sessions_root().filter(|path| path.is_dir())
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

fn file_cache_key(path: &Path) -> Option<FileCacheKey> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileCacheKey {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn usage_entries_from_file(path: &Path) -> Arc<[UsageEntry]> {
    let cache_key = file_cache_key(path);
    if let Some(cache_key) = cache_key.as_ref() {
        if let Some(cached) = usage_entries_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .filter(|cached| cached.key == *cache_key)
            .cloned()
        {
            return cached.entries;
        }
    }

    let entries = parse_usage_entries_from_file(path);
    let entries: Arc<[UsageEntry]> = entries.into();
    if let Some(cache_key) = cache_key {
        usage_entries_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                path.to_path_buf(),
                CachedUsageEntries {
                    key: cache_key,
                    entries: entries.clone(),
                },
            );
    }
    entries
}

fn parse_usage_entries_from_file(path: &Path) -> Vec<UsageEntry> {
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
    usage_entry_from_value_in_timezone(value, &Local)
}

fn usage_entry_from_value_in_timezone<Tz: TimeZone>(
    value: &Value,
    time_zone: &Tz,
) -> Option<UsageEntry> {
    let kind = value.get("type").and_then(Value::as_str);
    if kind.is_some_and(|kind| kind != "message") {
        return None;
    }

    let timestamp = value.get("timestamp")?.as_str()?;
    let date_time = DateTime::parse_from_rfc3339(timestamp).ok()?;
    let date = date_time
        .with_timezone(time_zone)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
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
        .map(str::to_string);

    Some(UsageEntry {
        timestamp_ms: date_time.timestamp_millis(),
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
