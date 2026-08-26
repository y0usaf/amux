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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_stage_reports_runtime_status_and_activity() {
        assert_eq!(PiSessionStage::Idle.as_runtime_status(), None);
        assert_eq!(
            PiSessionStage::Thinking.as_runtime_status(),
            Some("thinking")
        );
        assert_eq!(
            PiSessionStage::Outputting.as_runtime_status(),
            Some("outputting")
        );
        assert_eq!(PiSessionStage::Tool.as_runtime_status(), Some("tool"));
        assert!(!PiSessionStage::Idle.is_active());
        assert!(PiSessionStage::Thinking.is_active());
        assert!(PiSessionStage::Outputting.is_active());
        assert!(PiSessionStage::Tool.is_active());
    }

    #[test]
    fn sidecar_snapshot_validation_accepts_missing_or_snapshot_kind_only() {
        let base = PiSidecarSnapshot {
            kind: None,
            session_id: "session-1".into(),
            harness_session_id: None,
            session_file: None,
            session_name: None,
            stage: PiSessionStage::Idle,
            queued: false,
            interrupted: false,
            tool_name: None,
            ts_ms: 0,
        };
        assert!(base.is_valid());

        let with_snapshot_kind = PiSidecarSnapshot {
            kind: Some("snapshot".into()),
            ..base.clone()
        };
        assert!(with_snapshot_kind.is_valid());

        let with_other_kind = PiSidecarSnapshot {
            kind: Some("other".into()),
            ..base.clone()
        };
        assert!(!with_other_kind.is_valid());

        let with_empty_session = PiSidecarSnapshot {
            session_id: String::new(),
            ..base
        };
        assert!(!with_empty_session.is_valid());
    }
}
