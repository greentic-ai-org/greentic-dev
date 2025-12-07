use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use greentic_secrets::SecretsManager;
use greentic_secrets::env::EnvSecretsManager;
use tokio::runtime::{Handle, Runtime};

/// Shared secrets manager handle used by the host.
pub type DynSecretsManager = Arc<dyn SecretsManager>;

/// Supported secrets backend kinds recognised by the runner.
#[derive(Clone, Debug)]
pub enum SecretsBackend {
    Env,
}

impl SecretsBackend {
    pub fn from_env(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "env".into())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "env" => Ok(SecretsBackend::Env),
            other => Err(anyhow!("unsupported SECRETS_BACKEND `{other}`")),
        }
    }

    pub fn build_manager(&self) -> Result<DynSecretsManager> {
        match self {
            SecretsBackend::Env => Ok(Arc::new(EnvSecretsManager) as DynSecretsManager),
        }
    }
}

pub fn default_manager() -> DynSecretsManager {
    Arc::new(EnvSecretsManager) as DynSecretsManager
}

pub fn read_secret_blocking(manager: &DynSecretsManager, key: &str) -> Result<Vec<u8>> {
    let bytes = if let Ok(handle) = Handle::try_current() {
        handle
            .block_on(manager.read(key))
            .map_err(|err| anyhow!(err.to_string()))?
    } else {
        Runtime::new()
            .context("failed to initialise secrets runtime")?
            .block_on(manager.read(key))
            .map_err(|err| anyhow!(err.to_string()))?
    };
    Ok(bytes)
}
