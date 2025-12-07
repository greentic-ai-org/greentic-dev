pub mod adapt_messaging;
pub mod adapt_slack;
pub mod adapt_teams;
pub mod adapt_timer;
pub mod adapt_webchat;
pub mod adapt_webex;
pub mod adapt_webhook;
pub mod adapt_whatsapp;
pub mod engine;
pub mod ingress_util;
pub mod mocks;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::routing::{any, get, post};
use axum::{Router, serve};
use tokio::net::TcpListener;

use crate::http::{self, admin, auth::AdminAuth, health::HealthState};
use crate::routing::TenantRouting;
use crate::runtime::ActivePacks;
use crate::watcher::PackReloadHandle;

pub struct HostServer {
    addr: SocketAddr,
    router: Router,
    _state: ServerState,
}

impl HostServer {
    pub fn new(
        port: u16,
        active: Arc<ActivePacks>,
        routing: TenantRouting,
        health: Arc<HealthState>,
        reload: Option<PackReloadHandle>,
        admin: AdminAuth,
    ) -> Result<Self> {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let state = ServerState {
            active,
            routing,
            health,
            reload,
            admin,
        };
        let router = Router::new()
            .route(
                "/messaging/telegram/webhook",
                post(adapt_messaging::telegram_webhook),
            )
            .route("/webchat/activities", post(adapt_webchat::activities))
            .route("/teams/activities", post(adapt_teams::activities))
            .route("/slack/events", post(adapt_slack::events))
            .route("/slack/interactive", post(adapt_slack::interactive))
            .route("/webex/webhook", post(adapt_webex::webhook))
            .route(
                "/whatsapp/webhook",
                get(adapt_whatsapp::verify).post(adapt_whatsapp::webhook),
            )
            .route("/webhook/:flow_id", any(adapt_webhook::dispatch))
            .route("/healthz", get(http::health::handler))
            .route("/admin/packs/status", get(admin::status))
            .route("/admin/packs/reload", post(admin::reload))
            .with_state(state.clone());
        Ok(Self {
            addr,
            router,
            _state: state,
        })
    }

    pub async fn serve(self) -> Result<()> {
        tracing::info!(addr = %self.addr, "starting host server");
        let listener = TcpListener::bind(self.addr).await?;
        serve(
            listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ServerState {
    pub active: Arc<ActivePacks>,
    pub routing: TenantRouting,
    pub health: Arc<HealthState>,
    pub reload: Option<PackReloadHandle>,
    pub admin: AdminAuth,
}
