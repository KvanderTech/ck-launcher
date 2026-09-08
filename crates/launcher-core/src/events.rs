use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
/// Only public DTOs are delivered to an adapter; tokens remain in the core.
#[derive(Clone)]
pub struct EventBus(Arc<dyn Fn(&str, Value) + Send + Sync>);
impl EventBus {
    pub fn new(callback: impl Fn(&str, Value) + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
    pub fn emit(&self, name: &str, event: impl Serialize) -> Result<(), serde_json::Error> {
        (self.0)(name, serde_json::to_value(event)?);
        Ok(())
    }
    pub fn progress(
        &self,
        stage: &str,
        message: &str,
        done: u64,
        total: u64,
        files: u64,
        file_total: u64,
    ) {
        let _ = self.emit(
            "launcher://content-progress",
            serde_json::json!({
                "stage": stage, "message": message,
                "completedBytes": done, "totalBytes": total,
                "completedFiles": files, "totalFiles": file_total,
            }),
        );
    }
}
impl Default for EventBus {
    fn default() -> Self {
        Self::new(|_, _| {})
    }
}
