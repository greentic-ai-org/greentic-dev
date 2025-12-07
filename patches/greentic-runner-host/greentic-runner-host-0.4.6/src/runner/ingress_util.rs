use axum::body::Body;
use axum::http::StatusCode;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use lru::LruCache;
use parking_lot::Mutex;
use serde_json::Value;

pub fn mark_processed(cache: &Mutex<LruCache<String, Value>>, key: &str) -> bool {
    let mut cache = cache.lock();
    if cache.get(key).is_some() {
        true
    } else {
        cache.put(key.to_string(), Value::Null);
        false
    }
}

pub async fn collect_body(body: Body) -> Result<Bytes, StatusCode> {
    let mut stream = body.into_data_stream();
    let mut data = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        data.extend_from_slice(&chunk);
    }
    Ok(data.freeze())
}
