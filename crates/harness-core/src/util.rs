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

/// Rename the current process via `prctl(PR_SET_NAME)` so `ps`, `top`, and
/// `htop` show `amux-{adapter}` instead of the raw binary path. Best-effort:
/// failures (unsupported platform, name truncation is handled by the kernel)
/// are logged but never fatal, since the title is cosmetic.
pub fn set_process_name(name: &str) {
    #[cfg(unix)]
    {
        match std::ffi::CString::new(name) {
            Ok(cname) => {
                // SAFETY: `prctl(PR_SET_NAME)` copies at most 16 bytes out of
                // the passed pointer during the call and retains nothing.
                let rc = unsafe { libc::prctl(libc::PR_SET_NAME, cname.as_ptr()) };
                if rc != 0 {
                    log::debug!(
                        "prctl(PR_SET_NAME, {name}) failed: {}",
                        std::io::Error::last_os_error()
                    );
                }
            }
            Err(error) => {
                log::debug!("process name {name:?} is not a valid CString: {error}");
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = name;
    }
}
