use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha1::Sha1;

use crate::engine::runtime::IngressEnvelope;
use crate::ingress::{
    CanonicalAttachment, ProviderIds, build_canonical_payload, canonical_session_key,
    default_metadata, empty_entities,
};
use crate::routing::TenantRuntimeHandle;
use crate::runner::ingress_util::{collect_body, mark_processed};

type HmacSha1 = Hmac<Sha1>;

pub async fn webhook(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    request: Request<Body>,
) -> Result<StatusCode, StatusCode> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let bytes = collect_body(body).await?;
    verify_signature(&headers, &bytes)?;

    let raw_value: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let payload: WebexWebhook =
        serde_json::from_value(raw_value.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let message = payload.data.ok_or(StatusCode::BAD_REQUEST)?;

    if mark_processed(runtime.webhook_cache(), message.id.as_str()) {
        return Ok(StatusCode::ACCEPTED);
    }

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    let provider_ids = ProviderIds {
        conversation_id: Some(message.room_id.clone()),
        thread_id: message.parent_id.clone(),
        user_id: message.person_id.clone().or(message.person_email.clone()),
        message_id: Some(message.id.clone()),
        ..ProviderIds::default()
    };

    let session_key = canonical_session_key(&tenant, "webex", &provider_ids);
    let timestamp = parse_timestamp(payload.created.as_deref())?;
    let mut attachments = map_attachments(message.files.as_ref());
    let scopes = if attachments.is_empty() {
        vec!["chat".to_string()]
    } else {
        vec!["chat".to_string(), "attachments".to_string()]
    };
    let channel_data = json!({
        "resource": payload.resource,
        "event": payload.event,
        "roomType": message.room_type,
    });
    let canonical_payload = build_canonical_payload(
        &tenant,
        "webex",
        &provider_ids,
        session_key.clone(),
        &scopes,
        timestamp,
        None,
        message.text.clone(),
        {
            let mut vals = Vec::new();
            std::mem::swap(&mut vals, &mut attachments);
            vals
        },
        Vec::new(),
        empty_entities(),
        default_metadata(),
        channel_data,
        raw_value,
    );

    let envelope = IngressEnvelope {
        tenant,
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(session_key),
        provider: Some("webex".into()),
        channel: Some(message.room_id.clone()),
        conversation: Some(message.room_id.clone()),
        user: provider_ids.user_id.clone(),
        activity_id: Some(message.id.clone()),
        timestamp: Some(timestamp.to_rfc3339()),
        payload: canonical_payload,
        metadata: None,
    }
    .canonicalize();

    runtime
        .state_machine()
        .handle(envelope)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "webex flow execution failed");
            StatusCode::BAD_GATEWAY
        })?;
    Ok(StatusCode::ACCEPTED)
}

fn verify_signature(headers: &HeaderMap, body: &[u8]) -> Result<(), StatusCode> {
    if let Ok(secret) = std::env::var("WEBEX_WEBHOOK_SECRET") {
        let signature = headers
            .get("X-Spark-Signature")
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?
            .to_ascii_lowercase();
        let mut mac =
            HmacSha1::new_from_slice(secret.as_bytes()).map_err(|_| StatusCode::UNAUTHORIZED)?;
        mac.update(body);
        let expected = hex::encode(mac.finalize().into_bytes());
        if !subtle_equals(&signature, &expected) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(())
}

fn subtle_equals(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn parse_timestamp(raw: Option<&str>) -> Result<DateTime<Utc>, StatusCode> {
    if let Some(raw) = raw {
        return DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| StatusCode::BAD_REQUEST);
    }
    Ok(Utc::now())
}

fn map_attachments(files: Option<&Vec<String>>) -> Vec<Value> {
    files
        .into_iter()
        .flat_map(|f| f.iter())
        .map(|file| {
            CanonicalAttachment {
                attachment_type: "file".into(),
                name: None,
                mime: None,
                size: None,
                url: Some(file.clone()),
                data_inline_b64: None,
            }
            .into_value()
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct WebexWebhook {
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    created: Option<String>,
    data: Option<WebexMessageData>,
}

#[derive(Debug, Deserialize)]
struct WebexMessageData {
    id: String,
    #[serde(rename = "roomId")]
    room_id: String,
    #[serde(rename = "roomType")]
    #[serde(default)]
    room_type: Option<String>,
    #[serde(rename = "parentId")]
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(rename = "personId")]
    #[serde(default)]
    person_id: Option<String>,
    #[serde(rename = "personEmail")]
    #[serde(default)]
    person_email: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    files: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn webex_webhook_maps_to_canonical_payload() {
        let raw = json!({
            "resource": "messages",
            "event": "created",
            "created": "2025-11-11T09:00:00Z",
            "data": {
                "id": "msg-id",
                "roomId": "room-123",
                "roomType": "group",
                "personId": "person-456",
                "parentId": "parent-789",
                "text": "Hello Webex",
                "files": ["https://files.example.com/doc.pdf"]
            }
        });
        let mut payload: WebexWebhook = serde_json::from_value(raw.clone()).unwrap();
        let resource = payload.resource.clone();
        let event = payload.event.clone();
        let created = payload.created.clone();
        let message = payload.data.take().unwrap();
        let provider_ids = ProviderIds {
            conversation_id: Some(message.room_id.clone()),
            thread_id: message.parent_id.clone(),
            user_id: message.person_id.clone(),
            message_id: Some(message.id.clone()),
            ..ProviderIds::default()
        };
        let session_key = canonical_session_key("cisco", "webex", &provider_ids);
        assert_eq!(session_key, "cisco:webex:parent-789:person-456");
        let timestamp = parse_timestamp(created.as_deref()).unwrap();
        let attachments = map_attachments(message.files.as_ref());
        let scopes = vec!["chat".to_string(), "attachments".to_string()];
        let channel_data = json!({
            "resource": resource,
            "event": event,
            "roomType": message.room_type,
        });
        let canonical = build_canonical_payload(
            "cisco",
            "webex",
            &provider_ids,
            session_key,
            &scopes,
            timestamp,
            None,
            message.text.clone(),
            attachments,
            Vec::new(),
            empty_entities(),
            default_metadata(),
            channel_data,
            raw,
        );

        assert_eq!(canonical["provider"], json!("webex"));
        assert_eq!(canonical["provider_ids"]["thread_id"], json!("parent-789"));
        assert_eq!(
            canonical["session"]["key"],
            json!("cisco:webex:parent-789:person-456")
        );
        assert_eq!(
            canonical["attachments"][0]["url"],
            json!("https://files.example.com/doc.pdf")
        );
    }
}
