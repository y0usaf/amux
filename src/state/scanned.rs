use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ScannedSession {
    pub session_id: String,
    pub session_file: PathBuf,
    pub cwd: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub name: String,
}
