#[path = "util/paths.rs"]
mod paths;
#[path = "util/text.rs"]
mod text;
#[path = "util/time.rs"]
mod time;

pub use paths::{app_config_dir, app_runtime_dir, app_state_dir, normalize_project_path};
pub use text::{
    is_default_session_name, project_name_from_path, session_name_from_text, truncate_text,
};
pub use time::now_millis;
