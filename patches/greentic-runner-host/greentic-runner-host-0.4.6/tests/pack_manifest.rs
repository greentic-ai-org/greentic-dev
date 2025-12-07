use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use greentic_runner_host::config::{
    FlowRetryConfig, HostConfig, RateLimits, SecretsPolicy, WebhookPolicy,
};
use greentic_runner_host::pack::PackRuntime;
use greentic_runner_host::wasi::RunnerWasiPolicy;
use once_cell::sync::Lazy;
use serde_json::json;
use std::path::Path;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

fn host_config(bindings_path: &Path) -> HostConfig {
    HostConfig {
        tenant: "demo".into(),
        bindings_path: bindings_path.to_path_buf(),
        flow_type_bindings: HashMap::new(),
        rate_limits: RateLimits::default(),
        retry: FlowRetryConfig::default(),
        http_enabled: false,
        secrets_policy: SecretsPolicy::allow_all(),
        webhook_policy: WebhookPolicy::default(),
        timers: Vec::new(),
        oauth: None,
        mocks: None,
    }
}

fn demo_exec_ctx(node_id: &str) -> greentic_runner_host::component_api::node::ExecCtx {
    greentic_runner_host::component_api::node::ExecCtx {
        tenant: greentic_runner_host::component_api::node::TenantCtx {
            tenant: "demo".into(),
            team: None,
            user: None,
            trace_id: None,
            correlation_id: None,
            deadline_unix_ms: None,
            attempt: 0,
            idempotency_key: None,
        },
        flow_id: "demo.flow".into(),
        node_id: Some(node_id.into()),
    }
}

static RUNTIME: Lazy<&'static tokio::runtime::Runtime> = Lazy::new(|| {
    Box::leak(Box::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime"),
    ))
});

#[test]
fn gtpack_manifest_fast_path_invokes_components() -> Result<()> {
    let rt = *RUNTIME;
    let pack_root = workspace_root().join("tests/fixtures/packs/runner-components");
    let gtpack = pack_root.join("runner-components.gtpack");
    let bindings_path = pack_root.join("bindings.yaml");
    std::fs::write(&bindings_path, b"tenant: demo")?;

    let config = Arc::new(host_config(&bindings_path));
    let runtime = Arc::new(rt.block_on(PackRuntime::load(
        &gtpack,
        Arc::clone(&config),
        None,
        None,
        None,
        None,
        Arc::new(RunnerWasiPolicy::new()),
        greentic_runner_host::secrets::default_manager(),
        None,
        false,
    ))?);

    let flows = rt.block_on(runtime.list_flows())?;
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].id, "demo.flow");

    let ir = runtime.load_flow_ir("demo.flow")?;
    assert!(ir.start.is_some());

    let ctx = demo_exec_ctx("qa");
    let result = rt.block_on(runtime.invoke_component(
        "qa.process",
        ctx,
        "process",
        None,
        serde_json::to_string(&json!({ "text": "hello" }))?,
    ))?;
    assert_eq!(result, json!({ "text": "hello" }));

    // Validate the templating component resolves and can render against simple state.
    let ctx = demo_exec_ctx("tmpl");
    let render = rt.block_on(runtime.invoke_component(
        "templating.handlebars",
        ctx,
        "render",
        None,
        serde_json::to_string(&json!({
            "template": "Echo: {{input.text}}",
            "input": { "text": "hi" }
        }))?,
    ))?;
    match render {
        serde_json::Value::String(s) => assert!(s.starts_with("Echo:")),
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get("text") {
                assert!(s.starts_with("Echo:"));
            } else {
                panic!("templating output missing text: {map:?}");
            }
        }
        other => panic!("unexpected templating output: {other:?}"),
    }
    Ok(())
}
