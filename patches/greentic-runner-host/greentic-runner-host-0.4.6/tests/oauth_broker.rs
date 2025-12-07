use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use greentic_runner_host::config::HostConfig;
use greentic_runner_host::imports;
use greentic_runner_host::pack::{ComponentState, HostState};
use greentic_runner_host::runtime_wasmtime::{Component, Engine, Linker, Store};
use greentic_runner_host::secrets::default_manager;
use greentic_runner_host::wasi::RunnerWasiPolicy;
use reqwest::blocking::Client as BlockingClient;
use tempfile::NamedTempFile;

#[test]
fn oauth_world_instantiates_when_enabled() -> Result<()> {
    let wasm = build_fixture()?;
    let host_cfg = load_host_config(true)?;
    instantiate_component(&wasm, host_cfg, true).context("component should instantiate")
}

fn instantiate_component(wasm: &Path, config: Arc<HostConfig>, oauth_enabled: bool) -> Result<()> {
    let engine = Engine::default();
    let component = Component::from_file(&engine, wasm)
        .with_context(|| format!("failed to load {}", wasm.display()))?;
    let host_state = HostState::new(
        Arc::clone(&config),
        Arc::new(BlockingClient::builder().build()?),
        None,
        None,
        None,
        default_manager(),
        config.oauth_broker_config(),
    )?;
    let policy = Arc::new(RunnerWasiPolicy::default());
    let state = ComponentState::new(host_state, policy)?;
    let mut store = Store::new(&engine, state);
    let mut linker = Linker::new(&engine);
    imports::register_all(&mut linker, oauth_enabled)?;
    linker
        .instantiate(&mut store, &component)
        .context("component instantiation failed")?;
    Ok(())
}

fn load_host_config(enable_oauth: bool) -> Result<Arc<HostConfig>> {
    let file = NamedTempFile::new()?;
    let oauth_block = if enable_oauth {
        r#"
oauth:
  http_base_url: "https://oauth.example"
  nats_url: "nats://localhost:4222"
  provider: "demo"
"#
    } else {
        ""
    };
    let contents = format!(
        r#"
tenant: test-tenant
flow_type_bindings: {{}}
mcp:
  store: {{}}
  security: {{}}
  runtime: {{}}
rate_limits: {{}}
timers: []
{oauth_block}
"#
    );
    fs::write(file.path(), contents)?;
    let cfg = HostConfig::load_from_path(file.path())?;
    Ok(Arc::new(cfg))
}

fn build_fixture() -> Result<PathBuf> {
    let workspace = workspace_root();
    let manifest = workspace.join("tests/fixtures/oauth-broker-component/Cargo.toml");
    let target_dir = workspace.join("target/oauth-broker-fixture");
    let status = Command::new("cargo")
        .args([
            "build",
            "--manifest-path",
            manifest
                .to_str()
                .ok_or_else(|| anyhow!("fixture manifest path not valid utf-8"))?,
            "--target",
            "wasm32-wasip2",
            "--release",
            "--target-dir",
            target_dir
                .to_str()
                .ok_or_else(|| anyhow!("fixture target dir not valid utf-8"))?,
        ])
        .status()
        .context("failed to build oauth broker fixture")?;
    if !status.success() {
        anyhow::bail!("failed to build oauth broker fixture");
    }
    Ok(target_dir.join("wasm32-wasip2/release/oauth_broker_component.wasm"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir")
        .parent()
        .expect("workspace dir")
        .to_path_buf()
}
