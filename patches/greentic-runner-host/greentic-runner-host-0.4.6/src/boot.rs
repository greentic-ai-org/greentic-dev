use anyhow::Result;
#[cfg(feature = "telemetry")]
use anyhow::anyhow;

use crate::TelemetryCfg;
use crate::http::health::HealthState;

#[cfg(feature = "telemetry")]
use greentic_telemetry::{OtlpConfig, init_otlp};

#[cfg(feature = "telemetry")]
use tracing::info;

pub fn init(health: &HealthState, otlp_cfg: Option<&TelemetryCfg>) -> Result<()> {
    init_telemetry(otlp_cfg)?;
    health.mark_telemetry_ready();
    health.mark_secrets_ready();
    Ok(())
}

#[cfg(feature = "telemetry")]
fn init_telemetry(config: Option<&crate::TelemetryCfg>) -> Result<()> {
    apply_preset_from_env();
    let target_service = config
        .cloned()
        .or_else(|| {
            let service = std::env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "greentic-runner-host".into());
            Some(OtlpConfig {
                service_name: service,
                endpoint: None,
                sampling_rate: None,
            })
        })
        .unwrap();
    info!(
        service = target_service.service_name,
        "initialising telemetry pipeline"
    );
    init_otlp(target_service, Vec::new()).map_err(|err| anyhow!(err.to_string()))
}

#[cfg(not(feature = "telemetry"))]
fn init_telemetry(_config: Option<&TelemetryCfg>) -> Result<()> {
    Ok(())
}

#[cfg(feature = "telemetry")]
fn apply_preset_from_env() {
    if let Ok(preset) = std::env::var("CLOUD_PRESET") {
        info!(preset = %preset, "telemetry preset requested");
    }
}
