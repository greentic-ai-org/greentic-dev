use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use arc_swap::ArcSwap;
use axum::http::StatusCode;
use lru::LruCache;
use parking_lot::Mutex;
use reqwest::Client;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::config::HostConfig;
use crate::engine::host::{SessionHost, StateHost};
use crate::engine::runtime::StateMachineRuntime;
use crate::pack::PackRuntime;
use crate::runner::engine::FlowEngine;
use crate::runner::mocks::MockLayer;
use crate::secrets::{DynSecretsManager, read_secret_blocking};
use crate::storage::session::DynSessionStore;
use crate::storage::state::DynStateStore;
use crate::wasi::RunnerWasiPolicy;

const TELEGRAM_CACHE_CAPACITY: usize = 1024;
const WEBHOOK_CACHE_CAPACITY: usize = 256;

/// Atomically swapped view of live tenant runtimes.
pub struct ActivePacks {
    inner: ArcSwap<HashMap<String, Arc<TenantRuntime>>>,
}

impl ActivePacks {
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    pub fn load(&self, tenant: &str) -> Option<Arc<TenantRuntime>> {
        self.inner.load().get(tenant).cloned()
    }

    pub fn snapshot(&self) -> Arc<HashMap<String, Arc<TenantRuntime>>> {
        self.inner.load_full()
    }

    pub fn replace(&self, next: HashMap<String, Arc<TenantRuntime>>) {
        self.inner.store(Arc::new(next));
    }

