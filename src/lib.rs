#[path = "app/mod.rs"]
pub mod app;
pub mod config;
pub mod notify;
pub mod pi;
#[path = "render/mod.rs"]
pub mod render;
pub mod sidecar;
pub mod state;
pub mod terminal;
pub mod util;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        match ENV_LOCK.get_or_init(|| Mutex::new(())).lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
