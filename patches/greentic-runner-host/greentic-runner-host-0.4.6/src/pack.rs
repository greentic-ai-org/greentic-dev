use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use crate::component_api::component::greentic::component::control::Host as ComponentControlHost;
use crate::component_api::{
    ComponentPre, control, node::ExecCtx as ComponentExecCtx, node::InvokeResult, node::NodeError,
};
use crate::imports::register_all;
use crate::oauth::{OAuthBrokerConfig, OAuthBrokerHost, OAuthHostContext};
use crate::runtime_wasmtime::{Component, Engine, Linker, ResourceTable, WasmResult};
use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::ir::{FlowIR, NodeIR, RouteIR};
use greentic_flow::model::FlowDoc;
use greentic_interfaces_host::host_import::v0_2 as host_import_v0_2;
use greentic_interfaces_host::host_import::v0_2::greentic::host_import::imports::{
    HttpRequest as LegacyHttpRequest, HttpResponse as LegacyHttpResponse,
    IfaceError as LegacyIfaceError, TenantCtx as LegacyTenantCtx,
};
use greentic_interfaces_host::host_import::v0_6::{
    self as host_import_v0_6, iface_types, state, types,
};
use greentic_pack::reader::{SigningPolicy, open_pack};
use greentic_session::SessionKey as StoreSessionKey;
use greentic_types::{
    EnvId, FlowId, SessionCursor as StoreSessionCursor, SessionData, StateKey as StoreStateKey,
    TeamId, TenantCtx as TypesTenantCtx, TenantId, UserId,
};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use reqwest::blocking::Client as BlockingClient;
use runner_core::normalize_under_root;
use serde::{Deserialize, Serialize};
use serde_cbor;
use serde_json::{self, Value};
use serde_yaml_bw as serde_yaml;
use tokio::fs;
use wasmparser::{Parser, Payload};
use wasmtime::StoreContextMut;
use zip::ZipArchive;

use crate::runner::mocks::{HttpDecision, HttpMockRequest, HttpMockResponse, MockLayer};

use crate::config::HostConfig;
use crate::secrets::{DynSecretsManager, read_secret_blocking};
use crate::storage::state::STATE_PREFIX;
use crate::storage::{DynSessionStore, DynStateStore};
use crate::verify;
use crate::wasi::RunnerWasiPolicy;
use tracing::warn;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

#[allow(dead_code)]
pub struct PackRuntime {
    /// Component artifact path (wasm file).
    path: PathBuf,
    /// Optional archive (.gtpack) used to load flows/manifests.
    archive_path: Option<PathBuf>,
    config: Arc<HostConfig>,
    engine: Engine,
    metadata: PackMetadata,
    manifest: Option<greentic_pack::builder::PackManifest>,
    mocks: Option<Arc<MockLayer>>,
    flows: Option<PackFlows>,
    components: HashMap<String, PackComponent>,
    http_client: Arc<BlockingClient>,
    pre_cache: Mutex<HashMap<String, ComponentPre<ComponentState>>>,
    session_store: Option<DynSessionStore>,
    state_store: Option<DynStateStore>,
    wasi_policy: Arc<RunnerWasiPolicy>,
    secrets: DynSecretsManager,
    oauth_config: Option<OAuthBrokerConfig>,
}

struct PackComponent {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    version: String,
    component: Component,
}

fn build_blocking_client() -> BlockingClient {
    std::thread::spawn(|| BlockingClient::builder().build().expect("blocking client"))
        .join()
        .expect("client build thread panicked")
}

fn normalize_pack_path(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let (root, candidate) = if path.is_absolute() {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("pack path {} has no parent", path.display()))?;
        let root = parent
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", parent.display()))?;
        let file = path
            .file_name()
            .ok_or_else(|| anyhow!("pack path {} has no file name", path.display()))?;
        (root, PathBuf::from(file))
    } else {
        let cwd = std::env::current_dir().context("failed to resolve current directory")?;
        let base = if let Some(parent) = path.parent() {
            cwd.join(parent)
        } else {
            cwd
        };
        let root = base
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", base.display()))?;
        let file = path
            .file_name()
            .ok_or_else(|| anyhow!("pack path {} has no file name", path.display()))?;
        (root, PathBuf::from(file))
    };
    let safe = normalize_under_root(&root, &candidate)?;
    Ok((root, safe))
}

static HTTP_CLIENT: Lazy<Arc<BlockingClient>> = Lazy::new(|| Arc::new(build_blocking_client()));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub flow_type: String,
    pub profile: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
}

pub struct HostState {
    config: Arc<HostConfig>,
    http_client: Arc<BlockingClient>,
    default_env: String,
    session_store: Option<DynSessionStore>,
    state_store: Option<DynStateStore>,
    mocks: Option<Arc<MockLayer>>,
    secrets: DynSecretsManager,
    oauth_config: Option<OAuthBrokerConfig>,
    oauth_host: OAuthBrokerHost,
}

impl HostState {
    #[allow(clippy::default_constructed_unit_structs)]
    pub fn new(
        config: Arc<HostConfig>,
        http_client: Arc<BlockingClient>,
        mocks: Option<Arc<MockLayer>>,
        session_store: Option<DynSessionStore>,
        state_store: Option<DynStateStore>,
        secrets: DynSecretsManager,
        oauth_config: Option<OAuthBrokerConfig>,
    ) -> Result<Self> {
        let default_env = std::env::var("GREENTIC_ENV").unwrap_or_else(|_| "local".to_string());
        Ok(Self {
            config,
            http_client,
            default_env,
            session_store,
            state_store,
            mocks,
            secrets,
            oauth_config,
            oauth_host: OAuthBrokerHost::default(),
        })
    }

