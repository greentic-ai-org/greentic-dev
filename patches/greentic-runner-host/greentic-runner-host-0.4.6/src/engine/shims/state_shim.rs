use super::super::error::GResult;
use super::super::host::{SessionKey, StateHost};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
pub struct InMemoryStateHost {
    store: RwLock<HashMap<String, Value>>,
}

impl InMemoryStateHost {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    fn key_of(session_key: &SessionKey) -> String {
        format!("{}:{}", session_key.tenant_key, session_key.flow_id)
    }
}

#[async_trait]
impl StateHost for InMemoryStateHost {
    async fn get_json(&self, key: &SessionKey) -> GResult<Option<Value>> {
        Ok(self.store.read().get(&Self::key_of(key)).cloned())
    }

    async fn set_json(&self, key: &SessionKey, value: Value) -> GResult<()> {
        self.store.write().insert(Self::key_of(key), value);
        Ok(())
    }

    async fn del(&self, key: &SessionKey) -> GResult<()> {
        self.store.write().remove(&Self::key_of(key));
        Ok(())
    }

    async fn del_prefix(&self, key_prefix: &str) -> GResult<()> {
        let mut guard = self.store.write();
        let keys: Vec<String> = guard
            .keys()
            .filter(|k| k.starts_with(key_prefix))
            .cloned()
            .collect();
        for key in keys {
            guard.remove(&key);
        }
        Ok(())
    }
}
