//! fx-sidecar wire types. fx has no extension host yet, so no process ever
//! emits these snapshots today; the types exist because harness-core's
//! sidecar stream compiles against them. Copy of the pi wire schema.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PiSessionStage {
    Idle,
    Thinking,
    Outputting,
    Tool,
}

impl PiSessionStage {
    pub fn as_runtime_status(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Thinking => Some("thinking"),
            Self::Outputting => Some("outputting"),
            Self::Tool => Some("tool"),
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiSidecarSnapshot {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub harness_session_id: Option<String>,
    #[serde(default)]
    pub session_file: Option<PathBuf>,
    #[serde(default)]
    pub parent_session_file: Option<PathBuf>,
    #[serde(default)]
    pub session_name: Option<String>,
    pub stage: PiSessionStage,
    #[serde(default)]
    pub queued: bool,
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub ts_ms: u64,
}

impl PiSidecarSnapshot {
    pub fn is_valid(&self) -> bool {
        self.kind.as_deref().is_none_or(|kind| kind == "snapshot") && !self.session_id.is_empty()
    }
}