    pub fn len(&self) -> usize {
        self.inner.load().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ActivePacks {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime bundle for a tenant pack.
pub struct TenantRuntime {
    tenant: String,
    config: Arc<HostConfig>,
    packs: Vec<Arc<PackRuntime>>,
    digests: Vec<Option<String>>,
    engine: Arc<FlowEngine>,
    state_machine: Arc<StateMachineRuntime>,
    http_client: Client,
    telegram_cache: Mutex<LruCache<i64, StatusCode>>,
    webhook_cache: Mutex<LruCache<String, Value>>,
    messaging_rate: Mutex<RateLimiter>,
    mocks: Option<Arc<MockLayer>>,
    timer_handles: Mutex<Vec<JoinHandle<()>>>,
    secrets: DynSecretsManager,
}

impl TenantRuntime {
    #[allow(clippy::too_many_arguments)]
    pub async fn load(
        pack_path: &Path,
        config: Arc<HostConfig>,
        mocks: Option<Arc<MockLayer>>,
        archive_source: Option<&Path>,
        digest: Option<String>,
        wasi_policy: Arc<RunnerWasiPolicy>,
        session_host: Arc<dyn SessionHost>,
        session_store: DynSessionStore,
        state_store: DynStateStore,
        state_host: Arc<dyn StateHost>,
        secrets_manager: DynSecretsManager,
    ) -> Result<Arc<Self>> {
        let oauth_config = config.oauth_broker_config();
        let pack = Arc::new(
            PackRuntime::load(
                pack_path,
                Arc::clone(&config),
                mocks.clone(),
                archive_source,
                Some(Arc::clone(&session_store)),
                Some(Arc::clone(&state_store)),
                Arc::clone(&wasi_policy),
                Arc::clone(&secrets_manager),
                oauth_config.clone(),
                true,
            )
            .await
            .with_context(|| {
                format!(
                    "failed to load pack {} for tenant {}",
                    pack_path.display(),
                    config.tenant
                )
            })?,
        );
        Self::from_packs(
            config,
            vec![(pack, digest)],
            mocks,
            session_host,
            session_store,
            state_store,
            state_host,
            secrets_manager,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn from_packs(
        config: Arc<HostConfig>,
        packs: Vec<(Arc<PackRuntime>, Option<String>)>,
        mocks: Option<Arc<MockLayer>>,
        session_host: Arc<dyn SessionHost>,
        session_store: DynSessionStore,
        _state_store: DynStateStore,
        state_host: Arc<dyn StateHost>,
        secrets_manager: DynSecretsManager,
    ) -> Result<Arc<Self>> {
        let telegram_capacity = NonZeroUsize::new(TELEGRAM_CACHE_CAPACITY)
            .expect("telegram cache capacity must be > 0");
        let webhook_capacity =
            NonZeroUsize::new(WEBHOOK_CACHE_CAPACITY).expect("webhook cache capacity must be > 0");
        let pack_runtimes = packs
            .iter()
            .map(|(pack, _)| Arc::clone(pack))
            .collect::<Vec<_>>();
        let digests = packs
            .iter()
            .map(|(_, digest)| digest.clone())
            .collect::<Vec<_>>();
        let engine = Arc::new(
            FlowEngine::new(pack_runtimes.clone(), Arc::clone(&config))
                .await
                .context("failed to prime flow engine")?,
        );
        let state_machine = Arc::new(
            StateMachineRuntime::from_flow_engine(
                Arc::clone(&config),
                Arc::clone(&engine),
                session_host,
                session_store,
                state_host,
                Arc::clone(&secrets_manager),
                mocks.clone(),
            )
            .context("failed to initialise state machine runtime")?,
        );
        let http_client = Client::builder().build()?;
        let rate_limits = config.rate_limits.clone();
        Ok(Arc::new(Self {
            tenant: config.tenant.clone(),
            config,
            packs: pack_runtimes,
            digests,
            engine,
            state_machine,
            http_client,
            telegram_cache: Mutex::new(LruCache::new(telegram_capacity)),
            webhook_cache: Mutex::new(LruCache::new(webhook_capacity)),
            messaging_rate: Mutex::new(RateLimiter::new(
                rate_limits.messaging_send_qps,
                rate_limits.messaging_burst,
            )),
            mocks,
            timer_handles: Mutex::new(Vec::new()),
            secrets: secrets_manager,
        }))
    }

    pub fn tenant(&self) -> &str {
        &self.tenant
    }

    pub fn config(&self) -> &Arc<HostConfig> {
        &self.config
    }

    pub fn main_pack(&self) -> &Arc<PackRuntime> {
        self.packs
            .first()
            .expect("tenant runtime must contain at least one pack")
    }

    pub fn pack(&self) -> Arc<PackRuntime> {
        Arc::clone(self.main_pack())
    }

    pub fn overlays(&self) -> Vec<Arc<PackRuntime>> {
        self.packs.iter().skip(1).cloned().collect()
    }

    pub fn engine(&self) -> &Arc<FlowEngine> {
        &self.engine
    }

    pub fn state_machine(&self) -> &Arc<StateMachineRuntime> {
        &self.state_machine
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn digest(&self) -> Option<&str> {
        self.digests.first().and_then(|d| d.as_deref())
    }

    pub fn overlay_digests(&self) -> Vec<Option<String>> {
        self.digests.iter().skip(1).cloned().collect()
    }

    pub fn telegram_cache(&self) -> &Mutex<LruCache<i64, StatusCode>> {
        &self.telegram_cache
    }

    pub fn webhook_cache(&self) -> &Mutex<LruCache<String, Value>> {
        &self.webhook_cache
    }

    pub fn messaging_rate(&self) -> &Mutex<RateLimiter> {
        &self.messaging_rate
    }

    pub fn mocks(&self) -> Option<&Arc<MockLayer>> {
        self.mocks.as_ref()
    }

    pub fn register_timers(&self, handles: Vec<JoinHandle<()>>) {
        self.timer_handles.lock().extend(handles);
    }

    pub fn get_secret(&self, key: &str) -> Result<String> {
        if !self.config.secrets_policy.is_allowed(key) {
            bail!("secret {key} is not permitted by bindings policy");
        }
        let bytes = read_secret_blocking(&self.secrets, key)
            .context("failed to read secret from manager")?;
        let value = String::from_utf8(bytes).context("secret value is not valid UTF-8")?;
        Ok(value)
    }
}

impl Drop for TenantRuntime {
    fn drop(&mut self) {
        for handle in self.timer_handles.lock().drain(..) {
            handle.abort();
        }
    }
}

pub struct RateLimiter {
    allowance: f64,
    rate: f64,
    burst: f64,
    last_check: Instant,
}

impl RateLimiter {
    pub fn new(qps: u32, burst: u32) -> Self {
        let rate = qps.max(1) as f64;
        let burst = burst.max(1) as f64;
        Self {
            allowance: burst,
            rate,
            burst,
            last_check: Instant::now(),
        }
    }

    pub fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_check).as_secs_f64();
        self.last_check = now;
        self.allowance += elapsed * self.rate;
        if self.allowance > self.burst {
            self.allowance = self.burst;
        }
        if self.allowance < 1.0 {
            false
        } else {
            self.allowance -= 1.0;
            true
        }
    }
}
