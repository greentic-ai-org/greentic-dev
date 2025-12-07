use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use greentic_state::inmemory::InMemoryStateStore;
use greentic_state::{StateKey as StoreStateKey, StateStore};
use greentic_types::{EnvId, TenantCtx, TenantId};
use serde_json::Value;

use crate::engine::error::{GResult, RunnerError};
use crate::engine::host::{SessionKey, StateHost};

pub type DynStateStore = Arc<dyn StateStore>;

pub(crate) const STATE_PREFIX: &str = "runner";

pub struct StateStoreHost {
    store: DynStateStore,
}

impl StateStoreHost {
    pub fn new(store: DynStateStore) -> Self {
        Self { store }
    }
}

pub fn new_state_store() -> DynStateStore {
    Arc::new(InMemoryStateStore::new())
}

pub fn state_host_from(store: DynStateStore) -> Arc<dyn StateHost> {
    Arc::new(StateStoreHost::new(store))
}

#[async_trait]
impl StateHost for StateStoreHost {
    async fn get_json(&self, key: &SessionKey) -> GResult<Option<Value>> {
        let tenant = tenant_ctx_from_key(key)?;
        let state_key = derive_state_key(key);
        self.store
            .get_json(&tenant, STATE_PREFIX, &state_key, None)
            .map_err(map_state_error)
    }

    async fn set_json(&self, key: &SessionKey, value: Value) -> GResult<()> {
        let tenant = tenant_ctx_from_key(key)?;
        let state_key = derive_state_key(key);
        self.store
            .set_json(&tenant, STATE_PREFIX, &state_key, None, &value, None)
            .map_err(map_state_error)
    }

    async fn del(&self, key: &SessionKey) -> GResult<()> {
        let tenant = tenant_ctx_from_key(key)?;
        let state_key = derive_state_key(key);
        self.store
            .del(&tenant, STATE_PREFIX, &state_key)
            .map_err(map_state_error)?;
        Ok(())
    }

    async fn del_prefix(&self, _key_prefix: &str) -> GResult<()> {
        // Prefix deletions are not currently used; provide a no-op implementation.
        Ok(())
    }
}

fn tenant_ctx_from_key(key: &SessionKey) -> GResult<TenantCtx> {
    let (env, tenant) = key
        .tenant_key
        .split_once("::")
        .ok_or_else(|| RunnerError::State {
            reason: format!("invalid tenant descriptor '{}'", key.tenant_key),
        })?;
    let env_id = EnvId::from_str(env).map_err(|err| RunnerError::State {
        reason: format!("invalid env id {env}: {err}"),
    })?;
    let tenant_id = TenantId::from_str(tenant).map_err(|err| RunnerError::State {
        reason: format!("invalid tenant id {tenant}: {err}"),
    })?;
    Ok(TenantCtx::new(env_id, tenant_id))
}

fn derive_state_key(key: &SessionKey) -> StoreStateKey {
    let hint = key.session_hint.as_deref().unwrap_or("-");
    StoreStateKey::from(format!("{}::{hint}", key.flow_id))
}

fn map_state_error(err: greentic_types::GreenticError) -> RunnerError {
    RunnerError::State {
        reason: err.to_string(),
    }
}
