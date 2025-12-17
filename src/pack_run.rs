use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::bail;
use anyhow::{Context, Result, anyhow};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use greentic_runner::desktop::{
    HttpMock, HttpMockMode, MocksConfig, OtlpHook, Runner, SigningPolicy, ToolsMock,
};
use serde_json::{Value as JsonValue, json};
use serde_yaml_bw as serde_yaml;
use zip::ZipArchive;

#[derive(Debug, Clone)]
pub struct PackRunConfig<'a> {
    pub pack_path: &'a Path,
    pub entry: Option<String>,
    pub input: Option<String>,
    pub policy: RunPolicy,
    pub otlp: Option<String>,
    pub allow_hosts: Option<Vec<String>>,
    pub mocks: MockSetting,
    pub artifacts_dir: Option<&'a Path>,
    pub json: bool,
    pub offline: bool,
    pub mock_exec: bool,
    pub allow_external: bool,
    pub mock_external: bool,
    pub mock_external_payload: Option<JsonValue>,
    pub secrets_seed: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy)]
pub enum RunPolicy {
    Strict,
    DevOk,
}

#[derive(Debug, Clone, Copy)]
pub enum MockSetting {
    On,
    Off,
}

pub fn run(config: PackRunConfig<'_>) -> Result<()> {
    if config.mock_exec {
        let input_value = parse_input(config.input.clone())?;
        let rendered = mock_execute_pack(
            config.pack_path,
            config.entry.as_deref().unwrap_or("default"),
            &input_value,
            config.offline,
            config.allow_external,
            config.mock_external,
            config
                .mock_external_payload
                .clone()
                .unwrap_or_else(|| json!({ "mocked": true })),
            config.secrets_seed,
        )?;
        let mut rendered = rendered;
        if let Some(map) = rendered.as_object_mut() {
            map.insert("exec_mode".to_string(), json!("mock"));
        }
        if config.json {
            println!(
                "{}",
                serde_json::to_string(&rendered).context("failed to render mock exec json")?
            );
        } else {
            println!("{}", serde_json::to_string_pretty(&rendered)?);
        }
        let status = rendered
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if status != "ok" {
            bail!("pack run failed");
        }
        return Ok(());
    }
    // Print runner diagnostics even if the caller did not configure tracing.
    let _ = tracing_subscriber::fmt::try_init();

    // Ensure Wasmtime cache/config paths live inside the workspace so sandboxed runs can create them.
    if std::env::var_os("HOME").is_none() || std::env::var_os("WASMTIME_CACHE_DIR").is_none() {
        let workspace = std::env::current_dir().context("failed to resolve workspace root")?;
        let home = workspace.join(".greentic").join("wasmtime-home");
        let cache_dir = home
            .join("Library")
            .join("Caches")
            .join("BytecodeAlliance.wasmtime");
        let config_dir = home
            .join("Library")
            .join("Application Support")
            .join("wasmtime");
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("failed to create {}", cache_dir.display()))?;
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create {}", config_dir.display()))?;
        // SAFETY: we scope HOME and cache dir to a workspace-local directory to avoid
        // writing outside the sandbox; this only affects the child Wasmtime engine.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("WASMTIME_CACHE_DIR", &cache_dir);
        }
    }

    let input_value = parse_input(config.input.clone())?;
    let otlp_hook = if config.offline {
        None
    } else {
        config.otlp.map(|endpoint| OtlpHook {
            endpoint,
            headers: Vec::new(),
            sample_all: true,
        })
    };

    // Avoid system proxy discovery (reqwest on macOS can panic in sandboxed CI).
    unsafe {
        std::env::set_var("NO_PROXY", "*");
        std::env::set_var("HTTPS_PROXY", "");
        std::env::set_var("HTTP_PROXY", "");
        std::env::set_var("CFNETWORK_DISABLE_SYSTEM_PROXY", "1");
    }

    let allow_hosts = config.allow_hosts.unwrap_or_default();
    let mocks_config = build_mocks_config(config.mocks, allow_hosts)?;

    let artifacts_override = config.artifacts_dir.map(|dir| dir.to_path_buf());
    if let Some(dir) = &artifacts_override {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create artifacts directory {}", dir.display()))?;
    }

    let runner = Runner::new();
    let run_result = runner
        .run_pack_with(config.pack_path, |opts| {
            opts.entry_flow = config.entry.clone();
            opts.input = input_value.clone();
            opts.signing = signing_policy(config.policy);
            if let Some(hook) = otlp_hook.clone() {
                opts.otlp = Some(hook);
            }
            opts.mocks = mocks_config.clone();
            opts.artifacts_dir = artifacts_override.clone();
        })
        .context("pack execution failed")?;

    let value = serde_json::to_value(&run_result).context("failed to render run result JSON")?;
    let mut value = value;
    if let Some(map) = value.as_object_mut() {
        map.insert("exec_mode".to_string(), json!("runtime"));
    }
    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if config.json {
        println!(
            "{}",
            serde_json::to_string(&value).context("failed to render run result JSON")?
        );
    } else {
        let rendered =
            serde_json::to_string_pretty(&value).context("failed to render run result JSON")?;
        println!("{rendered}");
    }

    if status == "Failure" || status == "PartialFailure" {
        let err = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("pack run returned failure status");
        bail!("pack run failed: {err}");
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn mock_execute_pack(
    path: &Path,
    flow_id: &str,
    input: &JsonValue,
    offline: bool,
    allow_external: bool,
    mock_external: bool,
    mock_external_payload: JsonValue,
    secrets_seed: Option<&Path>,
) -> Result<JsonValue> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read pack {}", path.display()))?;
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).context("open pack zip")?;
    let mut manifest_bytes = Vec::new();
    archive
        .by_name("manifest.cbor")
        .context("manifest.cbor missing")?
        .read_to_end(&mut manifest_bytes)
        .context("read manifest")?;
    let manifest: greentic_types::PackManifest =
        greentic_types::decode_pack_manifest(&manifest_bytes).context("decode manifest")?;
    let flow = manifest
        .flows
        .iter()
        .find(|f| f.id.as_str() == flow_id)
        .ok_or_else(|| anyhow!("flow `{flow_id}` not found in pack"))?;
    let mut exec_builder = crate::tests_exec::ExecOptions::builder();
    if let Some(seed_path) = secrets_seed {
        exec_builder = exec_builder
            .load_seed_file(seed_path)
            .context("failed to load secrets seed")?;
    }
    let exec_opts = exec_builder
        .offline(offline)
        .external_enabled(allow_external)
        .mock_external(mock_external)
        .mock_external_payload(mock_external_payload)
        .build()
        .context("build mock exec options")?;
    let exec = crate::tests_exec::execute_with_options(&flow.flow, input, &exec_opts)?;
    Ok(exec)
}

