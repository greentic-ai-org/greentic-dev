#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use greentic_secrets::{Result as SecretResult, SecretError, SecretsManager};
use greentic_types::FlowId;
use greentic_types::flow::{Flow, Node, Routing};
use serde_json::Value as JsonValue;
use serde_json::json;
use zip::ZipArchive;

pub struct ExecResult {
    pub output: JsonValue,
    pub trace: Vec<StepTrace>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct StepTrace {
    pub node_id: String,
    pub component: String,
    pub status: String,
    pub payload: JsonValue,
}

#[derive(Clone)]
pub struct ExecOptions {
    pub offline: bool,
    pub external_enabled: bool,
    pub mock_external: bool,
    pub mock_external_payload: JsonValue,
    pub secrets: Arc<dyn SecretsManager>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            offline: false,
            external_enabled: true,
            mock_external: false,
            mock_external_payload: json!({
                "mocked": true,
                "source": "fixture",
                "data": { "value": 1 }
            }),
            secrets: Arc::new(MemorySecrets::new()),
        }
    }
}

#[derive(Clone, Default)]
pub struct MemorySecrets {
    inner: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemorySecrets {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert_str(&self, key: &str, value: &str) {
        let mut guard = self.inner.lock().expect("mutex poisoned");
        guard.insert(key.to_string(), value.as_bytes().to_vec());
    }
}

#[async_trait]
impl SecretsManager for MemorySecrets {
    async fn read(&self, path: &str) -> SecretResult<Vec<u8>> {
        let guard = self.inner.lock().expect("mutex poisoned");
        guard
            .get(path)
            .cloned()
            .ok_or_else(|| SecretError::NotFound(path.to_string()))
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> SecretResult<()> {
        let mut guard = self.inner.lock().expect("mutex poisoned");
        guard.insert(path.to_string(), bytes.to_vec());
        Ok(())
    }

    async fn delete(&self, path: &str) -> SecretResult<()> {
        let mut guard = self.inner.lock().expect("mutex poisoned");
        guard.remove(path);
        Ok(())
    }
}

pub fn execute_flow_from_pack(
    pack_bytes: &[u8],
    flow_id: &str,
    input: JsonValue,
) -> Result<ExecResult> {
    execute_flow_from_pack_with_options(pack_bytes, flow_id, input, &ExecOptions::default())
}

pub fn execute_flow_from_pack_with_options(
    pack_bytes: &[u8],
    flow_id: &str,
    input: JsonValue,
    options: &ExecOptions,
) -> Result<ExecResult> {
    let mut archive =
        ZipArchive::new(std::io::Cursor::new(pack_bytes)).context("failed to open pack archive")?;
    let mut manifest_bytes = Vec::new();
    archive
        .by_name("manifest.cbor")
        .context("manifest.cbor missing")?
        .read_to_end(&mut manifest_bytes)
        .context("failed to read manifest.cbor")?;
    let manifest: greentic_types::PackManifest =
        greentic_types::decode_pack_manifest(&manifest_bytes).context("decode pack manifest")?;
    let flow_id_parsed: FlowId = flow_id.parse().context("invalid flow id")?;
    let flow = manifest
        .flows
        .iter()
        .find(|f| f.id == flow_id_parsed)
        .ok_or_else(|| anyhow!("flow `{flow_id}` not found in pack"))?;
    run_flow(&flow.flow, input, options)
}

fn run_flow(flow: &Flow, input: JsonValue, options: &ExecOptions) -> Result<ExecResult> {
    let nodes: HashMap<_, _> = flow
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node.clone()))
        .collect();
    let mut current = flow
        .ingress()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| anyhow!("flow has no ingress"))?;
    let mut payload = input;
    let mut trace = Vec::new();
    let mut last_status = String::from("ok");

    loop {
        let Some(node) = nodes.get(&current) else {
            bail!("node `{current}` missing");
        };
        let (next_status, next_payload) = exec_node(node, &payload, options)?;
        trace.push(StepTrace {
            node_id: node.id.as_str().to_string(),
            component: node.component.id.as_str().to_string(),
            status: next_status.clone(),
            payload: next_payload.clone(),
        });
        payload = next_payload;
        let status = next_status;
        if status == "error" || last_status == "error" {
            last_status = "error".to_string();
        } else {
            last_status = status.clone();
        }
        current = match &node.routing {
            Routing::Next { node_id } => node_id.clone(),
            Routing::Branch { on_status, default } => {
                if let Some(dest) = on_status.get(&status) {
                    dest.clone()
                } else if let Some(def) = default {
                    def.clone()
                } else {
                    bail!("no branch for status `{status}`");
                }
            }
            Routing::End => break,
            Routing::Reply => break,
            Routing::Custom(_) => break,
        };
    }

    Ok(ExecResult {
        output: payload,
        trace,
        status: last_status,
    })
}

