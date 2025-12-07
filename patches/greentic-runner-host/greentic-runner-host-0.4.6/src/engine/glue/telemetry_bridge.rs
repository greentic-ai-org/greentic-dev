use super::super::error::GResult;
use super::super::host::{SpanContext, TelemetryHost};
use async_trait::async_trait;
use std::sync::Arc;

type TelemetryFn = dyn Fn(&SpanContext, &[(&str, &str)]) -> GResult<()> + Send + Sync;

pub struct FnTelemetryHost {
    inner: Arc<TelemetryFn>,
}

impl FnTelemetryHost {
    pub fn new<F>(func: F) -> Self
    where
        F: Send + Sync + 'static + Fn(&SpanContext, &[(&str, &str)]) -> GResult<()>,
    {
        Self {
            inner: Arc::new(func),
        }
    }
}

#[async_trait]
impl TelemetryHost for FnTelemetryHost {
    async fn emit(&self, span: &SpanContext, fields: &[(&str, &str)]) -> GResult<()> {
        (self.inner)(span, fields)
    }
}
