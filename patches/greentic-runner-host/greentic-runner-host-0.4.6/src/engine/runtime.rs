use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use greentic_session::SessionData;
use greentic_types::{
    EnvId, FlowId, GreenticError, SessionCursor as TypesSessionCursor, TenantCtx, TenantId, UserId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::api::{RunFlowRequest, RunnerApi};
use super::builder::{Runner, RunnerBuilder};
use super::error::{GResult, RunnerError};
use super::glue::{FnSecretsHost, FnTelemetryHost};
use super::host::{HostBundle, SecretsHost, SessionHost, StateHost};
use super::policy::Policy;
use super::registry::{Adapter, AdapterCall, AdapterRegistry};
use super::shims::{InMemorySessionHost, InMemoryStateHost};
use super::state_machine::{FlowDefinition, FlowStep, PAYLOAD_FROM_LAST_INPUT};

use crate::config::{HostConfig, SecretsPolicy};
use crate::pack::FlowDescriptor;
use crate::runner::engine::{FlowContext, FlowEngine, FlowSnapshot, FlowStatus, FlowWait};
use crate::runner::mocks::MockLayer;
use crate::secrets::DynSecretsManager;
use crate::storage::session::DynSessionStore;

const DEFAULT_ENV: &str = "local";
const PACK_FLOW_ADAPTER: &str = "pack_flow";

#[derive(Clone)]
pub struct FlowResumeStore {
    store: DynSessionStore,
}

impl FlowResumeStore {
    pub fn new(store: DynSessionStore) -> Self {
        Self { store }
    }

    fn fetch(&self, envelope: &IngressEnvelope) -> GResult<Option<FlowSnapshot>> {
        let (mut ctx, user, _) = build_store_ctx(envelope)?;
        ctx = ctx.with_user(Some(user.clone()));
        if let Some((_key, data)) = self
            .store
            .find_by_user(&ctx, &user)
            .map_err(map_store_error)?
        {
            let record: FlowResumeRecord =
                serde_json::from_str(&data.context_json).map_err(|err| RunnerError::Session {
                    reason: format!("failed to decode flow resume snapshot: {err}"),
                })?;
            if record.snapshot.flow_id == envelope.flow_id {
                return Ok(Some(record.snapshot));
            }
        }
        Ok(None)
    }

    fn save(&self, envelope: &IngressEnvelope, wait: &FlowWait) -> GResult<()> {
        let (ctx, user, hint) = build_store_ctx(envelope)?;
        let record = FlowResumeRecord {
            snapshot: wait.snapshot.clone(),
            reason: wait.reason.clone(),
        };
        let data = record_to_session_data(&record, ctx.clone(), &user, &hint)?;
        let existing = self
            .store
            .find_by_user(&ctx, &user)
            .map_err(map_store_error)?;
        if let Some((key, _)) = existing {
            self.store
                .update_session(&key, data)
                .map_err(map_store_error)?;
        } else {
            self.store
                .create_session(&ctx, data)
                .map_err(map_store_error)?;
        }
        Ok(())
    }

    fn clear(&self, envelope: &IngressEnvelope) -> GResult<()> {
        let (ctx, user, _) = build_store_ctx(envelope)?;
        if let Some((key, _)) = self
            .store
            .find_by_user(&ctx, &user)
            .map_err(map_store_error)?
        {
            self.store.remove_session(&key).map_err(map_store_error)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct FlowResumeRecord {
    snapshot: FlowSnapshot,
    #[serde(default)]
    reason: Option<String>,
}

fn build_store_ctx(envelope: &IngressEnvelope) -> GResult<(TenantCtx, UserId, String)> {
    let hint = envelope
        .session_hint
        .clone()
        .unwrap_or_else(|| envelope.canonical_session_hint());
    let user = derive_user_id(&hint)?;
    let mut ctx = envelope.tenant_ctx();
    ctx = ctx.with_session(hint.clone());
    Ok((ctx, user, hint))
}

fn record_to_session_data(
    record: &FlowResumeRecord,
    ctx: TenantCtx,
    user: &UserId,
    session_hint: &str,
) -> GResult<SessionData> {
    let flow = FlowId::from_str(record.snapshot.flow_id.as_str()).map_err(map_store_error)?;
    let mut cursor = TypesSessionCursor::new(record.snapshot.next_node.clone());
    if let Some(reason) = record.reason.clone() {
        cursor = cursor.with_wait_reason(reason);
    }
    let context_json = serde_json::to_string(record).map_err(|err| RunnerError::Session {
        reason: format!("failed to encode flow resume snapshot: {err}"),
    })?;
    let ctx = ctx
        .with_user(Some(user.clone()))
        .with_session(session_hint.to_string())
        .with_flow(record.snapshot.flow_id.clone());
    Ok(SessionData {
        tenant_ctx: ctx,
        flow_id: flow,
        cursor,
        context_json,
    })
}

fn derive_user_id(hint: &str) -> GResult<UserId> {
    let digest = Sha256::digest(hint.as_bytes());
    let slug = format!("sess{}", hex::encode(&digest[..8]));
    UserId::from_str(&slug).map_err(map_store_error)
}

fn map_store_error(err: GreenticError) -> RunnerError {
    RunnerError::Session {
        reason: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::engine::ExecutionState;
    use crate::storage::session::new_session_store;
    use serde_json::json;

    fn sample_envelope() -> IngressEnvelope {
        IngressEnvelope {
            tenant: "demo".into(),
            env: Some("local".into()),
            flow_id: "flow.main".into(),
            flow_type: None,
            action: Some("messaging".into()),
            session_hint: Some("demo:provider:chan:conv:user".into()),
            provider: Some("provider".into()),
            channel: Some("chan".into()),
            conversation: Some("conv".into()),
            user: Some("user".into()),
            activity_id: Some("act-1".into()),
            timestamp: None,
            payload: json!({ "text": "hi" }),
            metadata: None,
        }
    }

    fn sample_wait() -> FlowWait {
        let state: ExecutionState = serde_json::from_value(json!({
            "input": { "text": "hi" },
            "nodes": {},
            "egress": []
        }))
        .expect("state");
        FlowWait {
            reason: Some("await-user".into()),
            snapshot: FlowSnapshot {
                flow_id: "flow.main".into(),
                next_node: "node-2".into(),
                state,
            },
        }
    }

    #[test]
    fn derive_user_id_is_stable() {
        let hint = "some-tenant::session-key";
        let a = derive_user_id(hint).unwrap();
        let b = derive_user_id(hint).unwrap();
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("sess"));
    }

    #[test]
    fn resume_store_roundtrip() -> GResult<()> {
        let store = FlowResumeStore::new(new_session_store());
        let envelope = sample_envelope();
        assert!(store.fetch(&envelope)?.is_none());

        let wait = sample_wait();
        store.save(&envelope, &wait)?;
        let snapshot = store.fetch(&envelope)?.expect("snapshot missing");
        assert_eq!(snapshot.flow_id, wait.snapshot.flow_id);
        assert_eq!(snapshot.next_node, wait.snapshot.next_node);

        store.clear(&envelope)?;
        assert!(store.fetch(&envelope)?.is_none());
        Ok(())
    }

    #[test]
    fn resume_store_overwrites_existing() -> GResult<()> {
        let store = FlowResumeStore::new(new_session_store());
        let envelope = sample_envelope();
        let mut wait = sample_wait();
        store.save(&envelope, &wait)?;

        wait.snapshot.next_node = "node-3".into();
        wait.reason = Some("retry".into());
        store.save(&envelope, &wait)?;

        let snapshot = store.fetch(&envelope)?.expect("snapshot missing");
        assert_eq!(snapshot.next_node, "node-3");
        store.clear(&envelope)?;
        Ok(())
    }

    #[test]
    fn canonicalize_populates_defaults() {
        let envelope = IngressEnvelope {
            tenant: "demo".into(),
            env: None,
            flow_id: "flow.main".into(),
            flow_type: None,
            action: None,
            session_hint: None,
            provider: None,
            channel: None,
            conversation: None,
            user: None,
            activity_id: Some("activity-1".into()),
            timestamp: None,
            payload: json!({}),
            metadata: None,
        }
        .canonicalize();

        assert_eq!(envelope.provider.as_deref(), Some("provider"));
        assert_eq!(envelope.channel.as_deref(), Some("flow.main"));
        assert_eq!(envelope.conversation.as_deref(), Some("flow.main"));
        assert_eq!(envelope.user.as_deref(), Some("activity-1"));
        assert!(envelope.session_hint.is_some());
    }
}

pub struct StateMachineRuntime {
    runner: Runner,
}

impl StateMachineRuntime {
    /// Construct a runtime from explicit flow definitions (legacy entrypoint used by tests/examples).
    pub fn new(flows: Vec<FlowDefinition>) -> GResult<Self> {
        let secrets = Arc::new(FnSecretsHost::new(|name| {
            Err(RunnerError::Secrets {
                reason: format!("secret {name} unavailable (noop host)"),
            })
        }));
        let telemetry = Arc::new(FnTelemetryHost::new(|_, _| Ok(())));
        let session = Arc::new(InMemorySessionHost::new());
        let state = Arc::new(InMemoryStateHost::new());
        let host = HostBundle::new(secrets, telemetry, session, state);

        let adapters = AdapterRegistry::default();
        let policy = Policy::default();

        let mut builder = RunnerBuilder::new()
            .with_host(host)
            .with_adapters(adapters)
            .with_policy(policy);
        for flow in flows {
            builder = builder.with_flow(flow);
        }
        let runner = builder.build()?;
        Ok(Self { runner })
    }

    /// Build a state-machine runtime that proxies pack flows through the legacy FlowEngine.
    pub fn from_flow_engine(
        config: Arc<HostConfig>,
        engine: Arc<FlowEngine>,
        session_host: Arc<dyn SessionHost>,
        session_store: DynSessionStore,
        state_host: Arc<dyn StateHost>,
        secrets_manager: DynSecretsManager,
        mocks: Option<Arc<MockLayer>>,
    ) -> Result<Self> {
        let policy = Arc::new(config.secrets_policy.clone());
        let secrets = Arc::new(PolicySecretsHost::new(policy, secrets_manager));
        let telemetry = Arc::new(FnTelemetryHost::new(|span, fields| {
            tracing::debug!(?span, ?fields, "telemetry emit");
            Ok(())
        }));
        let host = HostBundle::new(secrets, telemetry, session_host, state_host);
        let resume_store = FlowResumeStore::new(session_store);

        let mut adapters = AdapterRegistry::default();
        adapters.register(
            PACK_FLOW_ADAPTER,
            Box::new(PackFlowAdapter::new(
                Arc::clone(&config),
                Arc::clone(&engine),
                resume_store,
                mocks,
            )),
        );

        let flows = build_flow_definitions(engine.flows());
        let mut builder = RunnerBuilder::new()
            .with_host(host)
            .with_adapters(adapters)
            .with_policy(Policy::default());
        for flow in flows {
            builder = builder.with_flow(flow);
        }
        let runner = builder
            .build()
            .map_err(|err| anyhow!("state machine init failed: {err}"))?;
        Ok(Self { runner })
    }

    /// Execute the flow associated with the provided ingress event.
    pub async fn handle(&self, envelope: IngressEnvelope) -> Result<Value> {
        let tenant_ctx = envelope.tenant_ctx();
        let session_hint = envelope
            .session_hint
            .clone()
            .unwrap_or_else(|| envelope.canonical_session_hint());
        let input =
            serde_json::to_value(&envelope).context("failed to serialise ingress envelope")?;
        let request = RunFlowRequest {
            tenant: tenant_ctx,
            flow_id: envelope.flow_id.clone(),
            input,
            session_hint: Some(session_hint),
        };
        let result: super::api::RunFlowResult = self
            .runner
            .run_flow(request)
            .await
            .map_err(|err| anyhow!("flow execution failed: {err}"))?;
        let outcome = result.outcome;
        Ok(outcome.get("response").cloned().unwrap_or(outcome))
    }
}

struct PolicySecretsHost {
    policy: Arc<SecretsPolicy>,
    manager: DynSecretsManager,
}

impl PolicySecretsHost {
    fn new(policy: Arc<SecretsPolicy>, manager: DynSecretsManager) -> Self {
        Self { policy, manager }
    }
}

#[async_trait]
impl SecretsHost for PolicySecretsHost {
    async fn get(&self, name: &str) -> GResult<String> {
        if !self.policy.is_allowed(name) {
            return Err(RunnerError::Secrets {
                reason: format!("secret {name} denied by policy"),
            });
        }
        let bytes = self
            .manager
            .read(name)
            .await
            .map_err(|err| RunnerError::Secrets {
                reason: format!("secret {name} unavailable: {err}"),
            })?;
        String::from_utf8(bytes).map_err(|err| RunnerError::Secrets {
            reason: format!("secret {name} not valid UTF-8: {err}"),
        })
    }
}

fn build_flow_definitions(flows: &[FlowDescriptor]) -> Vec<FlowDefinition> {
    flows
        .iter()
        .map(|descriptor| {
            FlowDefinition::new(
                super::api::FlowSummary {
                    id: descriptor.id.clone(),
                    name: descriptor
                        .description
                        .clone()
                        .unwrap_or_else(|| descriptor.id.clone()),
                    version: descriptor.version.clone(),
                    description: descriptor.description.clone(),
                },
                serde_json::json!({
                    "type": "object"
                }),
                vec![FlowStep::Adapter(AdapterCall {
                    adapter: PACK_FLOW_ADAPTER.into(),
                    operation: descriptor.id.clone(),
                    payload: Value::String(PAYLOAD_FROM_LAST_INPUT.into()),
                })],
            )
        })
        .collect()
}

struct PackFlowAdapter {
    tenant: String,
    config: Arc<HostConfig>,
    engine: Arc<FlowEngine>,
    resume: FlowResumeStore,
    mocks: Option<Arc<MockLayer>>,
}

impl PackFlowAdapter {
    fn new(
        config: Arc<HostConfig>,
        engine: Arc<FlowEngine>,
        resume: FlowResumeStore,
        mocks: Option<Arc<MockLayer>>,
    ) -> Self {
        Self {
            tenant: config.tenant.clone(),
            config,
            engine,
            resume,
            mocks,
        }
    }
}

#[async_trait::async_trait]
impl Adapter for PackFlowAdapter {
    async fn call(&self, call: &AdapterCall) -> GResult<Value> {
        let envelope: IngressEnvelope =
            serde_json::from_value(call.payload.clone()).map_err(|err| {
                RunnerError::AdapterCall {
                    reason: format!("invalid ingress payload: {err}"),
                }
            })?;
        let flow_id = call.operation.clone();
        let action_owned = envelope.action.clone();
        let session_owned = envelope
            .session_hint
            .clone()
            .unwrap_or_else(|| envelope.canonical_session_hint());
        let provider_owned = envelope.provider.clone();
        let payload = envelope.payload.clone();
        let retry_config = self.config.retry_config().into();

        let mocks = self.mocks.as_deref();
        let ctx = FlowContext {
            tenant: &self.tenant,
            flow_id: &flow_id,
            node_id: None,
            tool: None,
            action: action_owned.as_deref(),
            session_id: Some(session_owned.as_str()),
            provider_id: provider_owned.as_deref(),
            retry_config,
            observer: None,
            mocks,
        };

        let execution = if let Some(snapshot) = self.resume.fetch(&envelope)? {
            self.engine.resume(ctx, snapshot, payload).await
        } else {
            self.engine.execute(ctx, payload).await
        }
        .map_err(|err| RunnerError::AdapterCall {
            reason: err.to_string(),
        })?;

        match execution.status {
            FlowStatus::Completed => {
                self.resume.clear(&envelope)?;
                Ok(execution.output)
            }
            FlowStatus::Waiting(wait) => {
                self.resume.save(&envelope, &wait)?;
                Ok(json!({
                    "status": "pending",
                    "reason": wait.reason,
                    "resume": wait.snapshot,
                    "response": execution.output,
                }))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IngressEnvelope {
    pub tenant: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    pub flow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl IngressEnvelope {
    pub fn canonicalize(mut self) -> Self {
        if self.provider.is_none() {
            self.provider = Some("provider".into());
        }
        if self.channel.is_none() {
            self.channel = Some(self.flow_id.clone());
        }
        if self.conversation.is_none() {
            self.conversation = self.channel.clone();
        }
        if self.user.is_none() {
            if let Some(ref hint) = self.session_hint {
                self.user = Some(hint.clone());
            } else if let Some(ref activity) = self.activity_id {
                self.user = Some(activity.clone());
            } else {
                self.user = Some("user".into());
            }
        }
        if self.session_hint.is_none() {
            self.session_hint = Some(self.canonical_session_hint());
        }
        self
    }

    pub fn canonical_session_hint(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.tenant,
            self.provider.as_deref().unwrap_or("provider"),
            self.channel.as_deref().unwrap_or("channel"),
            self.conversation.as_deref().unwrap_or("conversation"),
            self.user.as_deref().unwrap_or("user")
        )
    }

    pub fn tenant_ctx(&self) -> TenantCtx {
        let env_raw = self.env.clone().unwrap_or_else(|| DEFAULT_ENV.into());
        let env = EnvId::from_str(env_raw.as_str())
            .unwrap_or_else(|_| EnvId::from_str(DEFAULT_ENV).expect("default env must be valid"));
        let tenant_id = TenantId::from_str(self.tenant.as_str()).unwrap_or_else(|_| {
            TenantId::from_str("tenant.default").expect("tenant fallback must be valid")
        });
        let mut ctx = TenantCtx::new(env, tenant_id).with_flow(self.flow_id.clone());
        if let Some(provider) = &self.provider {
            ctx = ctx.with_provider(provider.clone());
        }
        if let Some(session) = &self.session_hint {
            ctx = ctx.with_session(session.clone());
        }
        ctx
    }
}
