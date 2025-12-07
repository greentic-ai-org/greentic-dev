use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cron::Schedule;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::engine::runtime::IngressEnvelope;
use crate::runtime::TenantRuntime;

pub fn spawn_timers(runtime: Arc<TenantRuntime>) -> Result<Vec<JoinHandle<()>>> {
    let mut handles = Vec::new();

    for timer in runtime.config().timers.clone() {
        let cron_expr = timer.cron.clone();
        let normalized = normalize_cron(&cron_expr);
        let schedule = Schedule::from_str(&normalized)
            .with_context(|| format!("invalid cron expression for {}", timer.schedule_id()))?;
        let flow_id = timer.flow_id.clone();
        let schedule_id = timer.schedule_id().to_string();
        let tenant = runtime.config().tenant.clone();
        let runtime_clone = Arc::clone(&runtime);

        let handle = tokio::spawn(async move {
            tracing::info!(
                flow_id = %flow_id,
                schedule_id = %schedule_id,
                cron = %cron_expr,
                normalized_cron = %normalized,
                "registered timer schedule"
            );
            for next in schedule.upcoming(Utc) {
                if let Some(wait) = duration_until(next) {
                    sleep(wait).await;
                } else {
                    continue;
                }
                let payload = json!({
                    "now": next.to_rfc3339(),
                    "schedule_id": schedule_id.clone(),
                });
                tracing::info!(
                    flow_id = %flow_id,
                    schedule_id = %schedule_id,
                    scheduled_for = %next,
                    "triggering timer flow"
                );
                let envelope = IngressEnvelope {
                    tenant: tenant.clone(),
                    env: None,
                    flow_id: flow_id.clone(),
                    flow_type: Some("timer".into()),
                    action: Some("timer".into()),
                    session_hint: Some(schedule_id.clone()),
                    provider: Some("timer".into()),
                    channel: Some(schedule_id.clone()),
                    conversation: Some(schedule_id.clone()),
                    user: None,
                    activity_id: Some(format!("{}@{}", schedule_id, next)),
                    timestamp: Some(next.to_rfc3339()),
                    payload,
                    metadata: None,
                }
                .canonicalize();
                match runtime_clone.state_machine().handle(envelope).await {
                    Ok(output) => {
                        tracing::info!(
                            flow_id = %flow_id,
                            schedule_id = %schedule_id,
                            now = %next,
                            response = %output,
                            "timer flow completed"
                        );
                    }
                    Err(err) => {
                        let chain = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
                        tracing::error!(
                            flow_id = %flow_id,
                            schedule_id = %schedule_id,
                            error.cause_chain = ?chain,
                            "timer flow execution failed"
                        );
                    }
                }
            }
            tracing::info!(
                flow_id = %flow_id,
                schedule_id = %schedule_id,
                "timer schedule completed"
            );
        });

        handles.push(handle);
    }

    Ok(handles)
}

fn duration_until(next: DateTime<Utc>) -> Option<Duration> {
    let now = Utc::now();
    let duration = next - now;
    if duration.num_milliseconds() <= 0 {
        return Some(Duration::from_secs(0));
    }
    duration.to_std().ok()
}

fn normalize_cron(expr: &str) -> String {
    if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cron_adds_seconds_for_five_fields() {
        assert_eq!(normalize_cron("*/5 * * * *"), "0 */5 * * * *");
        assert_eq!(normalize_cron("0 */2 * * * *"), "0 */2 * * * *");
    }

    #[test]
    fn duration_until_returns_zero_for_past_times() {
        let past = Utc::now() - chrono::Duration::seconds(10);
        assert_eq!(duration_until(past).unwrap(), Duration::from_secs(0));
    }
}
