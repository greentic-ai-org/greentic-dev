use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use greentic_secrets::{Result as SecretResult, SecretError, SecretsManager};
use greentic_types::flow::{Flow, Node, Routing};
use serde_json::{Value as JsonValue, json};

#[derive(Clone)]
pub struct ExecOptions {
    pub offline: bool,
    pub external_enabled: bool,
    pub mock_external: bool,
    pub mock_external_payload: JsonValue,
    pub secrets: Arc<dyn SecretsManager>,
}

impl ExecOptions {
    pub fn builder() -> ExecOptionsBuilder {
        ExecOptionsBuilder::default()
    }
}

#[derive(Default)]
pub struct ExecOptionsBuilder {
    offline: bool,
    external_enabled: bool,
    mock_external: bool,
    mock_external_payload: JsonValue,
    seeds: HashMap<String, Vec<u8>>,
}

impl ExecOptionsBuilder {
    pub fn offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    pub fn external_enabled(mut self, enabled: bool) -> Self {
        self.external_enabled = enabled;
        self
    }

    pub fn mock_external(mut self, enabled: bool) -> Self {
        self.mock_external = enabled;
        self
    }

    pub fn mock_external_payload(mut self, payload: JsonValue) -> Self {
        self.mock_external_payload = payload;
        self
    }

    pub fn load_seed_file(mut self, path: &std::path::Path) -> Result<Self> {
        let seeds = crate::secrets_seed::load_seed_file(path)?;
        self.seeds.extend(seeds);
        Ok(self)
    }

    pub fn seed_entry(mut self, key: &str, bytes: &[u8]) -> Self {
        self.seeds.insert(key.to_string(), bytes.to_vec());
        self
    }

    pub fn build(self) -> Result<ExecOptions> {
        let secrets = MemorySecrets::from_seed_map(&self.seeds);
        Ok(ExecOptions {
            offline: self.offline,
            external_enabled: self.external_enabled,
            mock_external: self.mock_external,
            mock_external_payload: self.mock_external_payload,
            secrets: Arc::new(secrets),
        })
    }
}

#[derive(Clone, Default)]
pub struct MemorySecrets {
    inner: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_seed_map(map: &HashMap<String, Vec<u8>>) -> Self {
        let mgr = Self::new();
        {
            let mut guard = mgr.inner.lock().expect("mutex poisoned");
            for (k, v) in map {
                guard.insert(k.clone(), v.clone());
            }
        }
        mgr
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

pub fn execute(flow: &Flow, input: &JsonValue) -> Result<JsonValue> {
    let opts = ExecOptionsBuilder::default().build()?;
    execute_with_options(flow, input, &opts)
}

pub fn execute_with_options(
    flow: &Flow,
    input: &JsonValue,
    opts: &ExecOptions,
) -> Result<JsonValue> {
    let nodes: HashMap<_, _> = flow
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node.clone()))
        .collect();
    let mut current = flow
        .ingress()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| anyhow!("flow has no ingress"))?;
    let mut payload = input.clone();
    let mut trace = Vec::new();
    let mut last_status = String::from("ok");

    loop {
        let Some(node) = nodes.get(&current) else {
            bail!("node `{current}` missing");
        };
        let (status, next_payload) = exec_node(node, &payload, opts)?;
        trace.push(json!({
            "node_id": node.id.as_str(),
            "component": node.component.id.as_str(),
            "status": status,
            "payload": next_payload
        }));
        payload = next_payload;
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

    Ok(json!({
        "status": last_status,
        "output": payload,
        "trace": trace,
    }))
}

fn exec_node(node: &Node, payload: &JsonValue, opts: &ExecOptions) -> Result<(String, JsonValue)> {
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
            let secret = read_secret(opts, "API_KEY")?;
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
            if opts.offline || !opts.external_enabled {
                return Ok((
                    "error".into(),
                    json!({
                        "error": "external_blocked",
                        "policy": {
                            "offline": opts.offline,
                            "external_enabled": opts.external_enabled,
                            "mock_external": opts.mock_external,
                        },
                        "policy_status": "blocked_by_policy"
                    }),
                ));
            }
            if opts.mock_external {
                return Ok((
                    "ok".into(),
                    json!({
                        "policy_status": "mocked_external",
                        "policy": {
                            "offline": opts.offline,
                            "external_enabled": opts.external_enabled,
                            "mock_external": opts.mock_external,
                        },
                        "result": opts.mock_external_payload,
                    }),
                ));
            }
            Ok((
                "error".into(),
                json!({
                    "error": "real_external_not_supported_in_tests",
                    "policy_status": "blocked_by_policy",
                    "policy": {
                        "offline": opts.offline,
                        "external_enabled": opts.external_enabled,
                        "mock_external": opts.mock_external,
                    }
                }),
            ))
        }
        _ => bail!("unknown component `{component}`"),
    }
}

fn read_secret(opts: &ExecOptions, key: &str) -> Result<Option<Vec<u8>>> {
    let fut = opts.secrets.read(key);
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
