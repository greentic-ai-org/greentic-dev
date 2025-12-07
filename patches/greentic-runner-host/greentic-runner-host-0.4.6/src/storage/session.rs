use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use greentic_session::inmemory::InMemorySessionStore;
use greentic_session::{SessionData, SessionKey as StoreSessionKey, SessionStore};
use greentic_types::{
    EnvId, FlowId, GreenticError, SessionCursor as TypesSessionCursor, TenantCtx, TenantId, UserId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::engine::error::{GResult, RunnerError};
use crate::engine::host::{
    OutboxKey, SessionCursor, SessionHost, SessionKey, SessionOutboxEntry, SessionSnapshot,
    WaitState,
};

pub type DynSessionStore = Arc<dyn SessionStore>;

/// Adapter that backs the runner session host with a greentic-session store.
pub struct SessionStoreHost {
    store: DynSessionStore,
}

impl SessionStoreHost {
    pub fn new(store: DynSessionStore) -> Self {
        Self { store }
    }

    fn lookup_entry(&self, key: &SessionKey) -> GResult<Option<StoreEntry>> {
        let base_ctx = tenant_ctx_from_key(key)?;
        let user = user_id_from_key(key)?;
        let ctx = base_ctx.clone().with_user(Some(user.clone()));
        let result = self
            .store
            .find_by_user(&ctx, &user)
            .map_err(map_store_error)?;
        if let Some((store_key, data)) = result {
            let snapshot = decode_snapshot(&data)?;
            if snapshot.key.tenant_key != key.tenant_key
                || snapshot.key.flow_id != key.flow_id
                || snapshot.key.session_hint != key.session_hint
            {
                return Ok(None);
            }
            Ok(Some(StoreEntry {
                key: store_key,
                snapshot,
                ctx,
                user,
            }))
        } else {
            Ok(None)
        }
    }

    fn upsert(
        &self,
        snapshot: &SessionSnapshot,
        ctx: TenantCtx,
        user: &UserId,
    ) -> GResult<StoreSessionKey> {
        let data = encode_snapshot(snapshot, ctx.clone(), user)?;
        match self
            .store
            .find_by_user(&ctx, user)
            .map_err(map_store_error)?
        {
            Some((store_key, _)) => {
                self.store
                    .update_session(&store_key, data)
                    .map_err(map_store_error)?;
                Ok(store_key)
            }
            None => self
                .store
                .create_session(&ctx, data)
                .map_err(map_store_error),
        }
    }
}

struct StoreEntry {
    key: StoreSessionKey,
    snapshot: SessionSnapshot,
    ctx: TenantCtx,
    user: UserId,
}

pub fn new_session_store() -> DynSessionStore {
    Arc::new(InMemorySessionStore::new())
}

pub fn session_host_from(store: DynSessionStore) -> Arc<dyn SessionHost> {
    Arc::new(SessionStoreHost::new(store))
}

#[async_trait]
impl SessionHost for SessionStoreHost {
    async fn get(&self, key: &SessionKey) -> GResult<Option<SessionSnapshot>> {
        Ok(self.lookup_entry(key)?.map(|entry| entry.snapshot))
    }

    async fn put(&self, snapshot: SessionSnapshot) -> GResult<()> {
        let base_ctx = tenant_ctx_from_key(&snapshot.key)?;
        let user = user_id_from_key(&snapshot.key)?;
        let ctx = base_ctx.with_user(Some(user.clone()));
        self.upsert(&snapshot, ctx, &user)?;
        Ok(())
    }

    async fn update_cas(
        &self,
        mut snapshot: SessionSnapshot,
        expected_revision: u64,
    ) -> GResult<bool> {
        let Some(entry) = self.lookup_entry(&snapshot.key)? else {
            return Ok(false);
        };
        if entry.snapshot.revision != expected_revision {
            return Ok(false);
        }
        snapshot.revision = expected_revision.saturating_add(1);
        self.upsert(&snapshot, entry.ctx, &entry.user)?;
        Ok(true)
    }

    async fn delete(&self, key: &SessionKey) -> GResult<()> {
        if let Some(entry) = self.lookup_entry(key)? {
            self.store
                .remove_session(&entry.key)
                .map_err(map_store_error)?;
        }
        Ok(())
    }

    async fn touch(&self, key: &SessionKey, ttl: Duration) -> GResult<()> {
        if let Some(mut entry) = self.lookup_entry(key)? {
            entry.snapshot.ttl = ttl;
            self.upsert(&entry.snapshot, entry.ctx, &entry.user)?;
        }
        Ok(())
    }
}

fn encode_snapshot(
    snapshot: &SessionSnapshot,
    mut ctx: TenantCtx,
    user: &UserId,
) -> GResult<SessionData> {
    let flow_id = FlowId::from_str(snapshot.key.flow_id.as_str()).map_err(map_store_error)?;
    ctx = ctx
        .with_flow(snapshot.key.flow_id.clone())
        .with_session(snapshot.session_id.clone());
    let cursor = TypesSessionCursor {
        node_pointer: format!("pos-{}", snapshot.cursor.position),
        wait_reason: snapshot.waiting.as_ref().map(|wait| wait.reason.clone()),
        outbox_marker: Some(snapshot.cursor.outbox_seq.to_string()),
    };
    let payload = PersistedSnapshot::from(snapshot);
    let context_json = serde_json::to_string(&payload).map_err(|err| RunnerError::Session {
        reason: format!("failed to encode session snapshot: {err}"),
    })?;
    Ok(SessionData {
        tenant_ctx: ctx.with_user(Some(user.clone())),
        flow_id,
        cursor,
        context_json,
    })
}

fn decode_snapshot(data: &SessionData) -> GResult<SessionSnapshot> {
    let stored: PersistedSnapshot =
        serde_json::from_str(&data.context_json).map_err(|err| RunnerError::Session {
            reason: format!("failed to decode session snapshot: {err}"),
        })?;
    Ok(stored.into())
}

fn tenant_ctx_from_key(key: &SessionKey) -> GResult<TenantCtx> {
    let (env, tenant) = key
        .tenant_key
        .split_once("::")
        .ok_or_else(|| RunnerError::Session {
            reason: format!("invalid tenant descriptor '{}'", key.tenant_key),
        })?;
    let env_id = EnvId::from_str(env).map_err(map_store_error)?;
    let tenant_id = TenantId::from_str(tenant).map_err(map_store_error)?;
    Ok(TenantCtx::new(env_id, tenant_id))
}

fn user_id_from_key(key: &SessionKey) -> GResult<UserId> {
    let hint = key
        .session_hint
        .clone()
        .unwrap_or_else(|| format!("{}::{}", key.tenant_key, key.flow_id));
    let digest = Sha256::digest(hint.as_bytes());
    let slug = format!("sess{}", hex::encode(&digest[..8]));
    UserId::from_str(&slug).map_err(map_store_error)
}

fn map_store_error(err: GreenticError) -> RunnerError {
    RunnerError::Session {
        reason: err.to_string(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedSnapshot {
    key: SessionKey,
    session_id: String,
    revision: u64,
    cursor: SessionCursor,
    state: serde_json::Value,
    #[serde(default)]
    outbox: Vec<PersistedOutboxEntry>,
    waiting: Option<WaitState>,
    last_outcome: Option<serde_json::Value>,
    ttl_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedOutboxEntry {
    key: OutboxKey,
    entry: SessionOutboxEntry,
}

impl From<&SessionSnapshot> for PersistedSnapshot {
    fn from(snapshot: &SessionSnapshot) -> Self {
        Self {
            key: snapshot.key.clone(),
            session_id: snapshot.session_id.clone(),
            revision: snapshot.revision,
            cursor: snapshot.cursor.clone(),
            state: snapshot.state.clone(),
            outbox: snapshot
                .outbox
                .iter()
                .map(|(key, entry)| PersistedOutboxEntry {
                    key: key.clone(),
                    entry: entry.clone(),
                })
                .collect(),
            waiting: snapshot.waiting.clone(),
            last_outcome: snapshot.last_outcome.clone(),
            ttl_secs: snapshot.ttl.as_secs(),
        }
    }
}

impl From<PersistedSnapshot> for SessionSnapshot {
    fn from(stored: PersistedSnapshot) -> Self {
        let mut outbox = HashMap::new();
        for entry in stored.outbox {
            outbox.insert(entry.key, entry.entry);
        }
        SessionSnapshot {
            key: stored.key,
            session_id: stored.session_id,
            revision: stored.revision,
            cursor: stored.cursor,
            state: stored.state,
            outbox,
            waiting: stored.waiting,
            last_outcome: stored.last_outcome,
            ttl: Duration::from_secs(stored.ttl_secs),
        }
    }
}