    pub fn get_secret(&self, key: &str) -> Result<String> {
        if !self.config.secrets_policy.is_allowed(key) {
            bail!("secret {key} is not permitted by bindings policy");
        }
        if let Some(mock) = &self.mocks
            && let Some(value) = mock.secrets_lookup(key)
        {
            return Ok(value);
        }
        let bytes = read_secret_blocking(&self.secrets, key)
            .context("failed to read secret from manager")?;
        let value = String::from_utf8(bytes).context("secret value is not valid UTF-8")?;
        Ok(value)
    }

    fn tenant_ctx_from_v6(&self, ctx: Option<types::TenantCtx>) -> Result<TypesTenantCtx> {
        let tenant_raw = ctx
            .as_ref()
            .map(|ctx| ctx.tenant.clone())
            .unwrap_or_else(|| self.config.tenant.clone());
        let tenant_id = TenantId::from_str(&tenant_raw)
            .with_context(|| format!("invalid tenant id `{tenant_raw}`"))?;
        let env_id = EnvId::from_str(&self.default_env)
            .unwrap_or_else(|_| EnvId::from_str("local").expect("default env must be valid"));
        let mut tenant_ctx = TypesTenantCtx::new(env_id, tenant_id);
        if let Some(ctx) = ctx {
            if let Some(team) = ctx.team {
                let team_id =
                    TeamId::from_str(&team).with_context(|| format!("invalid team id `{team}`"))?;
                tenant_ctx = tenant_ctx.with_team(Some(team_id));
            }
            if let Some(user) = ctx.user {
                let user_id =
                    UserId::from_str(&user).with_context(|| format!("invalid user id `{user}`"))?;
                tenant_ctx = tenant_ctx.with_user(Some(user_id));
            }
            if let Some(flow) = ctx.flow_id {
                tenant_ctx = tenant_ctx.with_flow(flow);
            }
            if let Some(node) = ctx.node_id {
                tenant_ctx = tenant_ctx.with_node(node);
            }
            if let Some(provider) = ctx.provider_id {
                tenant_ctx = tenant_ctx.with_provider(provider);
            }
            if let Some(session) = ctx.session_id {
                tenant_ctx = tenant_ctx.with_session(session);
            }
            tenant_ctx.trace_id = ctx.trace_id;
        }
        Ok(tenant_ctx)
    }

    fn session_store_handle(&self) -> Result<DynSessionStore, types::IfaceError> {
        self.session_store
            .as_ref()
            .cloned()
            .ok_or(types::IfaceError::Unavailable)
    }

    fn state_store_handle(&self) -> Result<DynStateStore, types::IfaceError> {
        self.state_store
            .as_ref()
            .cloned()
            .ok_or(types::IfaceError::Unavailable)
    }

    fn ensure_user(ctx: &TypesTenantCtx) -> Result<UserId, types::IfaceError> {
        ctx.user_id
            .clone()
            .or_else(|| ctx.user.clone())
            .ok_or(types::IfaceError::InvalidArg)
    }

    fn ensure_flow(ctx: &TypesTenantCtx) -> Result<FlowId, types::IfaceError> {
        let flow = ctx.flow_id().ok_or(types::IfaceError::InvalidArg)?;
        FlowId::from_str(flow).map_err(|_| types::IfaceError::InvalidArg)
    }

    fn cursor_from_iface(cursor: iface_types::SessionCursor) -> StoreSessionCursor {
        let mut store_cursor = StoreSessionCursor::new(cursor.node_pointer);
        if let Some(reason) = cursor.wait_reason {
            store_cursor = store_cursor.with_wait_reason(reason);
        }
        if let Some(marker) = cursor.outbox_marker {
            store_cursor = store_cursor.with_outbox_marker(marker);
        }
        store_cursor
    }
}

pub struct ComponentState {
    host: HostState,
    wasi_ctx: WasiCtx,
    resource_table: ResourceTable,
}

impl ComponentState {
    pub fn new(host: HostState, policy: Arc<RunnerWasiPolicy>) -> Result<Self> {
        let wasi_ctx = policy
            .instantiate()
            .context("failed to build WASI context")?;
        Ok(Self {
            host,
            wasi_ctx,
            resource_table: ResourceTable::new(),
        })
    }

    fn host_mut(&mut self) -> &mut HostState {
        &mut self.host
    }
}

impl control::Host for ComponentState {
    fn should_cancel(&mut self) -> bool {
        false
    }

    fn yield_now(&mut self) {
        // no-op cooperative yield
    }
}

fn add_component_control_to_linker(linker: &mut Linker<ComponentState>) -> wasmtime::Result<()> {
    let mut inst = linker.instance("greentic:component/control@0.4.0")?;
    inst.func_wrap(
        "should-cancel",
        |mut caller: StoreContextMut<'_, ComponentState>, (): ()| {
            let host = caller.data_mut();
            Ok((ComponentControlHost::should_cancel(host),))
        },
    )?;
    inst.func_wrap(
        "yield-now",
        |mut caller: StoreContextMut<'_, ComponentState>, (): ()| {
            let host = caller.data_mut();
            ComponentControlHost::yield_now(host);
            Ok(())
        },
    )?;
    Ok(())
}

impl OAuthHostContext for ComponentState {
    fn tenant_id(&self) -> &str {
        &self.host.config.tenant
    }

    fn env(&self) -> &str {
        &self.host.default_env
    }

    fn oauth_broker_host(&mut self) -> &mut OAuthBrokerHost {
        &mut self.host.oauth_host
    }

