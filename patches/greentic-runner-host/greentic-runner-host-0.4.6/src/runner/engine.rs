use std::collections::HashMap;
use std::env;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use crate::component_api::node::{ExecCtx as ComponentExecCtx, TenantCtx as ComponentTenantCtx};
use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::ir::{FlowIR, NodeIR};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value, json};
use tokio::task;

use super::mocks::MockLayer;
use crate::config::{FlowRetryConfig, HostConfig};
use crate::pack::{FlowDescriptor, PackRuntime};
use crate::telemetry::{FlowSpanAttributes, annotate_span, backoff_delay_ms, set_flow_context};

pub struct FlowEngine {
    packs: Vec<Arc<PackRuntime>>,
    flows: Vec<FlowDescriptor>,
    flow_sources: HashMap<String, usize>,
    flow_ir: RwLock<HashMap<String, FlowIR>>,
    default_env: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlowSnapshot {
    pub flow_id: String,
    pub next_node: String,
    pub state: ExecutionState,
}

#[derive(Clone, Debug)]
pub struct FlowWait {
    pub reason: Option<String>,
    pub snapshot: FlowSnapshot,
}

#[derive(Clone, Debug)]
pub enum FlowStatus {
    Completed,
    Waiting(FlowWait),
}

#[derive(Clone, Debug)]
pub struct FlowExecution {
    pub output: Value,
    pub status: FlowStatus,
}

impl FlowExecution {
    fn completed(output: Value) -> Self {
        Self {
            output,
            status: FlowStatus::Completed,
        }
    }

    fn waiting(output: Value, wait: FlowWait) -> Self {
        Self {
            output,
            status: FlowStatus::Waiting(wait),
        }
    }
}

impl FlowEngine {
    pub async fn new(packs: Vec<Arc<PackRuntime>>, _config: Arc<HostConfig>) -> Result<Self> {
        let mut flow_sources = HashMap::new();
        let mut descriptors = Vec::new();
        for (idx, pack) in packs.iter().enumerate() {
            let flows = pack.list_flows().await?;
            for flow in flows {
                tracing::info!(
                    flow_id = %flow.id,
                    flow_type = %flow.flow_type,
                    pack_index = idx,
                    "registered flow"
                );
                flow_sources.insert(flow.id.clone(), idx);
                descriptors.retain(|existing: &FlowDescriptor| existing.id != flow.id);
                descriptors.push(flow);
            }
        }

        let mut ir_map = HashMap::new();
        for flow in &descriptors {
            if let Some(&pack_idx) = flow_sources.get(&flow.id) {
                let pack_clone = Arc::clone(&packs[pack_idx]);
                let flow_id = flow.id.clone();
                let task_flow_id = flow_id.clone();
                match task::spawn_blocking(move || pack_clone.load_flow_ir(&task_flow_id)).await {
                    Ok(Ok(ir)) => {
                        ir_map.insert(flow_id, ir);
                    }
                    Ok(Err(err)) => {
                        tracing::warn!(flow_id = %flow.id, error = %err, "failed to load flow metadata");
                    }
                    Err(err) => {
                        tracing::warn!(flow_id = %flow.id, error = %err, "join error loading flow metadata");
                    }
                }
            }
        }

        Ok(Self {
            packs,
            flows: descriptors,
            flow_sources,
            flow_ir: RwLock::new(ir_map),
            default_env: env::var("GREENTIC_ENV").unwrap_or_else(|_| "local".to_string()),
        })
    }

    async fn get_or_load_flow_ir(&self, flow_id: &str) -> Result<FlowIR> {
        if let Some(ir) = self.flow_ir.read().get(flow_id).cloned() {
            return Ok(ir);
        }

        let pack_idx = *self
            .flow_sources
            .get(flow_id)
            .with_context(|| format!("flow {flow_id} not registered"))?;
        let pack = Arc::clone(&self.packs[pack_idx]);
        let flow_id_owned = flow_id.to_string();
        let task_flow_id = flow_id_owned.clone();
        let ir = task::spawn_blocking(move || pack.load_flow_ir(&task_flow_id))
            .await
            .context("failed to join flow metadata task")??;
        self.flow_ir
            .write()
            .insert(flow_id_owned.clone(), ir.clone());
        Ok(ir)
    }

