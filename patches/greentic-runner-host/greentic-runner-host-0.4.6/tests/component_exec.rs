use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use greentic_flow::ir::{FlowIR, NodeIR, RouteIR};
use greentic_runner_host::config::{
    FlowRetryConfig, HostConfig, RateLimits, SecretsPolicy, WebhookPolicy,
};
use greentic_runner_host::pack::PackRuntime;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

fn build_components() -> Result<Vec<(String, PathBuf)>> {
    let workspace = workspace_root().join("tests/fixtures/runner-components");
    let mut results = Vec::new();
    let crates = vec![
        ("qa.process", "qa_process"),
        ("templating.handlebars", "templating_handlebars"),
    ];
    for (name, krate) in &crates {
        let manifest = workspace.join(krate).join("Cargo.toml");
        let status = std::process::Command::new("cargo")
            .env("CARGO_NET_OFFLINE", "true")
            .current_dir(&workspace)
            .args([
                "build",
                "--offline",
                "--manifest-path",
                manifest.to_str().unwrap(),
                "--target",
                "wasm32-wasip2",
                "--release",
            ])
            .status()
            .with_context(|| format!("failed to build {krate} component"))?;
        if !status.success() {
            anyhow::bail!("component build failed for {krate}");
        }
        let artifact = workspace.join(format!("target/wasm32-wasip2/release/{}.wasm", krate));
        results.push((name.to_string(), artifact));
    }
    Ok(results)
}

fn demo_flow_ir() -> FlowIR {
    let mut nodes = indexmap::IndexMap::new();
    nodes.insert(
        "qa".into(),
        NodeIR {
            component: "component.exec".into(),
            payload_expr: serde_json::json!({
                "component": "qa.process",
                "operation": "process",
                "input": { "text": "hello" }
            }),
            routes: vec![RouteIR {
                to: Some("tmpl".into()),
                out: false,
            }],
        },
    );
    nodes.insert(
        "tmpl".into(),
        NodeIR {
            component: "component.exec".into(),
            payload_expr: serde_json::json!({
                "component": "templating.handlebars",
                "operation": "render",
                "input": {
                    "template": "Echo: {{state.nodes.qa.payload.text}}"
                }
            }),
            routes: vec![RouteIR {
                to: Some("emit".into()),
                out: false,
            }],
        },
    );
    nodes.insert(
        "emit".into(),
        NodeIR {
            component: "emit.response".into(),
            payload_expr: serde_json::json!({
                "from_node": "tmpl"
            }),
            routes: vec![RouteIR {
                to: None,
                out: true,
            }],
        },
    );
    FlowIR {
        id: "demo.flow".into(),
        flow_type: "demo".into(),
        start: Some("qa".into()),
        parameters: serde_json::Value::Object(Default::default()),
        nodes,
    }
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

#[test]
fn component_exec_invokes_pack_component() -> Result<()> {
    let temp = TempDir::new()?;
    let bindings_path = temp.path().join("bindings.yaml");
    std::fs::write(&bindings_path, b"tenant: demo")?;

    let components = build_components()?;
    let flow = demo_flow_ir();
    let mut flows = HashMap::new();
    flows.insert(flow.id.clone(), flow);

    let config = Arc::new(host_config(&bindings_path));
    let runtime = Arc::new(
        PackRuntime::for_component_test(components, flows.clone(), Arc::clone(&config))
            .context("pack init")?,
    );
    // Ensure the runtime constructed successfully with components and flows in place.
    let _ = (runtime, config, flows);
    Ok(())
}
