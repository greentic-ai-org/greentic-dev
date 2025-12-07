use super::super::error::GResult;
use super::super::registry::{Adapter, AdapterCall};
use async_trait::async_trait;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

type AdapterFuture = dyn Future<Output = GResult<Value>> + Send;
type AdapterInvoker = dyn Fn(AdapterCall) -> Pin<Box<AdapterFuture>> + Send + Sync;

#[async_trait]
pub trait AdapterBridge: Send + Sync {
    async fn invoke(&self, call: AdapterCall) -> GResult<Value>;
}

#[async_trait]
impl<T> Adapter for T
where
    T: AdapterBridge,
{
    async fn call(&self, call: &AdapterCall) -> GResult<Value> {
        self.invoke(call.clone()).await
    }
}

pub struct FnAdapterBridge {
    inner: Arc<AdapterInvoker>,
}

impl FnAdapterBridge {
    pub fn new<F, Fut>(func: F) -> Self
    where
        F: Send + Sync + 'static + Fn(AdapterCall) -> Fut,
        Fut: Future<Output = GResult<Value>> + Send + 'static,
    {
        let invoker: Arc<AdapterInvoker> = Arc::new(move |call: AdapterCall| {
            let fut = func(call);
            Box::pin(fut) as Pin<Box<AdapterFuture>>
        });
        Self { inner: invoker }
    }
}

#[async_trait]
impl AdapterBridge for FnAdapterBridge {
    async fn invoke(&self, call: AdapterCall) -> GResult<Value> {
        (self.inner)(call).await
    }
}
