use crate::oauth::OAuthBrokerConfig;
use crate::runner::mocks::MocksConfig;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml_bw as serde_yaml;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub tenant: String,
    pub bindings_path: PathBuf,
    pub flow_type_bindings: HashMap<String, FlowBinding>,
    pub rate_limits: RateLimits,
    pub retry: FlowRetryConfig,
    pub http_enabled: bool,
    pub secrets_policy: SecretsPolicy,
    pub webhook_policy: WebhookPolicy,
    pub timers: Vec<TimerBinding>,
    pub oauth: Option<OAuthConfig>,
    pub mocks: Option<MocksConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BindingsFile {
    pub tenant: String,
    #[serde(default)]
    pub flow_type_bindings: HashMap<String, FlowBinding>,
    #[serde(default)]
    pub rate_limits: RateLimits,
    #[serde(default)]
    pub retry: FlowRetryConfig,
    #[serde(default)]
    pub timers: Vec<TimerBinding>,
    #[serde(default)]
    pub oauth: Option<OAuthConfig>,
    #[serde(default)]
    pub mocks: Option<MocksConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowBinding {
    pub adapter: String,
    #[serde(default)]
    pub config: serde_yaml::Value,
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimits {
    #[serde(default = "default_messaging_qps")]
    pub messaging_send_qps: u32,
    #[serde(default = "default_messaging_burst")]
    pub messaging_burst: u32,
}

#[derive(Debug, Clone)]
pub struct SecretsPolicy {
    allowed: HashSet<String>,
    allow_all: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowRetryConfig {
    #[serde(default = "default_retry_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_base_delay_ms")]
    pub base_delay_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WebhookPolicy {
    allow_paths: Vec<String>,
    deny_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookBindingConfig {
    #[serde(default)]
    pub allow_paths: Vec<String>,
    #[serde(default)]
    pub deny_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimerBinding {
    pub flow_id: String,
    pub cron: String,
    #[serde(default)]
    pub schedule_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthConfig {
    pub http_base_url: String,
    pub nats_url: String,
    pub provider: String,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
}

impl HostConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read bindings file {path:?}"))?;
        let bindings: BindingsFile = serde_yaml::from_str(&content)
            .with_context(|| format!("failed to parse bindings file {path:?}"))?;

        let secrets_policy = SecretsPolicy::from_bindings(&bindings);
        let http_enabled = bindings.flow_type_bindings.contains_key("messaging");
        let webhook_policy = bindings
            .flow_type_bindings
            .get("webhook")
            .and_then(|binding| {
                serde_yaml::from_value::<WebhookBindingConfig>(binding.config.clone())
                    .map(WebhookPolicy::from)
                    .map_err(|err| {
                        tracing::warn!(error = %err, "failed to parse webhook binding config");
                        err
                    })
                    .ok()
            })
            .unwrap_or_default();

        Ok(Self {
            tenant: bindings.tenant.clone(),
            bindings_path: path.to_path_buf(),
            flow_type_bindings: bindings.flow_type_bindings.clone(),
            rate_limits: bindings.rate_limits.clone(),
            retry: bindings.retry.clone(),
            http_enabled,
            secrets_policy,
            webhook_policy,
            timers: bindings.timers.clone(),
            oauth: bindings.oauth.clone(),
            mocks: bindings.mocks.clone(),
        })
    }

    pub fn messaging_binding(&self) -> Option<&FlowBinding> {
        self.flow_type_bindings.get("messaging")
    }

    pub fn retry_config(&self) -> FlowRetryConfig {
        self.retry.clone()
    }

    pub fn oauth_broker_config(&self) -> Option<OAuthBrokerConfig> {
        let oauth = self.oauth.as_ref()?;
        let mut cfg = OAuthBrokerConfig::new(&oauth.http_base_url, &oauth.nats_url);
        if !oauth.provider.is_empty() {
            cfg.default_provider = Some(oauth.provider.clone());
        }
        if let Some(team) = &oauth.team
            && !team.is_empty()
        {
            cfg.team = Some(team.clone());
        }
        Some(cfg)
    }
}

impl SecretsPolicy {
    fn from_bindings(bindings: &BindingsFile) -> Self {
        let allowed = bindings
            .flow_type_bindings
            .values()
            .flat_map(|binding| binding.secrets.iter().cloned())
            .collect::<HashSet<_>>();
        Self {
            allowed,
            allow_all: false,
        }
    }

    pub fn is_allowed(&self, key: &str) -> bool {
        self.allow_all || self.allowed.contains(key)
    }

    pub fn allow_all() -> Self {
        Self {
            allowed: HashSet::new(),
            allow_all: true,
        }
    }
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            messaging_send_qps: default_messaging_qps(),
            messaging_burst: default_messaging_burst(),
        }
    }
}

fn default_messaging_qps() -> u32 {
    10
}

fn default_messaging_burst() -> u32 {
    20
}

impl From<WebhookBindingConfig> for WebhookPolicy {
    fn from(value: WebhookBindingConfig) -> Self {
        Self {
            allow_paths: value.allow_paths,
            deny_paths: value.deny_paths,
        }
    }
}

impl WebhookPolicy {
    pub fn is_allowed(&self, path: &str) -> bool {
        if self
            .deny_paths
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            return false;
        }

        if self.allow_paths.is_empty() {
            return true;
        }

        self.allow_paths
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }
}

impl TimerBinding {
    pub fn schedule_id(&self) -> &str {
        self.schedule_id.as_deref().unwrap_or(self.flow_id.as_str())
    }
}

impl Default for FlowRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_retry_attempts(),
            base_delay_ms: default_retry_base_delay_ms(),
        }
    }
}

fn default_retry_attempts() -> u32 {
    3
}

fn default_retry_base_delay_ms() -> u64 {
    250
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn host_config_with_oauth(oauth: Option<OAuthConfig>) -> HostConfig {
        HostConfig {
            tenant: "tenant-a".to_string(),
            bindings_path: PathBuf::from("/tmp/bindings.yaml"),
            flow_type_bindings: HashMap::new(),
            rate_limits: RateLimits::default(),
            retry: FlowRetryConfig::default(),
            http_enabled: false,
            secrets_policy: SecretsPolicy::allow_all(),
            webhook_policy: WebhookPolicy::default(),
            timers: Vec::new(),
            oauth,
            mocks: None,
        }
    }

    #[test]
    fn oauth_broker_config_absent_without_block() {
        let cfg = host_config_with_oauth(None);
        assert!(cfg.oauth_broker_config().is_none());
    }

    #[test]
    fn oauth_broker_config_maps_fields() {
        let cfg = host_config_with_oauth(Some(OAuthConfig {
            http_base_url: "https://oauth.example/".into(),
            nats_url: "nats://broker:4222".into(),
            provider: "demo".into(),
            env: None,
            team: Some("ops".into()),
        }));
        let broker = cfg.oauth_broker_config().expect("missing broker config");
        assert_eq!(broker.http_base_url, "https://oauth.example/");
        assert_eq!(broker.nats_url, "nats://broker:4222");
        assert_eq!(broker.default_provider.as_deref(), Some("demo"));
        assert_eq!(broker.team.as_deref(), Some("ops"));
    }
}
