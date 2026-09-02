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
    /// Present only for subagent sessions: the parent session's file path.
    /// Subagent runners inherit the parent's harness env, so the harness must
    /// route them by this link instead of by `harness_session_id`.
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
