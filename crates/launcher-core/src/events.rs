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
}
impl Default for EventBus {
    fn default() -> Self {
        Self::new(|_, _| {})
    }
}
