use super::error::GResult;
use async_trait::async_trait;
use greentic_types::TenantCtx;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SpanContext {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

#[async_trait]
pub trait SecretsHost: Send + Sync {
    async fn get(&self, name: &str) -> GResult<String>;
}

#[async_trait]
pub trait TelemetryHost: Send + Sync {
    async fn emit(&self, span: &SpanContext, fields: &[(&str, &str)]) -> GResult<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub tenant_key: String,
    pub flow_id: String,
    pub session_hint: Option<String>,
}

impl SessionKey {
    pub fn new(tenant: &TenantCtx, flow_id: &str, session_hint: Option<String>) -> Self {
        let tenant_key = format!("{}::{}", tenant.env.as_str(), tenant.tenant.as_str());
        Self {
            tenant_key,
            flow_id: flow_id.to_string(),
            session_hint,
        }
    }

    pub fn stable_session_id(&self) -> Option<String> {
        self.session_hint.clone()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SessionCursor {
    pub position: usize,
    pub outbox_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionOutboxEntry {
    pub seq: u64,
    pub hash: String,
    pub response: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WaitState {
    pub reason: String,
    pub recorded_at: SystemTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionSnapshot {
    pub key: SessionKey,
    pub session_id: String,
    pub revision: u64,
    pub cursor: SessionCursor,
    pub state: Value,
    pub outbox: HashMap<OutboxKey, SessionOutboxEntry>,
    pub waiting: Option<WaitState>,
    pub last_outcome: Option<Value>,
    pub ttl: Duration,
}

impl SessionSnapshot {
    pub fn new(key: SessionKey, session_id: String) -> Self {
        Self {
            key,
            session_id,
            revision: 0,
            cursor: SessionCursor::default(),
            state: Value::Object(Default::default()),
            outbox: HashMap::new(),
            waiting: None,
            last_outcome: None,
            ttl: Duration::from_secs(3600),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxKey {
    seq: u64,
    hash: String,
}

impl OutboxKey {
    pub fn new(seq: u64, hash: String) -> Self {
        Self { seq, hash }
    }
}

impl Hash for OutboxKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.seq.hash(state);
        self.hash.hash(state);
    }
}

#[async_trait]
pub trait SessionHost: Send + Sync {
    async fn get(&self, key: &SessionKey) -> GResult<Option<SessionSnapshot>>;
    async fn put(&self, snapshot: SessionSnapshot) -> GResult<()>;
    async fn update_cas(&self, snapshot: SessionSnapshot, expected_revision: u64) -> GResult<bool>;
    async fn delete(&self, key: &SessionKey) -> GResult<()>;
    async fn touch(&self, key: &SessionKey, ttl: Duration) -> GResult<()>;
}

#[async_trait]
pub trait StateHost: Send + Sync {
    async fn get_json(&self, key: &SessionKey) -> GResult<Option<Value>>;
    async fn set_json(&self, key: &SessionKey, value: Value) -> GResult<()>;
    async fn del(&self, key: &SessionKey) -> GResult<()>;
    async fn del_prefix(&self, key_prefix: &str) -> GResult<()>;
}

pub struct HostBundle {
    pub secrets: Arc<dyn SecretsHost>,
    pub telemetry: Arc<dyn TelemetryHost>,
    pub session: Arc<dyn SessionHost>,
    pub state: Arc<dyn StateHost>,
}

impl HostBundle {
    pub fn new(
        secrets: Arc<dyn SecretsHost>,
        telemetry: Arc<dyn TelemetryHost>,
        session: Arc<dyn SessionHost>,
        state: Arc<dyn StateHost>,
    ) -> Self {
        Self {
            secrets,
            telemetry,
            session,
            state,
        }
    }
}
