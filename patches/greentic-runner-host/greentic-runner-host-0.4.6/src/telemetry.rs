use greentic_types::telemetry::set_current_tenant_ctx;
use greentic_types::{EnvId, TenantCtx, TenantId};
use rand::{Rng, rng};
use std::str::FromStr;
use tracing::Span;

pub const PROVIDER_ID: &str = "greentic-runner";

#[derive(Debug, Clone)]
pub struct FlowSpanAttributes<'a> {
    pub tenant: &'a str,
    pub flow_id: &'a str,
    pub node_id: Option<&'a str>,
    pub tool: Option<&'a str>,
    pub action: Option<&'a str>,
}

pub fn annotate_span(span: &Span, attrs: &FlowSpanAttributes<'_>) {
    span.record("tenant", attrs.tenant);
    span.record("flow_id", attrs.flow_id);
    if let Some(node) = attrs.node_id {
        span.record("node_id", node);
    }
    if let Some(tool) = attrs.tool {
        span.record("tool", tool);
    }
    if let Some(action) = attrs.action {
        span.record("action", action);
    }
}

pub fn tenant_context(
    env: &str,
    tenant: &str,
    flow_id: Option<&str>,
    node_id: Option<&str>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
) -> TenantCtx {
    let env_id = EnvId::from_str(env).expect("invalid env id");
    let tenant_id = TenantId::from_str(tenant).expect("invalid tenant id");
    let mut ctx = TenantCtx::new(env_id, tenant_id);
    let provider = provider_id.unwrap_or(PROVIDER_ID);
    ctx = ctx.with_provider(provider.to_string());
    if let Some(flow) = flow_id {
        ctx = ctx.with_flow(flow.to_string());
    }
    if let Some(node) = node_id {
        ctx = ctx.with_node(node.to_string());
    }
    if let Some(session) = session_id {
        ctx = ctx.with_session(session.to_string());
    }
    ctx
}

pub fn set_flow_context(
    env: &str,
    tenant: &str,
    flow_id: &str,
    node_id: Option<&str>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
) {
    let ctx = tenant_context(env, tenant, Some(flow_id), node_id, provider_id, session_id);
    set_current_tenant_ctx(&ctx);
}

pub fn backoff_delay_ms(base: u64, attempt: u32) -> u64 {
    let multiplier = 1_u64 << attempt.min(10);
    let exp = base.saturating_mul(multiplier);
    let mut rng = rng();
    let jitter = rng.random_range(0..=exp.min(1000));
    exp + jitter
}