fn parse_input(input: Option<String>) -> Result<JsonValue> {
    if let Some(raw) = input {
        if raw.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&raw).context("failed to parse --input JSON")
    } else {
        Ok(json!({}))
    }
}

fn build_mocks_config(setting: MockSetting, allow_hosts: Vec<String>) -> Result<MocksConfig> {
    let mut config = MocksConfig {
        net_allowlist: allow_hosts
            .into_iter()
            .map(|host| host.trim().to_ascii_lowercase())
            .filter(|host| !host.is_empty())
            .collect(),
        ..MocksConfig::default()
    };

    if matches!(setting, MockSetting::On) {
        config.http = Some(HttpMock {
            record_replay_dir: None,
            mode: HttpMockMode::RecordReplay,
            rewrites: Vec::new(),
        });

        let tools_dir = PathBuf::from(".greentic").join("mocks").join("tools");
        fs::create_dir_all(&tools_dir)
            .with_context(|| format!("failed to create {}", tools_dir.display()))?;
        config.mcp_tools = Some(ToolsMock {
            directory: None,
            script_dir: Some(tools_dir),
            short_circuit: true,
        });
    }

    Ok(config)
}

fn signing_policy(policy: RunPolicy) -> SigningPolicy {
    match policy {
        RunPolicy::Strict => SigningPolicy::Strict,
        RunPolicy::DevOk => SigningPolicy::DevOk,
    }
}

