use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use runner_core::{Index, PackConfig, PackManager};
use tokio::sync::mpsc;
use tokio::task;

use crate::HostConfig;
use crate::engine::host::{SessionHost, StateHost};
use crate::host::RunnerHost;
use crate::http::health::HealthState;
use crate::pack::PackRuntime;
use crate::runner::adapt_timer;
use crate::runtime::{ActivePacks, TenantRuntime};
use crate::secrets::DynSecretsManager;
use crate::storage::session::DynSessionStore;
use crate::storage::state::DynStateStore;
use crate::wasi::RunnerWasiPolicy;

pub struct PackWatcher {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for PackWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[derive(Clone)]
pub struct PackReloadHandle {
    trigger: mpsc::Sender<()>,
}

impl PackReloadHandle {
    pub async fn trigger(&self) -> Result<()> {
        self.trigger
            .send(())
            .await
            .map_err(|_| anyhow!("pack watcher task stopped"))
    }
}

pub async fn start_pack_watcher(
    host: Arc<RunnerHost>,
    cfg: PackConfig,
    refresh: Duration,
) -> Result<(PackWatcher, PackReloadHandle)> {
    let cfg_clone = cfg.clone();
    let manager = task::spawn_blocking(move || PackManager::new(cfg_clone))
        .await
        .context("pack manager init task failed")??;
    let manager = Arc::new(manager);
    let configs = Arc::new(host.tenant_configs());
    let active = host.active_packs();
    let health = host.health_state();
    let session_host = host.session_host();
    let session_store = host.session_store();
    let state_store = host.state_store();
    let state_host = host.state_host();
    let wasi_policy = host.wasi_policy();
    let secrets_manager = host.secrets_manager();

    reload_once(
        configs.as_ref(),
        &manager,
        &cfg,
        &active,
        &health,
        session_host.clone(),
        session_store.clone(),
        state_store.clone(),
        state_host.clone(),
        Arc::clone(&wasi_policy),
        secrets_manager.clone(),
    )
    .await?;

    let (tx, mut rx) = mpsc::channel::<()>(4);
    let index_cfg = cfg.clone();
    let manager_clone = Arc::clone(&manager);
    let health_clone = Arc::clone(&health);
    let active_clone = Arc::clone(&active);
    let configs_clone = Arc::clone(&configs);
    let state_store_clone = Arc::clone(&state_store);
    let wasi_policy_clone = Arc::clone(&wasi_policy);
    let secrets_manager_clone = secrets_manager.clone();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(refresh);
        loop {
            tokio::select! {
                _ = ticker.tick() => {},
                recv = rx.recv() => {
                    if recv.is_none() {
                        break;
                    }
                }
            }
            if let Err(err) = reload_once(
                configs_clone.as_ref(),
                &manager_clone,
                &index_cfg,
                &active_clone,
                &health_clone,
                session_host.clone(),
                session_store.clone(),
                state_store_clone.clone(),
                state_host.clone(),
                Arc::clone(&wasi_policy_clone),
                secrets_manager_clone.clone(),
            )
            .await
            {
                tracing::error!(error = %err, "pack reload failed");
                health_clone.record_reload_error(&err);
            }
        }
    });

    let watcher = PackWatcher { handle };
    let handle = PackReloadHandle { trigger: tx };
    Ok((watcher, handle))
}

#[allow(clippy::too_many_arguments)]
async fn reload_once(
    configs: &HashMap<String, Arc<HostConfig>>,
    manager: &Arc<PackManager>,
    cfg: &PackConfig,
    active: &Arc<ActivePacks>,
    health: &Arc<HealthState>,
    session_host: Arc<dyn SessionHost>,
    session_store: DynSessionStore,
    state_store: DynStateStore,
    state_host: Arc<dyn StateHost>,
    wasi_policy: Arc<RunnerWasiPolicy>,
    secrets_manager: DynSecretsManager,
) -> Result<()> {
    let index = Index::load(&cfg.index_location)?;
    let resolved = manager.resolve_all_for_index(&index)?;
    let mut next = HashMap::new();
    for (tenant, record) in resolved.tenants() {
        let config = configs
            .get(tenant)
            .cloned()
            .with_context(|| format!("no host config registered for tenant {tenant}"))?;
        let oauth_config = config.oauth_broker_config();
        let mut packs = Vec::new();
        let main_runtime = Arc::new(
            PackRuntime::load(
                &record.main.path,
                Arc::clone(&config),
                None,
                Some(&record.main.path),
                Some(Arc::clone(&session_store)),
                Some(Arc::clone(&state_store)),
                Arc::clone(&wasi_policy),
                Arc::clone(&secrets_manager),
                oauth_config.clone(),
                true,
            )
            .await
            .with_context(|| format!("failed to load pack for tenant {tenant}"))?,
        );
        packs.push((main_runtime, Some(record.main.digest.as_str().to_string())));

        for overlay in &record.overlays {
            let runtime = Arc::new(
                PackRuntime::load(
                    &overlay.path,
                    Arc::clone(&config),
                    None,
                    Some(&overlay.path),
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
                        "failed to load overlay {} for tenant {tenant}",
                        overlay.reference.name
                    )
                })?,
            );
            packs.push((runtime, Some(overlay.digest.as_str().to_string())));
        }

        let runtime = TenantRuntime::from_packs(
            Arc::clone(&config),
            packs,
            None,
            Arc::clone(&session_host),
            Arc::clone(&session_store),
            Arc::clone(&state_store),
            Arc::clone(&state_host),
            Arc::clone(&secrets_manager),
        )
        .await?;
        let timers = adapt_timer::spawn_timers(Arc::clone(&runtime))?;
        runtime.register_timers(timers);

        next.insert(tenant.clone(), runtime);
    }
    active.replace(next);
    health.record_reload_success();
    tracing::info!("pack reload completed successfully");
    Ok(())
}
