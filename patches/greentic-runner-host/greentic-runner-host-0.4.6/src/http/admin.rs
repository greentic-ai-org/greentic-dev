use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use time::format_description::well_known::Rfc3339;

use crate::http::auth::AdminGuard;
use crate::runner::ServerState;

pub async fn status(AdminGuard: AdminGuard, State(state): State<ServerState>) -> impl IntoResponse {
    let snapshot = state.active.snapshot();
    let tenants = snapshot
        .iter()
        .map(|(tenant, runtime)| {
            let pack = runtime.pack();
            let metadata = pack.metadata();
            let overlays = runtime
                .overlays()
                .into_iter()
                .zip(runtime.overlay_digests().into_iter())
                .map(|(overlay, digest)| {
                    let meta = overlay.metadata();
                    json!({
                        "pack_id": meta.pack_id,
                        "version": meta.version,
                        "digest": digest,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "tenant": tenant,
                "pack_id": metadata.pack_id,
                "version": metadata.version,
                "digest": runtime.digest(),
                "overlays": overlays,
            })
        })
        .collect::<Vec<_>>();

    let health = state.health.snapshot();
    let last_reload = health.last_reload.and_then(|ts| ts.format(&Rfc3339).ok());

    Json(json!({
        "tenants": tenants,
        "active": snapshot.len(),
        "last_reload": last_reload,
        "last_error": health.last_error,
    }))
}

pub async fn reload(AdminGuard: AdminGuard, State(state): State<ServerState>) -> impl IntoResponse {
    if let Some(handle) = &state.reload {
        match handle.trigger().await {
            Ok(()) => {
                tracing::info!("pack.reload.requested");
                (
                    StatusCode::ACCEPTED,
                    Json(json!({ "status": "reload requested" })),
                )
            }
            Err(err) => {
                tracing::warn!(error = %err, "reload trigger failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": err.to_string() })),
                )
            }
        }
    } else {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({ "error": "reload handle unavailable" })),
        )
    }
}