/// Run a config flow and return the final payload as a JSON string.
#[allow(dead_code)]
pub fn run_config_flow(flow_path: &Path) -> Result<String> {
    let source = std::fs::read_to_string(flow_path)
        .with_context(|| format!("failed to read config flow {}", flow_path.display()))?;
    // Validate against embedded schema to catch malformed flows.
    load_and_validate_bundle(&source, Some(flow_path)).context("config flow validation failed")?;

    let doc: serde_yaml::Value = serde_yaml::from_str(&source)
        .with_context(|| format!("invalid YAML in {}", flow_path.display()))?;
    let nodes = doc
        .get("nodes")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| anyhow!("config flow missing nodes map"))?;

    let mut current = nodes
        .iter()
        .next()
        .map(|(k, _)| k.as_str().unwrap_or_default().to_string())
        .ok_or_else(|| anyhow!("config flow has no nodes to execute"))?;
    let mut state: BTreeMap<String, String> = BTreeMap::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let is_tty = io::stdin().is_terminal();

    loop {
        if !visited.insert(current.clone()) {
            bail!("config flow routing loop detected at {}", current);
        }

        let node_val = nodes
            .get(serde_yaml::Value::String(current.clone(), None))
            .ok_or_else(|| anyhow!("node `{current}` not found in config flow"))?;
        let mapping = node_val
            .as_mapping()
            .ok_or_else(|| anyhow!("node `{current}` is not a mapping"))?;

        // questions node
        if let Some(fields) = mapping
            .get(serde_yaml::Value::String("questions".to_string(), None))
            .and_then(|q| {
                q.as_mapping()
                    .and_then(|m| m.get(serde_yaml::Value::String("fields".to_string(), None)))
            })
            .and_then(|v| v.as_sequence())
        {
            for field in fields {
                let Some(field_map) = field.as_mapping() else {
                    continue;
                };
                let id = field_map
                    .get(serde_yaml::Value::String("id".to_string(), None))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
                    continue;
                }
                let prompt = field_map
                    .get(serde_yaml::Value::String("prompt".to_string(), None))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id);
                let default = field_map
                    .get(serde_yaml::Value::String("default".to_string(), None))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let value = if is_tty {
                    print!("{prompt} [{default}]: ");
                    let _ = io::stdout().flush();
                    let mut buf = String::new();
                    io::stdin().read_line(&mut buf).ok();
                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        default.to_string()
                    } else {
                        trimmed.to_string()
                    }
                } else {
                    default.to_string()
                };
                state.insert(id, value);
            }
        }

        // template string path
        if let Some(template) = mapping
            .get(serde_yaml::Value::String("template".to_string(), None))
            .and_then(|v| v.as_str())
        {
            let mut rendered = template.to_string();
            for (k, v) in &state {
                let needle = format!("{{{{state.{k}}}}}");
                rendered = rendered.replace(&needle, v);
            }
            return Ok(rendered);
        }

        // payload with node_id/node
        if let Some(payload) = mapping.get(serde_yaml::Value::String("payload".to_string(), None)) {
            let json_str = serde_json::to_string(&serde_yaml::from_value::<serde_json::Value>(
                payload.clone(),
            )?)
            .context("failed to render config flow payload")?;
            return Ok(json_str);
        }

        // follow routing if present
        if let Some(next) = mapping
            .get(serde_yaml::Value::String("routing".to_string(), None))
            .and_then(|r| r.as_sequence())
            .and_then(|seq| seq.first())
            .and_then(|entry| {
                entry
                    .as_mapping()
                    .and_then(|m| m.get(serde_yaml::Value::String("to".to_string(), None)))
                    .and_then(|v| v.as_str())
            })
        {
            current = next.to_string();
            continue;
        }

        bail!("config flow ended without producing template or payload");
    }
}
