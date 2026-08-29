//! fx usage reader. fx appends per-generation facts to `$HOME/.fx/usage.jsonl`:
//! `{"kind":"generation","fact":{"model","input_tokens","output_tokens",
//! "cache_read_tokens","cache_write_tokens","total_cost","created_at_ms"}}`.
//! Aggregate them into the same day/model report shape pi's usage path emits.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use chrono::{Local, TimeZone};
use serde_json::Value;

use crate::agent::usage_types::{
    PiUsageDay, PiUsageModelBreakdown, PiUsageReport, PiUsageTotals,
};
use super::files::sessions_root;

pub fn load_usage_report() -> PiUsageReport {
    let Some(path) = default_usage_path() else {
        return Default::default();
    };
    load_usage_report_from_path(&path)
}

pub fn load_usage_report_from_path(path: &std::path::Path) -> PiUsageReport {
    let file = fs::File::open(path);
    let Ok(file) = file else {
        return Default::default();
    };

    let mut report = PiUsageReport {
        files_scanned: 1,
        ..Default::default()
    };
    let mut by_day: BTreeMap<String, PiUsageTotals> = BTreeMap::new();
    let mut models_by_day: BTreeMap<String, BTreeMap<String, PiUsageTotals>> =
        BTreeMap::new();
    let mut seen = std::collections::HashSet::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("kind").and_then(Value::as_str) != Some("generation") {
            continue;
        }
        let Some(fact) = value.get("fact") else {
            continue;
        };
        let Some(created_at_ms) = fact.get("created_at_ms").and_then(Value::as_i64) else {
            continue;
        };
        let totals = PiUsageTotals {
            input_tokens: fact.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
            output_tokens: fact.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
            cache_creation_tokens: fact
                .get("cache_write_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_read_tokens: fact
                .get("cache_read_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            total_cost: fact.get("total_cost").and_then(Value::as_f64).unwrap_or(0.0),
        };
        // fx logs each generation once; the identity key still guards against
        // a duplicated line from a partial append + rewrite.
        if !seen.insert((created_at_ms, fact.to_string())) {
            report.skipped_duplicates = report.skipped_duplicates.saturating_add(1);
            continue;
        }

        report.entries = report.entries.saturating_add(1);
        report.totals.add(&totals);
        let date = local_date(&created_at_ms);
        by_day.entry(date.clone()).or_default().add(&totals);
        let model = fact
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        models_by_day
            .entry(date)
            .or_default()
            .entry(model)
            .or_default()
            .add(&totals);
    }

    report.days = by_day
        .into_iter()
        .map(|(date, totals)| {
            let models = models_by_day.remove(&date).unwrap_or_default();
            let model_breakdowns = models
                .into_iter()
                .map(|(model_name, totals)| PiUsageModelBreakdown {
                    model_name,
                    totals,
                })
                .collect::<Vec<_>>();
            let models_used = model_breakdowns
                .iter()
                .map(|breakdown| breakdown.model_name.clone())
                .collect();
            PiUsageDay {
                date,
                totals,
                models_used,
                model_breakdowns,
            }
        })
        .collect();
    report.days.sort_by(|a, b| b.date.cmp(&a.date));
    report
}

fn default_usage_path() -> Option<PathBuf> {
    // fx keeps usage next to the sessions store, not inside it.
    sessions_root()?.parent().map(|dir| dir.join("usage.jsonl"))
}

fn local_date(created_at_ms: &i64) -> String {
    Local
        .timestamp_millis_opt(*created_at_ms)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn aggregates_generation_facts_by_day_and_model() {
        let path = std::env::temp_dir().join(format!("amux-fx-usage-{}.jsonl", std::process::id()));
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"schema_version":1,"kind":"generation","fact":{{"id":"g1","created_at_ms":1787341522308,"model":"zai/glm-5.2","input_tokens":100,"output_tokens":10,"cache_read_tokens":5,"cache_write_tokens":1,"total_cost":0.5}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"schema_version":1,"kind":"coverage","started_at_ms":1}}"#
        )
        .unwrap();
        drop(file);

        let report = load_usage_report_from_path(&path);
        assert_eq!(report.entries, 1);
        assert_eq!(report.totals.input_tokens, 100);
        assert_eq!(report.totals.cache_creation_tokens, 1);
        assert_eq!(report.days.len(), 1);
        assert_eq!(report.days[0].model_breakdowns[0].model_name, "zai/glm-5.2");

        let _ = fs::remove_file(&path);
    }
}