    pub async fn execute(&self, ctx: FlowContext<'_>, input: Value) -> Result<FlowExecution> {
        let span = tracing::info_span!(
            "flow.execute",
            tenant = tracing::field::Empty,
            flow_id = tracing::field::Empty,
            node_id = tracing::field::Empty,
            tool = tracing::field::Empty,
            action = tracing::field::Empty
        );
        annotate_span(
            &span,
            &FlowSpanAttributes {
                tenant: ctx.tenant,
                flow_id: ctx.flow_id,
                node_id: ctx.node_id,
                tool: ctx.tool,
                action: ctx.action,
            },
        );
        set_flow_context(
            &self.default_env,
            ctx.tenant,
            ctx.flow_id,
            ctx.node_id,
            ctx.provider_id,
            ctx.session_id,
        );
        let retry_config = ctx.retry_config;
        let original_input = input;
        async move {
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                match self.execute_once(&ctx, original_input.clone()).await {
                    Ok(value) => return Ok(value),
                    Err(err) => {
                        if attempt >= retry_config.max_attempts || !should_retry(&err) {
                            return Err(err);
                        }
                        let delay = backoff_delay_ms(retry_config.base_delay_ms, attempt - 1);
                        tracing::warn!(
                            tenant = ctx.tenant,
                            flow_id = ctx.flow_id,
                            attempt,
                            max_attempts = retry_config.max_attempts,
                            delay_ms = delay,
                            error = %err,
                            "transient flow execution failure, backing off"
                        );
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }
        .instrument(span)
        .await
    }

    pub async fn resume(
        &self,
        ctx: FlowContext<'_>,
        snapshot: FlowSnapshot,
        input: Value,
    ) -> Result<FlowExecution> {
        if snapshot.flow_id != ctx.flow_id {
            bail!(
                "snapshot flow {} does not match requested {}",
                snapshot.flow_id,
                ctx.flow_id
            );
        }
        let flow_ir = self.get_or_load_flow_ir(ctx.flow_id).await?;
        let mut state = snapshot.state;
        state.replace_input(input);
        self.drive_flow(&ctx, flow_ir, state, Some(snapshot.next_node))
            .await
    }

    async fn execute_once(&self, ctx: &FlowContext<'_>, input: Value) -> Result<FlowExecution> {
        let flow_ir = self.get_or_load_flow_ir(ctx.flow_id).await?;
        let state = ExecutionState::new(input);
        self.drive_flow(ctx, flow_ir, state, None).await
    }

    async fn drive_flow(
        &self,
        ctx: &FlowContext<'_>,
        flow_ir: FlowIR,
        mut state: ExecutionState,
        resume_from: Option<String>,
    ) -> Result<FlowExecution> {
        let mut current = flow_ir
            .start
            .clone()
            .or_else(|| flow_ir.nodes.keys().next().cloned())
            .with_context(|| format!("flow {} has no start node", flow_ir.id))?;
        if let Some(resume) = resume_from {
            current = resume;
        }
        let mut final_payload = None;

        loop {
            let node = flow_ir
                .nodes
                .get(&current)
                .with_context(|| format!("node {current} not found"))?;

            let payload = node.payload_expr.clone();
            let observed_payload = payload.clone();
            let node_id = current.clone();
            let event = NodeEvent {
                context: ctx,
                node_id: &node_id,
                node,
                payload: &observed_payload,
            };
            if let Some(observer) = ctx.observer {
                observer.on_node_start(&event);
            }
            let DispatchOutcome {
                output,
                wait_reason,
            } = self
                .dispatch_node(ctx, &current, node, &mut state, payload)
                .await?;

            state.nodes.insert(current.clone(), output.clone());

            let mut next = None;
            let mut should_exit = false;
            for route in &node.routes {
                if route.out || matches!(route.to.as_deref(), Some("out")) {
                    final_payload = Some(output.payload.clone());
                    should_exit = true;
                    break;
                }
                if let Some(to) = &route.to {
                    next = Some(to.clone());
                    break;
                }
            }

            if let Some(wait_reason) = wait_reason {
                let resume_target = next.clone().ok_or_else(|| {
                    anyhow!("session.wait node {current} requires a non-empty route")
                })?;
                let mut snapshot_state = state.clone();
                snapshot_state.clear_egress();
                let snapshot = FlowSnapshot {
                    flow_id: ctx.flow_id.to_string(),
                    next_node: resume_target,
                    state: snapshot_state,
                };
                let output_value = state.clone().finalize_with(None);
                return Ok(FlowExecution::waiting(
                    output_value,
                    FlowWait {
                        reason: Some(wait_reason),
                        snapshot,
                    },
                ));
            }

            if should_exit {
                break;
            }

            match next {
                Some(n) => current = n,
                None => {
                    final_payload = Some(output.payload.clone());
                    break;
                }
            }
        }

        let payload = final_payload.unwrap_or(Value::Null);
        Ok(FlowExecution::completed(state.finalize_with(Some(payload))))
    }

    async fn dispatch_node(
        &self,
        ctx: &FlowContext<'_>,
        node_id: &str,
        node: &NodeIR,
        state: &mut ExecutionState,
        payload: Value,
    ) -> Result<DispatchOutcome> {
        match node.component.as_str() {
            "component.exec" => self
                .execute_component_exec(ctx, node_id, state, payload)
                .await
                .map(DispatchOutcome::complete),
            "flow.call" => self
                .execute_flow_call(ctx, payload)
                .await
                .map(DispatchOutcome::complete),
            component if component.starts_with("emit") => {
                state.push_egress(payload.clone());
                Ok(DispatchOutcome::complete(NodeOutput::new(payload)))
            }
            "session.wait" => {
                let reason = extract_wait_reason(&payload);
                Ok(DispatchOutcome::wait(NodeOutput::new(payload), reason))
            }
            other => bail!(
                "unsupported node component `{other}` in flow {} node {}",
                ctx.flow_id,
                node_id
            ),
        }
    }

    async fn execute_flow_call(&self, ctx: &FlowContext<'_>, payload: Value) -> Result<NodeOutput> {
        #[derive(Deserialize)]
        struct FlowCallPayload {
            #[serde(alias = "flow")]
            flow_id: String,
            #[serde(default)]
            input: Value,
        }

        let call: FlowCallPayload =
            serde_json::from_value(payload).context("invalid payload for flow.call node")?;
        if call.flow_id.trim().is_empty() {
            bail!("flow.call requires a non-empty flow_id");
        }

        let sub_input = if call.input.is_null() {
            Value::Null
        } else {
            call.input
        };

        let flow_id_owned = call.flow_id;
        let action = "flow.call";
        let sub_ctx = FlowContext {
            tenant: ctx.tenant,
            flow_id: flow_id_owned.as_str(),
            node_id: None,
            tool: ctx.tool,
            action: Some(action),
            session_id: ctx.session_id,
            provider_id: ctx.provider_id,
            retry_config: ctx.retry_config,
            observer: ctx.observer,
            mocks: ctx.mocks,
        };

        let execution = Box::pin(self.execute(sub_ctx, sub_input))
            .await
            .with_context(|| format!("flow.call failed for {}", flow_id_owned))?;
        match execution.status {
            FlowStatus::Completed => Ok(NodeOutput::new(execution.output)),
            FlowStatus::Waiting(wait) => bail!(
                "flow.call cannot pause (flow {} waiting {:?})",
                flow_id_owned,
                wait.reason
            ),
        }
    }

    async fn execute_component_exec(
        &self,
        ctx: &FlowContext<'_>,
        node_id: &str,
        state: &ExecutionState,
        payload: Value,
    ) -> Result<NodeOutput> {
        #[derive(Deserialize)]
        struct ComponentPayload {
            #[serde(default, alias = "component_ref", alias = "component")]
            component: Option<String>,
            #[serde(alias = "op")]
            operation: Option<String>,
            #[serde(default)]
            input: Value,
            #[serde(default)]
            config: Value,
        }

        let payload: ComponentPayload =
            serde_json::from_value(payload).context("invalid payload for component.exec")?;
        let component_ref = payload
            .component
            .filter(|v| !v.trim().is_empty())
            .with_context(|| "component.exec requires a component_ref")?;
        let operation = payload
            .operation
            .filter(|v| !v.trim().is_empty())
            .with_context(|| "component.exec requires an operation")?;
        let mut input = payload.input;
        if let Value::Object(mut map) = input {
            map.entry("state".to_string())
                .or_insert_with(|| state.context());
            input = Value::Object(map);
        }
        let input_json = serde_json::to_string(&input)?;
        let config_json = if payload.config.is_null() {
            None
        } else {
            Some(serde_json::to_string(&payload.config)?)
        };

        let pack_idx = *self
            .flow_sources
            .get(ctx.flow_id)
            .with_context(|| format!("flow {} not registered", ctx.flow_id))?;
        let pack = Arc::clone(&self.packs[pack_idx]);
        let exec_ctx = component_exec_ctx(ctx, node_id);
        let value = pack
            .invoke_component(
                &component_ref,
                exec_ctx,
                &operation,
                config_json,
                input_json,
            )
            .await?;

        Ok(NodeOutput::new(value))
    }

    pub fn flows(&self) -> &[FlowDescriptor] {
        &self.flows
    }

    pub fn flow_by_type(&self, flow_type: &str) -> Option<&FlowDescriptor> {
        self.flows
            .iter()
            .find(|descriptor| descriptor.flow_type == flow_type)
    }

    pub fn flow_by_id(&self, flow_id: &str) -> Option<&FlowDescriptor> {
        self.flows
            .iter()
            .find(|descriptor| descriptor.id == flow_id)
    }
}

pub trait ExecutionObserver: Send + Sync {
    fn on_node_start(&self, event: &NodeEvent<'_>);
    fn on_node_end(&self, event: &NodeEvent<'_>, output: &Value);
    fn on_node_error(&self, event: &NodeEvent<'_>, error: &dyn StdError);
}

pub struct NodeEvent<'a> {
    pub context: &'a FlowContext<'a>,
    pub node_id: &'a str,
    pub node: &'a NodeIR,
    pub payload: &'a Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionState {
    input: Value,
    nodes: HashMap<String, NodeOutput>,
    egress: Vec<Value>,
}

impl ExecutionState {
    fn new(input: Value) -> Self {
        Self {
            input,
            nodes: HashMap::new(),
            egress: Vec::new(),
        }
    }

    fn context(&self) -> Value {
        let mut nodes = JsonMap::new();
        for (id, output) in &self.nodes {
            nodes.insert(
                id.clone(),
                json!({
                    "ok": output.ok,
                    "payload": output.payload.clone(),
                    "meta": output.meta.clone(),
                }),
            );
        }
        json!({
            "input": self.input.clone(),
            "nodes": nodes,
        })
    }
    fn push_egress(&mut self, payload: Value) {
        self.egress.push(payload);
    }

    fn replace_input(&mut self, input: Value) {
        self.input = input;
    }

    fn clear_egress(&mut self) {
        self.egress.clear();
    }

    fn finalize_with(mut self, final_payload: Option<Value>) -> Value {
        if self.egress.is_empty() {
            return final_payload.unwrap_or(Value::Null);
        }
        let mut emitted = std::mem::take(&mut self.egress);
        if let Some(value) = final_payload {
            match value {
                Value::Null => {}
                Value::Array(items) => emitted.extend(items),
                other => emitted.push(other),
            }
        }
        Value::Array(emitted)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeOutput {
    ok: bool,
    payload: Value,
    meta: Value,
}

impl NodeOutput {
    fn new(payload: Value) -> Self {
        Self {
            ok: true,
            payload,
            meta: Value::Null,
        }
    }
}

struct DispatchOutcome {
    output: NodeOutput,
    wait_reason: Option<String>,
}

impl DispatchOutcome {
    fn complete(output: NodeOutput) -> Self {
        Self {
            output,
            wait_reason: None,
        }
    }

    fn wait(output: NodeOutput, reason: Option<String>) -> Self {
        Self {
            output,
            wait_reason: reason,
        }
    }
}

fn component_exec_ctx(ctx: &FlowContext<'_>, node_id: &str) -> ComponentExecCtx {
    ComponentExecCtx {
        tenant: ComponentTenantCtx {
            tenant: ctx.tenant.to_string(),
            team: None,
            user: ctx.provider_id.map(str::to_string),
            trace_id: None,
            correlation_id: ctx.session_id.map(str::to_string),
            deadline_unix_ms: None,
            attempt: 1,
            idempotency_key: ctx.session_id.map(str::to_string),
        },
        flow_id: ctx.flow_id.to_string(),
        node_id: Some(node_id.to_string()),
    }
}

fn extract_wait_reason(payload: &Value) -> Option<String> {
    match payload {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("reason")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn templating_renders_with_partials_and_data() {
        let mut state = ExecutionState::new(json!({ "city": "London" }));
        state.nodes.insert(
            "forecast".to_string(),
            NodeOutput::new(json!({ "temp": "20C" })),
        );

        // templating handled via component now; ensure context still includes node outputs
        let ctx = state.context();
        assert_eq!(ctx["nodes"]["forecast"]["payload"]["temp"], json!("20C"));
    }

    #[test]
    fn finalize_wraps_emitted_payloads() {
        let mut state = ExecutionState::new(json!({}));
        state.push_egress(json!({ "text": "first" }));
        state.push_egress(json!({ "text": "second" }));
        let result = state.finalize_with(Some(json!({ "text": "final" })));
        assert_eq!(
            result,
            json!([
                { "text": "first" },
                { "text": "second" },
                { "text": "final" }
            ])
        );
    }

    #[test]
    fn finalize_flattens_final_array() {
        let mut state = ExecutionState::new(json!({}));
        state.push_egress(json!({ "text": "only" }));
        let result = state.finalize_with(Some(json!([
            { "text": "extra-1" },
            { "text": "extra-2" }
        ])));
        assert_eq!(
            result,
            json!([
                { "text": "only" },
                { "text": "extra-1" },
                { "text": "extra-2" }
            ])
        );
    }
}

use tracing::Instrument;

pub struct FlowContext<'a> {
    pub tenant: &'a str,
    pub flow_id: &'a str,
    pub node_id: Option<&'a str>,
    pub tool: Option<&'a str>,
    pub action: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub provider_id: Option<&'a str>,
    pub retry_config: RetryConfig,
    pub observer: Option<&'a dyn ExecutionObserver>,
    pub mocks: Option<&'a MockLayer>,
}

#[derive(Copy, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
}

fn should_retry(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    lower.contains("transient") || lower.contains("unavailable") || lower.contains("internal")
}

impl From<FlowRetryConfig> for RetryConfig {
    fn from(value: FlowRetryConfig) -> Self {
        Self {
            max_attempts: value.max_attempts.max(1),
            base_delay_ms: value.base_delay_ms.max(50),
        }
    }
}
