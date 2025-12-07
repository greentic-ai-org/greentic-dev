use anyhow::Result;
use wasmtime::component::Linker;

#[derive(Clone, Debug, Default)]
pub struct OAuthBrokerConfig {
    pub http_base_url: String,
    pub nats_url: String,
    pub default_provider: Option<String>,
    pub team: Option<String>,
}

impl OAuthBrokerConfig {
    pub fn new(http_base_url: impl Into<String>, nats_url: impl Into<String>) -> Self {
        Self {
            http_base_url: http_base_url.into(),
            nats_url: nats_url.into(),
            default_provider: None,
            team: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OAuthBrokerHost;

pub trait OAuthHostContext {
    fn tenant_id(&self) -> &str;
    fn env(&self) -> &str;
    fn oauth_broker_host(&mut self) -> &mut OAuthBrokerHost;
    fn oauth_config(&self) -> Option<&OAuthBrokerConfig>;
}

pub fn add_oauth_broker_to_linker<T>(_linker: &mut Linker<T>) -> Result<()> {
    Ok(())
}
