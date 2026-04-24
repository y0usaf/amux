use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::state::ScannedSession;
use crate::util::{is_default_session_name, now_millis};

#[derive(Clone, Debug, Default)]
pub struct SessionRuntime {
    pub running: bool,
    pub status: Option<String>,
    pub queued: bool,
    pub tool_name: Option<String>,
    pub unread: bool,
    pub last_sidecar_ts_ms: u64,
}

impl SessionRuntime {
    pub fn is_active(&self) -> bool {
        self.running || self.queued
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub local_id: String,
    pub name: String,
    pub pi_session_id: Option<String>,
    pub session_file: Option<PathBuf>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub promoted_at_ms: u64,
    pub runtime: SessionRuntime,
    pub draft: bool,
}

impl Session {
    pub fn from_scan(scan: ScannedSession) -> Self {
        Self {
            local_id: scan.session_id.clone(),
            name: scan.name,
            pi_session_id: Some(scan.session_id),
            session_file: Some(scan.session_file),
            created_at_ms: scan.created_at_ms,
            updated_at_ms: scan.updated_at_ms,
            promoted_at_ms: 0,
            runtime: SessionRuntime::default(),
            draft: false,
        }
    }

    pub fn new_draft() -> Self {
        let now = now_millis();
        Self {
            local_id: Uuid::new_v4().to_string(),
            name: "Session".to_string(),
            pi_session_id: None,
            session_file: None,
            created_at_ms: now,
            updated_at_ms: now,
            promoted_at_ms: 0,
            runtime: SessionRuntime::default(),
            draft: true,
        }
    }

    pub fn selection_key(&self) -> String {
        self.persisted_selection_key()
            .unwrap_or_else(|| self.local_id.clone())
    }

    pub fn persisted_selection_key(&self) -> Option<String> {
        self.pi_session_id.clone().or_else(|| {
            self.session_file
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
        })
    }

    pub fn apply_scan(&mut self, scan: ScannedSession) {
        if self.should_adopt_name(&scan.name) {
            self.name = scan.name.clone();
        }
        self.pi_session_id = Some(scan.session_id);
        self.session_file = Some(scan.session_file);
        self.created_at_ms = self.created_at_ms.min(scan.created_at_ms);
        self.updated_at_ms = self.updated_at_ms.max(scan.updated_at_ms);
        self.draft = false;
    }

    pub fn touch_at(&mut self, timestamp_ms: u64) {
        self.updated_at_ms = self.updated_at_ms.max(timestamp_ms);
    }

    pub fn promote_at(&mut self, timestamp_ms: u64) {
        self.touch_at(timestamp_ms);
        self.promoted_at_ms = self.promoted_at_ms.max(self.updated_at_ms);
    }

    pub fn has_materialized_session_file(&self) -> bool {
        self.session_file.as_ref().is_some_and(|path| path.exists())
    }

    pub fn is_ephemeral_draft(&self) -> bool {
        self.draft
            && self.pi_session_id.is_none()
            && self.session_file.is_none()
            && !self.runtime.is_active()
            && self.runtime.status.is_none()
            && !self.runtime.unread
    }

    pub fn should_render_in_sidebar(&self) -> bool {
        !self.is_ephemeral_draft()
    }

    pub fn counts_for_activity_ordering(&self) -> bool {
        !self.is_ephemeral_draft()
    }

    pub fn matches_scan(&self, scan: &ScannedSession) -> bool {
        self.pi_session_id.as_deref() == Some(scan.session_id.as_str())
            || self.session_file.as_ref() == Some(&scan.session_file)
    }

    pub fn matches_identity(
        &self,
        harness_session_id: Option<&str>,
        pi_session_id: &str,
        session_file: Option<&Path>,
    ) -> bool {
        harness_session_id.is_some_and(|id| id == self.local_id)
            || self.pi_session_id.as_deref() == Some(pi_session_id)
            || session_file.is_some_and(|path| self.session_file.as_deref() == Some(path))
    }

    pub fn should_adopt_name(&self, next_name: &str) -> bool {
        let next_name = next_name.trim();
        !next_name.is_empty()
            && (self.name.trim().is_empty()
                || is_default_session_name(&self.name)
                || self.name.trim() == next_name)
    }
}
