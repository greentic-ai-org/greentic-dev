use super::error::GResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterCall {
    pub adapter: String,
    pub operation: String,
    pub payload: Value,
}

#[async_trait]
pub trait Adapter: Send + Sync {
    async fn call(&self, call: &AdapterCall) -> GResult<Value>;
}

#[derive(Default, Clone)]
pub struct AdapterRegistry {
    inner: HashMap<String, Arc<dyn Adapter>>,
}

impl AdapterRegistry {
    pub fn register(&mut self, name: impl Into<String>, adapter: Box<dyn Adapter>) {
        self.inner.insert(name.into(), Arc::from(adapter));
    }

    pub fn register_arc(&mut self, name: impl Into<String>, adapter: Arc<dyn Adapter>) {
        self.inner.insert(name.into(), adapter);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        self.inner.get(name).cloned()
    }
}
