use anyhow::{Context, Result};
use hex::encode;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use url::Url;

pub trait MockEventSink: Send + Sync {
    fn on_mock_event(&self, capability: &str, provider: &str, payload: &Value);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MocksConfig {
    pub http: Option<HttpMock>,
    pub secrets: Option<SecretsMock>,
    pub kv: Option<KvMock>,
    pub telemetry: Option<TelemetryMock>,
    pub mcp_tools: Option<ToolsMock>,
    pub time: Option<TimeMock>,
    #[serde(default)]
    pub net_allowlist: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HttpMock {
    pub record_replay_dir: Option<PathBuf>,
    pub mode: HttpMockMode,
    #[serde(default)]
    pub rewrites: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum HttpMockMode {
    #[default]
    Off,
    Replay,
    Record,
    RecordReplay,
    FailOnMiss,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SecretsMock {
    pub map: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KvMock;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelemetryMock;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolsMock {
    pub directory: Option<PathBuf>,
    pub script_dir: Option<PathBuf>,
    pub short_circuit: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimeMock;

pub struct MockLayer {
    config: MocksConfig,
    http: Option<HttpMockRuntime>,
    sinks: Mutex<Vec<Weak<dyn MockEventSink>>>,
    net_allowlist: HashSet<String>,
}

impl MockLayer {
    pub fn new(config: MocksConfig, run_dir: &Path) -> Result<Self> {
        let http = if let Some(http_cfg) = &config.http {
            match http_cfg.mode {
                HttpMockMode::Off => None,
                _ => Some(HttpMockRuntime::new(http_cfg, run_dir)?),
            }
        } else {
            None
        };
        let net_allowlist = config
            .net_allowlist
            .iter()
            .map(|value| value.to_ascii_lowercase())
            .collect();
        Ok(Self {
            config,
            http,
            sinks: Mutex::new(Vec::new()),
            net_allowlist,
        })
    }

    pub fn register_sink(&self, sink: Arc<dyn MockEventSink>) {
        let mut guard = self.sinks.lock();
        guard.retain(|entry| entry.upgrade().is_some());
        guard.push(Arc::downgrade(&sink));
    }

    fn emit_event(&self, capability: &str, provider: &str, payload: Value) {
        let mut guard = self.sinks.lock();
        guard.retain(|entry| entry.upgrade().is_some());
        for weak in guard.iter() {
            if let Some(strong) = weak.upgrade() {
                strong.on_mock_event(capability, provider, &payload);
            }
        }
    }

    pub fn secrets_lookup(&self, key: &str) -> Option<String> {
        let secrets = self.config.secrets.as_ref()?;
        secrets.map.get(key).map(|value| {
            self.emit_event("secrets", "mock", json!({ "key": key, "source": "map" }));
            value.clone()
        })
    }

    pub fn telemetry_drain(&self, fields: &[(&str, &str)]) -> bool {
        if self.config.telemetry.is_none() {
            return false;
        }
        let entries = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>();
        self.emit_event("telemetry", "mock", json!({ "fields": entries }));
        true
    }

    pub fn tool_short_circuit(&self, tool: &str, action: &str) -> Option<Result<Value>> {
        let tools = self.config.mcp_tools.as_ref()?;
        if !tools.short_circuit {
            return None;
        }
        let script_dir = tools.script_dir.as_ref()?;
        let filename = format!("{}__{}.json", sanitize(tool), sanitize(action));
        let path = script_dir.join(filename);
        let body = fs::read_to_string(&path)
            .with_context(|| format!("failed to read mock script {}", path.display()));
        let result = body.and_then(|text| {
            serde_json::from_str(&text)
                .with_context(|| format!("mock script {} is not valid json", path.display()))
        });
        Some(match result {
            Ok(value) => {
                self.emit_event(
                    "tools",
                    "mock",
                    json!({ "tool": tool, "action": action, "script": path }),
                );
                Ok(value)
            }
            Err(err) => Err(err),
        })
    }

    pub fn http_begin(&self, request: &HttpMockRequest) -> HttpDecision {
        let runtime = match &self.http {
            Some(runtime) => runtime,
            None => return HttpDecision::Passthrough { record: false },
        };

        if let Some(response) = runtime.replay(request) {
            self.emit_event(
                "http",
                "mock",
                json!({
                    "url": request.url,
                    "method": request.method,
                    "mode": "replay"
                }),
            );
            return HttpDecision::Mock(response);
        }

        if !self.allow_host(&request.url) {
            return HttpDecision::Deny(format!("host {} not present in allowlist", request.host));
        }

        if runtime.should_record() {
            HttpDecision::Passthrough { record: true }
        } else {
            match runtime.mode {
                HttpMockMode::RecordReplay | HttpMockMode::Record => {
                    HttpDecision::Passthrough { record: false }
                }
                _ => HttpDecision::Deny("no recorded response".into()),
            }
        }
    }

    pub fn http_record(&self, request: &HttpMockRequest, response: &HttpMockResponse) {
        if let Some(runtime) = &self.http
            && runtime.record(request, response).is_ok()
        {
            self.emit_event(
                "http",
                "mock",
                json!({
                    "url": request.url,
                    "method": request.method,
                    "mode": "record"
                }),
            );
        }
    }

    fn allow_host(&self, url: &str) -> bool {
        if self.net_allowlist.is_empty() {
            return false;
        }
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .map(|host| self.net_allowlist.contains(&host))
            .unwrap_or(false)
    }
}

pub enum HttpDecision {
    Mock(HttpMockResponse),
    Deny(String),
    Passthrough { record: bool },
}

pub struct HttpMockRequest {
    pub method: String,
    pub url: String,
    pub host: String,
    pub fingerprint: String,
}

impl HttpMockRequest {
    pub fn new(method: &str, url: &str, body: Option<&[u8]>) -> Result<Self> {
        let parsed = Url::parse(url).with_context(|| format!("invalid url {url}"))?;
        let host = parsed
            .host_str()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(url.as_bytes());
        if let Some(body) = body {
            hasher.update(body);
        }
        let fingerprint = encode(hasher.finalize());
        Ok(Self {
            method: method.to_string(),
            url: url.to_string(),
            host,
            fingerprint,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpMockResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

impl HttpMockResponse {
    pub fn new(status: u16, headers: BTreeMap<String, String>, body: Option<String>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }
}

struct HttpMockRuntime {
    mode: HttpMockMode,
    dir: Option<PathBuf>,
    entries: Mutex<HashMap<String, HttpMockResponse>>,
}

impl HttpMockRuntime {
    fn new(config: &HttpMock, run_dir: &Path) -> Result<Self> {
        let dir = config
            .record_replay_dir
            .clone()
            .or_else(|| Some(run_dir.join("cassettes/http")));
        if let Some(path) = dir.as_ref() {
            let _ = fs::create_dir_all(path);
        }
        let entries = Mutex::new(Self::load_entries(dir.as_ref())?);
        Ok(Self {
            mode: config.mode.clone(),
            dir,
            entries,
        })
    }

    fn load_entries(dir: Option<&PathBuf>) -> Result<HashMap<String, HttpMockResponse>> {
        let mut map = HashMap::new();
        if let Some(dir) = dir
            && dir.exists()
        {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let bytes = fs::read(&path)?;
                let resp: HttpMockResponse = serde_json::from_slice(&bytes)?;
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    map.insert(stem.to_string(), resp);
                }
            }
        }
        Ok(map)
    }

    fn replay(&self, req: &HttpMockRequest) -> Option<HttpMockResponse> {
        let guard = self.entries.lock();
        guard.get(&req.fingerprint).cloned()
    }

    fn should_record(&self) -> bool {
        matches!(self.mode, HttpMockMode::Record | HttpMockMode::RecordReplay)
    }

    fn record(&self, req: &HttpMockRequest, resp: &HttpMockResponse) -> Result<()> {
        if !self.should_record() {
            return Ok(());
        }
        let dir = match &self.dir {
            Some(dir) => dir,
            None => return Ok(()),
        };
        let path = dir.join(format!("{}.json", &req.fingerprint));
        let mut file = fs::File::create(&path)
            .with_context(|| format!("failed to write {}", path.display()))?;
        let body = serde_json::to_vec_pretty(resp)?;
        file.write_all(&body)?;
        self.entries
            .lock()
            .insert(req.fingerprint.clone(), resp.clone());
        Ok(())
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