    fn oauth_config(&self) -> Option<&OAuthBrokerConfig> {
        self.host.oauth_config.as_ref()
    }
}

impl WasiView for ComponentState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

#[allow(unsafe_code)]
unsafe impl Send for ComponentState {}
#[allow(unsafe_code)]
unsafe impl Sync for ComponentState {}

impl host_import_v0_6::HostImports for ComponentState {
    fn secrets_get(
        &mut self,
        key: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        host_import_v0_6::HostImports::secrets_get(self.host_mut(), key, ctx)
    }

    fn telemetry_emit(
        &mut self,
        span_json: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<()> {
        host_import_v0_6::HostImports::telemetry_emit(self.host_mut(), span_json, ctx)
    }

    fn http_fetch(
        &mut self,
        req: host_import_v0_6::http::HttpRequest,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<host_import_v0_6::http::HttpResponse, types::IfaceError>> {
        host_import_v0_6::HostImports::http_fetch(self.host_mut(), req, ctx)
    }

    fn mcp_exec(
        &mut self,
        component: String,
        action: String,
        args_json: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        host_import_v0_6::HostImports::mcp_exec(self.host_mut(), component, action, args_json, ctx)
    }

    fn state_get(
        &mut self,
        key: iface_types::StateKey,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        host_import_v0_6::HostImports::state_get(self.host_mut(), key, ctx)
    }

    fn state_set(
        &mut self,
        key: iface_types::StateKey,
        value_json: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<state::OpAck, types::IfaceError>> {
        host_import_v0_6::HostImports::state_set(self.host_mut(), key, value_json, ctx)
    }

    fn session_update(
        &mut self,
        cursor: iface_types::SessionCursor,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        host_import_v0_6::HostImports::session_update(self.host_mut(), cursor, ctx)
    }
}

impl host_import_v0_2::HostImports for ComponentState {
    fn secrets_get(
        &mut self,
        key: String,
        ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<Result<String, LegacyIfaceError>> {
        host_import_v0_2::HostImports::secrets_get(self.host_mut(), key, ctx)
    }

    fn telemetry_emit(
        &mut self,
        span_json: String,
        ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<()> {
        host_import_v0_2::HostImports::telemetry_emit(self.host_mut(), span_json, ctx)
    }

    fn tool_invoke(
        &mut self,
        tool: String,
        action: String,
        args_json: String,
        ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<Result<String, LegacyIfaceError>> {
        host_import_v0_2::HostImports::tool_invoke(self.host_mut(), tool, action, args_json, ctx)
    }

    fn http_fetch(
        &mut self,
        req: host_import_v0_2::greentic::host_import::imports::HttpRequest,
        ctx: Option<host_import_v0_2::greentic::host_import::imports::TenantCtx>,
    ) -> WasmResult<
        Result<
            host_import_v0_2::greentic::host_import::imports::HttpResponse,
            host_import_v0_2::greentic::host_import::imports::IfaceError,
        >,
    > {
        host_import_v0_2::HostImports::http_fetch(self.host_mut(), req, ctx)
    }
}

impl host_import_v0_6::HostImports for HostState {
    fn secrets_get(
        &mut self,
        key: String,
        _ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        Ok(self.get_secret(&key).map_err(|err| {
            tracing::warn!(secret = %key, error = %err, "secret lookup denied");
            types::IfaceError::Denied
        }))
    }

    fn telemetry_emit(
        &mut self,
        span_json: String,
        _ctx: Option<types::TenantCtx>,
    ) -> WasmResult<()> {
        if let Some(mock) = &self.mocks
            && mock.telemetry_drain(&[("span_json", span_json.as_str())])
        {
            return Ok(());
        }
        tracing::info!(span = %span_json, "telemetry emit from pack");
        Ok(())
    }

    fn http_fetch(
        &mut self,
        req: host_import_v0_6::http::HttpRequest,
        _ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<host_import_v0_6::http::HttpResponse, types::IfaceError>> {
        let legacy_req = LegacyHttpRequest {
            method: req.method,
            url: req.url,
            headers_json: req.headers_json,
            body: req.body,
        };
        match host_import_v0_2::HostImports::http_fetch(self, legacy_req, None)? {
            Ok(resp) => Ok(Ok(host_import_v0_6::http::HttpResponse {
                status: resp.status,
                headers_json: resp.headers_json,
                body: resp.body,
            })),
            Err(err) => Ok(Err(map_legacy_error(err))),
        }
    }

    fn mcp_exec(
        &mut self,
        component: String,
        action: String,
        args_json: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        let _ = (component, action, args_json, ctx);
        tracing::warn!("mcp.exec requested but MCP bridge is removed; returning unavailable");
        Ok(Err(types::IfaceError::Unavailable))
    }

    fn state_get(
        &mut self,
        key: iface_types::StateKey,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        let store = match self.state_store_handle() {
            Ok(store) => store,
            Err(err) => return Ok(Err(err)),
        };
        let tenant_ctx = match self.tenant_ctx_from_v6(ctx) {
            Ok(ctx) => ctx,
            Err(err) => {
                tracing::warn!(error = %err, "invalid tenant context for state.get");
                return Ok(Err(types::IfaceError::InvalidArg));
            }
        };
        let key = StoreStateKey::from(key);
        match store.get_json(&tenant_ctx, STATE_PREFIX, &key, None) {
            Ok(Some(value)) => {
                let result = if let Some(text) = value.as_str() {
                    text.to_string()
                } else {
                    serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
                };
                Ok(Ok(result))
            }
            Ok(None) => Ok(Err(types::IfaceError::NotFound)),
            Err(err) => {
                tracing::warn!(error = %err, "state.get failed");
                Ok(Err(types::IfaceError::Internal))
            }
        }
    }

    fn state_set(
        &mut self,
        key: iface_types::StateKey,
        value_json: String,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<state::OpAck, types::IfaceError>> {
        let store = match self.state_store_handle() {
            Ok(store) => store,
            Err(err) => return Ok(Err(err)),
        };
        let tenant_ctx = match self.tenant_ctx_from_v6(ctx) {
            Ok(ctx) => ctx,
            Err(err) => {
                tracing::warn!(error = %err, "invalid tenant context for state.set");
                return Ok(Err(types::IfaceError::InvalidArg));
            }
        };
        let key = StoreStateKey::from(key);
        let value = serde_json::from_str(&value_json).unwrap_or(Value::String(value_json));
        match store.set_json(&tenant_ctx, STATE_PREFIX, &key, None, &value, None) {
            Ok(()) => Ok(Ok(state::OpAck::Ok)),
            Err(err) => {
                tracing::warn!(error = %err, "state.set failed");
                Ok(Err(types::IfaceError::Internal))
            }
        }
    }

    fn session_update(
        &mut self,
        cursor: iface_types::SessionCursor,
        ctx: Option<types::TenantCtx>,
    ) -> WasmResult<Result<String, types::IfaceError>> {
        let store = match self.session_store_handle() {
            Ok(store) => store,
            Err(err) => return Ok(Err(err)),
        };
        let tenant_ctx = match self.tenant_ctx_from_v6(ctx) {
            Ok(ctx) => ctx,
            Err(err) => {
                tracing::warn!(error = %err, "invalid tenant context for session.update");
                return Ok(Err(types::IfaceError::InvalidArg));
            }
        };
        let user = match Self::ensure_user(&tenant_ctx) {
            Ok(user) => user,
            Err(err) => return Ok(Err(err)),
        };
        let flow_id = match Self::ensure_flow(&tenant_ctx) {
            Ok(flow) => flow,
            Err(err) => return Ok(Err(err)),
        };
        let cursor = Self::cursor_from_iface(cursor);
        let payload = SessionData {
            tenant_ctx: tenant_ctx.clone(),
            flow_id,
            cursor: cursor.clone(),
            context_json: serde_json::json!({
                "node_pointer": cursor.node_pointer,
                "wait_reason": cursor.wait_reason,
                "outbox_marker": cursor.outbox_marker,
            })
            .to_string(),
        };
        if let Some(existing) = tenant_ctx.session_id() {
            let key = StoreSessionKey::from(existing.to_string());
            if let Err(err) = store.update_session(&key, payload) {
                tracing::error!(error = %err, "failed to update session snapshot");
                return Ok(Err(types::IfaceError::Internal));
            }
            return Ok(Ok(existing.to_string()));
        }
        match store.find_by_user(&tenant_ctx, &user) {
            Ok(Some((key, _))) => {
                if let Err(err) = store.update_session(&key, payload) {
                    tracing::error!(error = %err, "failed to update existing user session");
                    return Ok(Err(types::IfaceError::Internal));
                }
                return Ok(Ok(key.to_string()));
            }
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = %err, "session lookup failed");
                return Ok(Err(types::IfaceError::Internal));
            }
        }
        let key = match store.create_session(&tenant_ctx, payload.clone()) {
            Ok(key) => key,
            Err(err) => {
                tracing::error!(error = %err, "failed to create session");
                return Ok(Err(types::IfaceError::Internal));
            }
        };
        let ctx_with_session = tenant_ctx.with_session(key.to_string());
        let updated_payload = SessionData {
            tenant_ctx: ctx_with_session.clone(),
            ..payload
        };
        if let Err(err) = store.update_session(&key, updated_payload) {
            tracing::warn!(error = %err, "failed to stamp session id after create");
        }
        Ok(Ok(key.to_string()))
    }
}

impl host_import_v0_2::HostImports for HostState {
    fn secrets_get(
        &mut self,
        key: String,
        _ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<Result<String, LegacyIfaceError>> {
        Ok(self.get_secret(&key).map_err(|err| {
            tracing::warn!(secret = %key, error = %err, "secret lookup denied");
            LegacyIfaceError::Denied
        }))
    }

    fn telemetry_emit(
        &mut self,
        span_json: String,
        _ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<()> {
        if let Some(mock) = &self.mocks
            && mock.telemetry_drain(&[("span_json", span_json.as_str())])
        {
            return Ok(());
        }
        tracing::info!(span = %span_json, "telemetry emit from pack");
        Ok(())
    }

    fn tool_invoke(
        &mut self,
        tool: String,
        action: String,
        args_json: String,
        ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<Result<String, LegacyIfaceError>> {
        let _ = (tool, action, args_json, ctx);
        tracing::warn!("tool invoke requested but MCP bridge is removed; returning unavailable");
        Ok(Err(LegacyIfaceError::Unavailable))
    }

    fn http_fetch(
        &mut self,
        req: LegacyHttpRequest,
        _ctx: Option<LegacyTenantCtx>,
    ) -> WasmResult<Result<LegacyHttpResponse, LegacyIfaceError>> {
        if !self.config.http_enabled {
            tracing::warn!(url = %req.url, "http fetch denied by policy");
            return Ok(Err(LegacyIfaceError::Denied));
        }

        let mut mock_state = None;
        let raw_body = req.body.clone();
        if let Some(mock) = &self.mocks
            && let Ok(meta) = HttpMockRequest::new(
                &req.method,
                &req.url,
                raw_body.as_deref().map(|body| body.as_bytes()),
            )
        {
            match mock.http_begin(&meta) {
                HttpDecision::Mock(response) => {
                    let http = LegacyHttpResponse::from(&response);
                    return Ok(Ok(http));
                }
                HttpDecision::Deny(reason) => {
                    tracing::warn!(url = %req.url, reason = %reason, "http fetch blocked by mocks");
                    return Ok(Err(LegacyIfaceError::Denied));
                }
                HttpDecision::Passthrough { record } => {
                    mock_state = Some((meta, record));
                }
            }
        }

        let method = req.method.parse().unwrap_or(reqwest::Method::GET);
        let mut builder = self.http_client.request(method, &req.url);

        if let Some(headers_json) = req.headers_json.as_ref() {
            match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(headers_json) {
                Ok(map) => {
                    for (key, value) in map {
                        if let Some(val) = value.as_str()
                            && let Ok(header) =
                                reqwest::header::HeaderName::from_bytes(key.as_bytes())
                            && let Ok(header_value) = reqwest::header::HeaderValue::from_str(val)
                        {
                            builder = builder.header(header, header_value);
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "failed to parse headers for http.fetch");
                }
            }
        }

        if let Some(body) = raw_body.clone() {
            builder = builder.body(body);
        }

        let response = match builder.send() {
            Ok(resp) => resp,
            Err(err) => {
                tracing::error!(url = %req.url, error = %err, "http fetch failed");
                return Ok(Err(LegacyIfaceError::Unavailable));
            }
        };

        let status = response.status().as_u16();
        let headers_map = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let headers_json = serde_json::to_string(&headers_map).ok();
        let body = response.text().ok();

        if let Some((meta, true)) = mock_state.take()
            && let Some(mock) = &self.mocks
        {
            let recorded = HttpMockResponse::new(status, headers_map.clone(), body.clone());
            mock.http_record(&meta, &recorded);
        }

        Ok(Ok(LegacyHttpResponse {
            status,
            headers_json,
            body,
        }))
    }
}

impl PackRuntime {
    #[allow(clippy::too_many_arguments)]
    pub async fn load(
        path: impl AsRef<Path>,
        config: Arc<HostConfig>,
        mocks: Option<Arc<MockLayer>>,
        archive_source: Option<&Path>,
        session_store: Option<DynSessionStore>,
        state_store: Option<DynStateStore>,
        wasi_policy: Arc<RunnerWasiPolicy>,
        secrets: DynSecretsManager,
        oauth_config: Option<OAuthBrokerConfig>,
        verify_archive: bool,
    ) -> Result<Self> {
        let path = path.as_ref();
        let (_pack_root, safe_path) = normalize_pack_path(path)?;
        let is_component = safe_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("wasm"))
            .unwrap_or(false);
        let archive_hint_path = if let Some(source) = archive_source {
            let (_, normalized) = normalize_pack_path(source)?;
            Some(normalized)
        } else if is_component {
            None
        } else {
            Some(safe_path.clone())
        };
        let archive_hint = archive_hint_path.as_deref();
        if verify_archive {
            let verify_target = archive_hint.unwrap_or(&safe_path);
            verify::verify_pack(verify_target).await?;
            tracing::info!(pack_path = %verify_target.display(), "pack verification complete");
        }
        let engine = Engine::default();
        let wasm_bytes = fs::read(&safe_path).await?;
        let mut metadata = PackMetadata::from_wasm(&wasm_bytes)
            .unwrap_or_else(|| PackMetadata::fallback(&safe_path));
        let mut manifest = None;
        let flows = if let Some(archive_path) = archive_hint {
            match open_pack(archive_path, SigningPolicy::DevOk) {
                Ok(pack) => {
                    metadata = PackMetadata::from_manifest(&pack.manifest);
                    manifest = Some(pack.manifest);
                    None
                }
                Err(err) => {
                    warn!(error = ?err, pack = %archive_path.display(), "failed to parse pack manifest; falling back to legacy flow cache");
                    match PackFlows::from_archive(archive_path) {
                        Ok(cache) => {
                            metadata = cache.metadata.clone();
                            Some(cache)
                        }
                        Err(err) => {
                            warn!(
                                error = %err,
                                pack = %archive_path.display(),
                                "failed to parse pack flows via greentic-pack; falling back to component exports"
                            );
                            None
                        }
                    }
                }
            }
        } else {
            None
        };
        let components = if let Some(archive_path) = archive_hint {
            match load_components_from_archive(&engine, archive_path) {
                Ok(map) => map,
                Err(err) => {
                    warn!(error = %err, pack = %archive_path.display(), "failed to load components from archive");
                    HashMap::new()
                }
            }
        } else if is_component {
            let name = safe_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "component".to_string());
            let component = Component::from_binary(&engine, &wasm_bytes)?;
            let mut map = HashMap::new();
            map.insert(
                name.clone(),
                PackComponent {
                    name,
                    version: metadata.version.clone(),
                    component,
                },
            );
            map
        } else {
            HashMap::new()
        };
        let http_client = Arc::clone(&HTTP_CLIENT);
        Ok(Self {
            path: safe_path,
            archive_path: archive_hint.map(Path::to_path_buf),
            config,
            engine,
            metadata,
            manifest,
            mocks,
            flows,
            components,
            http_client,
            pre_cache: Mutex::new(HashMap::new()),
            session_store,
            state_store,
            wasi_policy,
            secrets,
            oauth_config,
        })
    }

    pub async fn list_flows(&self) -> Result<Vec<FlowDescriptor>> {
        if let Some(cache) = &self.flows {
            return Ok(cache.descriptors.clone());
        }
        if let Some(manifest) = &self.manifest {
            let descriptors = manifest
                .flows
                .iter()
                .map(|flow| FlowDescriptor {
                    id: flow.id.clone(),
                    flow_type: flow.kind.clone(),
                    profile: manifest.meta.name.clone(),
                    version: manifest.meta.version.to_string(),
                    description: Some(flow.kind.clone()),
                })
                .collect();
            return Ok(descriptors);
        }
        Ok(Vec::new())
    }

    #[allow(dead_code)]
    pub async fn run_flow(
        &self,
        _flow_id: &str,
        _input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        // TODO: dispatch flow execution via Wasmtime
        Ok(serde_json::json!({}))
    }

    pub async fn invoke_component(
        &self,
        component_ref: &str,
        ctx: ComponentExecCtx,
        operation: &str,
        _config_json: Option<String>,
        input_json: String,
    ) -> Result<Value> {
        let pack_component = self
            .components
            .get(component_ref)
            .with_context(|| format!("component '{component_ref}' not found in pack"))?;

        let pre = if let Some(pre) = self.pre_cache.lock().get(component_ref).cloned() {
            pre
        } else {
            let mut linker = Linker::new(&self.engine);
            register_all(&mut linker, self.oauth_config.is_some())?;
            add_component_control_to_linker(&mut linker)?;
            let pre = ComponentPre::new(
                linker
                    .instantiate_pre(&pack_component.component)
                    .map_err(|err| anyhow!(err))?,
            )
            .map_err(|err| anyhow!(err))?;
            self.pre_cache
                .lock()
                .insert(component_ref.to_string(), pre.clone());
            pre
        };

        let host_state = HostState::new(
            Arc::clone(&self.config),
            Arc::clone(&self.http_client),
            self.mocks.clone(),
            self.session_store.clone(),
            self.state_store.clone(),
            Arc::clone(&self.secrets),
            self.oauth_config.clone(),
        )?;
        let store_state = ComponentState::new(host_state, Arc::clone(&self.wasi_policy))?;
        let mut store = wasmtime::Store::new(&self.engine, store_state);
        let bindings = pre
            .instantiate_async(&mut store)
            .await
            .map_err(|err| anyhow!(err))?;
        let node = bindings.greentic_component_node();

        let result = node.call_invoke(&mut store, &ctx, operation, &input_json)?;

        match result {
            InvokeResult::Ok(body) => {
                if body.is_empty() {
                    return Ok(Value::Null);
                }
                serde_json::from_str(&body).or_else(|_| Ok(Value::String(body)))
            }
            InvokeResult::Err(NodeError {
                code,
                message,
                retryable,
                backoff_ms,
                details,
            }) => {
                let mut obj = serde_json::Map::new();
                obj.insert("ok".into(), Value::Bool(false));
                let mut error = serde_json::Map::new();
                error.insert("code".into(), Value::String(code));
                error.insert("message".into(), Value::String(message));
                error.insert("retryable".into(), Value::Bool(retryable));
                if let Some(backoff) = backoff_ms {
                    error.insert("backoff_ms".into(), Value::Number(backoff.into()));
                }
                if let Some(details) = details {
                    error.insert(
                        "details".into(),
                        serde_json::from_str(&details).unwrap_or(Value::String(details)),
                    );
                }
                obj.insert("error".into(), Value::Object(error));
                Ok(Value::Object(obj))
            }
        }
    }

    pub fn load_flow_ir(&self, flow_id: &str) -> Result<greentic_flow::ir::FlowIR> {
        if let Some(cache) = &self.flows {
            return cache
                .flows
                .get(flow_id)
                .cloned()
                .ok_or_else(|| anyhow!("flow '{flow_id}' not found in pack"));
        }
        if let Some(manifest) = &self.manifest {
            let entry = manifest
                .flows
                .iter()
                .find(|f| f.id == flow_id)
                .ok_or_else(|| anyhow!("flow '{flow_id}' not found in manifest"))?;
            let archive_path = self.archive_path.as_ref().unwrap_or(&self.path);
            let mut archive = ZipArchive::new(File::open(archive_path)?)?;
            match load_flow_doc(&mut archive, entry).and_then(|doc| {
                greentic_flow::to_ir(doc.clone())
                    .with_context(|| format!("failed to build IR for flow {}", doc.id))
            }) {
                Ok(ir) => return Ok(ir),
                Err(err) => {
                    warn!(
                        error = %err,
                        flow_id = flow_id,
                        "failed to parse flow from archive; falling back to stub IR"
                    );
                    return Ok(build_stub_flow_ir(&entry.id, &entry.kind));
                }
            }
        }
        bail!("flow '{flow_id}' not available (pack exports disabled)")
    }

    pub fn metadata(&self) -> &PackMetadata {
        &self.metadata
    }

    pub fn for_component_test(
        components: Vec<(String, PathBuf)>,
        flows: HashMap<String, FlowIR>,
        config: Arc<HostConfig>,
    ) -> Result<Self> {
        let engine = Engine::default();
        let mut component_map = HashMap::new();
        for (name, path) in components {
            if !path.exists() {
                bail!("component artifact missing: {}", path.display());
            }
            let wasm_bytes = std::fs::read(&path)?;
            let component = Component::from_binary(&engine, &wasm_bytes)
                .with_context(|| format!("failed to compile component {}", path.display()))?;
            component_map.insert(
                name.clone(),
                PackComponent {
                    name,
                    version: "0.0.0".into(),
                    component,
                },
            );
        }

        let descriptors = flows
            .iter()
            .map(|(id, ir)| FlowDescriptor {
                id: id.clone(),
                flow_type: ir.flow_type.clone(),
                profile: "test".into(),
                version: "0.0.0".into(),
                description: None,
            })
            .collect::<Vec<_>>();
        let flows_cache = PackFlows {
            descriptors: descriptors.clone(),
            flows,
            metadata: PackMetadata::fallback(Path::new("component-test")),
        };

        Ok(Self {
            path: PathBuf::new(),
            archive_path: None,
            config,
            engine,
            metadata: PackMetadata::fallback(Path::new("component-test")),
            manifest: None,
            mocks: None,
            flows: Some(flows_cache),
            components: component_map,
            http_client: Arc::clone(&HTTP_CLIENT),
            pre_cache: Mutex::new(HashMap::new()),
            session_store: None,
            state_store: None,
            wasi_policy: Arc::new(RunnerWasiPolicy::new()),
            secrets: crate::secrets::default_manager(),
            oauth_config: None,
        })
    }
}

fn map_legacy_error(err: LegacyIfaceError) -> types::IfaceError {
    match err {
        LegacyIfaceError::InvalidArg => types::IfaceError::InvalidArg,
        LegacyIfaceError::NotFound => types::IfaceError::NotFound,
        LegacyIfaceError::Denied => types::IfaceError::Denied,
        LegacyIfaceError::Unavailable => types::IfaceError::Unavailable,
        LegacyIfaceError::Internal => types::IfaceError::Internal,
    }
}

struct PackFlows {
    descriptors: Vec<FlowDescriptor>,
    flows: HashMap<String, FlowIR>,
    metadata: PackMetadata,
}

impl PackFlows {
    fn from_archive(path: &Path) -> Result<Self> {
        let pack = open_pack(path, SigningPolicy::DevOk)
            .map_err(|err| anyhow!("failed to open pack {}: {}", path.display(), err.message))?;
        let file = File::open(path)
            .with_context(|| format!("failed to open archive {}", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("{} is not a valid gtpack", path.display()))?;
        let mut flows = HashMap::new();
        let mut descriptors = Vec::new();
        for flow in &pack.manifest.flows {
            match load_flow_entry(&mut archive, flow, &pack.manifest.meta) {
                Ok((descriptor, ir)) => {
                    flows.insert(descriptor.id.clone(), ir);
                    descriptors.push(descriptor);
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        flow_id = flow.id,
                        "failed to parse flow metadata from archive; using stub flow"
                    );
                    let stub = build_stub_flow_ir(&flow.id, &flow.kind);
                    flows.insert(flow.id.clone(), stub.clone());
                    descriptors.push(FlowDescriptor {
                        id: flow.id.clone(),
                        flow_type: flow.kind.clone(),
                        profile: pack.manifest.meta.name.clone(),
                        version: pack.manifest.meta.version.to_string(),
                        description: Some(flow.kind.clone()),
                    });
                }
            }
        }

        Ok(Self {
            metadata: PackMetadata::from_manifest(&pack.manifest),
            descriptors,
            flows,
        })
    }
}

fn load_flow_entry(
    archive: &mut ZipArchive<File>,
    entry: &greentic_pack::builder::FlowEntry,
    meta: &greentic_pack::builder::PackMeta,
) -> Result<(FlowDescriptor, FlowIR)> {
    let doc = load_flow_doc(archive, entry)?;
    let ir = greentic_flow::to_ir(doc.clone())
        .with_context(|| format!("failed to build IR for flow {}", doc.id))?;
    let descriptor = FlowDescriptor {
        id: doc.id.clone(),
        flow_type: doc.flow_type.clone(),
        profile: meta.name.clone(),
        version: meta.version.to_string(),
        description: doc
            .description
            .clone()
            .or(doc.title.clone())
            .or_else(|| Some(entry.kind.clone())),
    };
    Ok((descriptor, ir))
}

fn load_flow_doc(
    archive: &mut ZipArchive<File>,
    entry: &greentic_pack::builder::FlowEntry,
) -> Result<greentic_flow::model::FlowDoc> {
    if let Ok(bytes) = read_entry(archive, &entry.file_yaml)
        && let Ok(doc) = serde_yaml::from_slice(&bytes)
    {
        return Ok(normalize_flow_doc(doc));
    }
    let json_bytes = read_entry(archive, &entry.file_json)
        .with_context(|| format!("missing flow document {}", entry.file_json))?;
    let doc = serde_json::from_slice(&json_bytes)
        .with_context(|| format!("failed to parse {}", entry.file_json))?;
    Ok(normalize_flow_doc(doc))
}

fn read_entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut file = archive
        .by_name(name)
        .with_context(|| format!("entry {name} missing from archive"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

fn normalize_flow_doc(mut doc: FlowDoc) -> FlowDoc {
    for node in doc.nodes.values_mut() {
        if node.component.is_empty()
            && let Some((component_ref, payload)) = node.raw.iter().next()
        {
            // Preserve the component key from the raw map and keep the original payload
            // so component.exec nodes retain their inner `component` target.
            node.component = component_ref.clone();
            node.payload = payload.clone();
        }
    }
    doc
}

fn infer_component_exec(payload: &Value, component_ref: &str) -> (String, Value, Option<Value>) {
    let default_op = if component_ref.starts_with("templating.") {
        "render"
    } else {
        "invoke"
    }
    .to_string();

    if let Value::Object(map) = payload {
        let op = map
            .get("op")
            .or_else(|| map.get("operation"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_op.clone());

        let mut input = map.clone();
        let config = input.remove("config");
        input.remove("op");
        input.remove("operation");
        return (op, Value::Object(input), config);
    }

    (default_op, payload.clone(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use greentic_flow::model::{FlowDoc, Node, Route};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn normalizes_raw_component_to_component_exec() {
        let mut nodes = BTreeMap::new();
        let mut raw = BTreeMap::new();
        raw.insert(
            "templating.handlebars".into(),
            json!({ "template": "Hi {{name}}" }),
        );
        nodes.insert(
            "start".into(),
            Node {
                raw,
                routing: vec![Route {
                    to: None,
                    out: Some(true),
                }],
                ..Default::default()
            },
        );
        let doc = FlowDoc {
            id: "welcome".into(),
            title: None,
            description: None,
            flow_type: "messaging".into(),
            start: Some("start".into()),
            parameters: json!({}),
            nodes,
        };

        let normalized = normalize_flow_doc(doc);
        let node = normalized.nodes.get("start").expect("node exists");
        assert_eq!(node.component, "component.exec");
        assert!(node.raw.is_empty() || node.raw.contains_key("templating.handlebars"));
        let payload = node.payload.as_object().expect("payload object");
        assert_eq!(
            payload.get("component"),
            Some(&Value::String("templating.handlebars".into()))
        );
        assert_eq!(
            payload.get("operation"),
            Some(&Value::String("render".into()))
        );
        let input = payload.get("input").unwrap();
        assert_eq!(input, &json!({ "template": "Hi {{name}}" }));
    }
}

fn load_components_from_archive(
    engine: &Engine,
    path: &Path,
) -> Result<HashMap<String, PackComponent>> {
    let pack = open_pack(path, SigningPolicy::DevOk)
        .map_err(|err| anyhow!("failed to open pack {}: {}", path.display(), err.message))?;
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let mut components = HashMap::new();
    for entry in &pack.manifest.components {
        let bytes = read_entry(&mut archive, &entry.file_wasm)
            .with_context(|| format!("missing component {}", entry.file_wasm))?;
        let component = Component::from_binary(engine, &bytes)
            .with_context(|| format!("failed to compile component {}", entry.name))?;
        components.insert(
            entry.name.clone(),
            PackComponent {
                name: entry.name.clone(),
                version: entry.version.to_string(),
                component,
            },
        );
    }
    Ok(components)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PackMetadata {
    pub pack_id: String,
    pub version: String,
    #[serde(default)]
    pub entry_flows: Vec<String>,
}

impl PackMetadata {
    fn from_wasm(bytes: &[u8]) -> Option<Self> {
        let parser = Parser::new(0);
        for payload in parser.parse_all(bytes) {
            let payload = payload.ok()?;
            match payload {
                Payload::CustomSection(section) => {
                    if section.name() == "greentic.manifest"
                        && let Ok(meta) = Self::from_bytes(section.data())
                    {
                        return Some(meta);
                    }
                }
                Payload::DataSection(reader) => {
                    for segment in reader.into_iter().flatten() {
                        if let Ok(meta) = Self::from_bytes(segment.data) {
                            return Some(meta);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, serde_cbor::Error> {
        #[derive(Deserialize)]
        struct RawManifest {
            pack_id: String,
            version: String,
            #[serde(default)]
            entry_flows: Vec<String>,
            #[serde(default)]
            flows: Vec<RawFlow>,
        }

        #[derive(Deserialize)]
        struct RawFlow {
            id: String,
        }

        let manifest: RawManifest = serde_cbor::from_slice(bytes)?;
        let mut entry_flows = if manifest.entry_flows.is_empty() {
            manifest.flows.iter().map(|f| f.id.clone()).collect()
        } else {
            manifest.entry_flows.clone()
        };
        entry_flows.retain(|id| !id.is_empty());
        Ok(Self {
            pack_id: manifest.pack_id,
            version: manifest.version,
            entry_flows,
        })
    }

    pub fn fallback(path: &Path) -> Self {
        let pack_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown-pack".to_string());
        Self {
            pack_id,
            version: "0.0.0".to_string(),
            entry_flows: Vec::new(),
        }
    }

    pub fn from_manifest(manifest: &greentic_pack::builder::PackManifest) -> Self {
        let entry_flows = if manifest.meta.entry_flows.is_empty() {
            manifest
                .flows
                .iter()
                .map(|flow| flow.id.clone())
                .collect::<Vec<_>>()
        } else {
            manifest.meta.entry_flows.clone()
        };
        Self {
            pack_id: manifest.meta.pack_id.clone(),
            version: manifest.meta.version.to_string(),
            entry_flows,
        }
    }
}

fn build_stub_flow_ir(flow_id: &str, flow_type: &str) -> FlowIR {
    let mut nodes = IndexMap::new();
    nodes.insert(
        "complete".into(),
        NodeIR {
            component: "component.exec".into(),
            payload_expr: serde_json::json!({
                "component": "qa.process",
                "operation": "process",
                "input": {
                    "status": "done",
                    "flow_id": flow_id,
                }
            }),
            routes: vec![RouteIR {
                to: None,
                out: true,
            }],
        },
    );
    FlowIR {
        id: flow_id.to_string(),
        flow_type: flow_type.to_string(),
        start: Some("complete".into()),
        parameters: Value::Object(Default::default()),
        nodes,
    }
}

impl From<&HttpMockResponse> for LegacyHttpResponse {
    fn from(value: &HttpMockResponse) -> Self {
        let headers_json = serde_json::to_string(&value.headers).ok();
        Self {
            status: value.status,
            headers_json,
            body: value.body.clone(),
        }
    }
}