fn exec_node(
    node: &Node,
    payload: &JsonValue,
    options: &ExecOptions,
) -> Result<(String, JsonValue)> {
    let component = node.component.id.as_str();
    match component {
        "component.start" => Ok(("ok".into(), payload.clone())),
        "component.tool.fixed" => {
            if payload
                .get("fail")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                Ok((
                    "error".into(),
                    json!({
                        "error": "tool_failed",
                        "input": payload
                    }),
                ))
            } else {
                Ok((
                    "ok".into(),
                    json!({
                        "query": payload.get("query").cloned().unwrap_or(JsonValue::Null),
                        "result": "fixed",
                        "constant": 42
                    }),
                ))
            }
        }
        "component.template" => {
            let result_value = payload.get("result").cloned().unwrap_or(JsonValue::Null);
            let result = result_value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&result_value).unwrap_or_default());
            Ok((
                "ok".into(),
                json!({
                    "answer": format!("Result: {result}"),
                    "source": "template",
                    "input": payload
                }),
            ))
        }
        "component.error.map" => Ok((
            "ok".into(),
            json!({
                "message": "A friendly error occurred",
                "details": payload
            }),
        )),
        "component.tool.secret" => {
            let secret = read_secret(options, "API_KEY")?;
            match secret {
                None => Ok((
                    "error".into(),
                    json!({
                        "error": "missing_secret",
                        "key": "API_KEY",
                        "secret_lookup": {
                            "key": "API_KEY",
                            "status": "missing"
                        }
                    }),
                )),
                Some(bytes) => {
                    let prefix = String::from_utf8_lossy(&bytes);
                    let prefix = prefix.chars().take(3).collect::<String>();
                    Ok((
                        "ok".into(),
                        json!({
                            "has_key": true,
                            "prefix": prefix,
                            "secret_lookup": {
                                "key": "API_KEY",
                                "status": "found"
                            }
                        }),
                    ))
                }
            }
        }
        "component.tool.external" => {
            if options.offline || !options.external_enabled {
                return Ok((
                    "error".into(),
                    json!({
                        "error": "external_blocked",
                        "policy": {
                            "offline": options.offline,
                            "external_enabled": options.external_enabled,
                            "mock_external": options.mock_external,
                        },
                        "policy_status": "blocked_by_policy"
                    }),
                ));
            }
            if options.mock_external {
                return Ok((
                    "ok".into(),
                    json!({
                        "policy_status": "mocked_external",
                        "policy": {
                            "offline": options.offline,
                            "external_enabled": options.external_enabled,
                            "mock_external": options.mock_external,
                        },
                        "result": options.mock_external_payload,
                    }),
                ));
            }
            Ok((
                "error".into(),
                json!({
                    "error": "real_external_not_supported_in_tests",
                    "policy_status": "blocked_by_policy",
                    "policy": {
                        "offline": options.offline,
                        "external_enabled": options.external_enabled,
                        "mock_external": options.mock_external,
                    }
                }),
            ))
        }
        _ => bail!("unknown component `{component}`"),
    }
}

fn read_secret(options: &ExecOptions, key: &str) -> Result<Option<Vec<u8>>> {
    let fut = options.secrets.read(key);
    let handle = tokio::runtime::Handle::try_current();
    let outcome = match handle {
        Ok(handle) => handle.block_on(fut),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(fut)
        }
    };
    match outcome {
        Ok(bytes) => Ok(Some(bytes)),
        Err(SecretError::NotFound(_)) => Ok(None),
        Err(other) => Err(anyhow!(other)),
    }
}
