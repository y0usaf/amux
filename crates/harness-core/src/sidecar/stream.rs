use super::SidecarMessage;
use crate::agent::PiSidecarSnapshot;
use crate::notify::Notify;
use crate::render::Color;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

pub(super) fn read_sidecar_stream(
    stream: UnixStream,
    tx: mpsc::Sender<SidecarMessage>,
    raw_tx: Option<mpsc::Sender<String>>,
    notify: Notify,
) {
    let reader = BufReader::new(stream);
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((message, raw)) = parse_sidecar_line(trimmed) else {
            continue;
        };
        let _ = tx.send(message);
        if let Some(raw_tx) = &raw_tx {
            let _ = raw_tx.send(raw);
        }
        notify();
    }
}

/// Parse one validated sidecar NDJSON line into its message and raw form.
/// Returns `None` for empty or invalid lines (logged, never fatal).
pub fn parse_sidecar_line(trimmed: &str) -> Option<(SidecarMessage, String)> {
    let value = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => value,
        Err(_) => {
            log::warn!("malformed sidecar line: {trimmed}");
            return None;
        }
    };
    if value.get("type").and_then(|v| v.as_str()) == Some("theme") {
        let Some(roles) = value.get("roles").and_then(|v| v.as_array()) else {
            log::warn!("malformed theme sidecar line: {trimmed}");
            return None;
        };
        if roles.len() != 15 {
            log::warn!("malformed theme sidecar line (expected 15 roles): {trimmed}");
            return None;
        }
        let mut out = [Color::rgba(0, 0, 0, 0); 15];
        let mut valid = true;
        for (i, r) in roles.iter().enumerate() {
            out[i] = match r.get("kind").and_then(|v| v.as_str()) {
                Some("default") => Some(Color::rgba(0, 0, 0, 0)),
                Some("ansi") => r
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .filter(|n| *n <= 255)
                    .map(|n| Color::ansi_index(n as u8)),
                Some("rgb") => r
                    .get("r")
                    .and_then(|v| v.as_u64())
                    .zip(r.get("g").and_then(|v| v.as_u64()))
                    .zip(r.get("b").and_then(|v| v.as_u64()))
                    .filter(|((r, g), b)| *r <= 255 && *g <= 255 && *b <= 255)
                    .map(|((r, g), b)| Color::rgb(r as u8, g as u8, b as u8)),
                _ => None,
            }
            .unwrap_or_else(|| {
                valid = false;
                Color::rgba(0, 0, 0, 0)
            });
        }
        if !valid {
            log::warn!("malformed theme sidecar line: {trimmed}");
            return None;
        }
        return Some((SidecarMessage::Theme(out), trimmed.to_string()));
    }
    let Ok(snapshot) = serde_json::from_value::<PiSidecarSnapshot>(value) else {
        return None;
    };
    if !snapshot.is_valid() {
        return None;
    }
    Some((SidecarMessage::Snapshot(snapshot), trimmed.to_string()))
}
