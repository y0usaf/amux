use std::sync::Arc;

pub type Notify = Arc<dyn Fn() + Send + Sync + 'static>;

pub fn noop() -> Notify {
    Arc::new(|| {})
}
