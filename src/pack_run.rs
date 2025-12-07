use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::bail;
use anyhow::{Context, Result, anyhow};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use greentic_runner::desktop::{
    HttpMock, HttpMockMode, MocksConfig, OtlpHook, Runner, SigningPolicy, ToolsMock,
};
use serde_json::{Value as JsonValue, json};
use serde_yaml_bw as serde_yaml;

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

    let input_value = parse_input(config.input)?;
    let otlp_hook = config.otlp.map(|endpoint| OtlpHook {
        endpoint,
        headers: Vec::new(),
        sample_all: true,
    });
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
    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let rendered =
        serde_json::to_string_pretty(&value).context("failed to render run result JSON")?;
    println!("{rendered}");

    if status == "Failure" || status == "PartialFailure" {
        let err = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("pack run returned failure status");
        bail!("pack run failed: {err}");
    }

    Ok(())
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
