use super::error::{GResult, RunnerError};
use rand::{Rng, rng};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub retry: RetryPolicy,
    pub max_egress_adapters: usize,
    pub max_payload_bytes: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            max_egress_adapters: 32,
            max_payload_bytes: 512 * 1024,
        }
    }
}

pub async fn retry_with_jitter<F, Fut, T>(policy: &RetryPolicy, mut op: F) -> GResult<T>
where
    F: FnMut() -> Fut + Send,
    Fut: std::future::Future<Output = GResult<T>> + Send,
    T: Send,
{
    let mut attempt = 0u32;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                attempt += 1;
                if attempt >= policy.max_attempts {
                    return Err(err);
                }
                let backoff = backoff_with_jitter(policy, attempt);
                sleep(backoff).await;
            }
        }
    }
}

fn backoff_with_jitter(policy: &RetryPolicy, attempt: u32) -> Duration {
    let capped_attempt = attempt.min(10);
    let initial_ms = policy.initial_backoff.as_millis().max(1) as u64;
    let multiplier = 1u64 << capped_attempt;
    let mut base_ms = initial_ms.saturating_mul(multiplier);
    let max_ms = policy.max_backoff.as_millis().max(1) as u64;
    if base_ms > max_ms {
        base_ms = max_ms;
    }
    let base = Duration::from_millis(base_ms);
    let mut rng = rng();
    let jitter_cap = base.as_millis().max(1) as u64;
    let jitter_ms = rng.random_range(0..=jitter_cap);
    base + Duration::from_millis(jitter_ms)
}

pub fn policy_violation(reason: impl Into<String>) -> RunnerError {
    RunnerError::Policy {
        reason: reason.into(),
    }
}
