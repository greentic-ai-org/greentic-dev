use super::super::error::GResult;
use super::super::host::SecretsHost;
use async_trait::async_trait;
use std::sync::Arc;

type SecretsFn = dyn Fn(&str) -> GResult<String> + Send + Sync;

pub struct FnSecretsHost {
    inner: Arc<SecretsFn>,
}

impl FnSecretsHost {
    pub fn new<F>(func: F) -> Self
    where
        F: Send + Sync + 'static + Fn(&str) -> GResult<String>,
    {
        Self {
            inner: Arc::new(func),
        }
    }
}

#[async_trait]
impl SecretsHost for FnSecretsHost {
    async fn get(&self, name: &str) -> GResult<String> {
        (self.inner)(name)
    }
}
