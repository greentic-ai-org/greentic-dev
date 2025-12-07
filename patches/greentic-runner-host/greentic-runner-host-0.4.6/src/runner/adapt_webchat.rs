use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::engine::runtime::IngressEnvelope;
use crate::ingress::{
    CanonicalAttachment, ProviderIds, build_canonical_payload, canonical_session_key,
    default_metadata, empty_entities,
};
use crate::routing::TenantRuntimeHandle;
use crate::runner::ingress_util::{collect_body, mark_processed};

pub async fn activities(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    request: Request<Body>,
) -> Result<StatusCode, StatusCode> {
    let (_, body) = request.into_parts();
    let bytes = collect_body(body).await?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let activity: WebChatActivity =
        serde_json::from_value(raw.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    if activity
        .id
        .as_deref()
        .is_some_and(|event_id| mark_processed(runtime.webhook_cache(), event_id))
    {
        return Ok(StatusCode::ACCEPTED);
    }

    let provider_ids = build_provider_ids(&activity)?;
    let session_key = canonical_session_key(&tenant, "webchat", &provider_ids);
    let timestamp = parse_timestamp(activity.timestamp.as_deref())?;
    let locale = activity.locale.clone();
    let text = activity.text.clone();
    let attachments = map_attachments(&activity);
    let scopes = base_scopes(!attachments.is_empty());
    let channel_data = build_channel_data(&activity);
    let metadata = default_metadata();
    let canonical_payload = build_canonical_payload(
        &tenant,
        "webchat",
        &provider_ids,
        session_key.clone(),
        &scopes,
        timestamp,
        locale,
        text,
        attachments,
        Vec::new(),
        empty_entities(),
        metadata,
        channel_data,
        raw.clone(),
    );

    let envelope = IngressEnvelope {
        tenant,
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(session_key),
        provider: Some("webchat".into()),
        channel: provider_ids
            .conversation_id
            .clone()
            .or_else(|| provider_ids.channel_id.clone()),
        conversation: provider_ids.conversation_id.clone(),
        user: provider_ids.user_id.clone(),
        activity_id: provider_ids
            .message_id
            .clone()
            .or_else(|| provider_ids.event_id.clone()),
        timestamp: Some(timestamp.to_rfc3339()),
        payload: canonical_payload,
        metadata: None,
    }
    .canonicalize();

    match runtime.state_machine().handle(envelope).await {
        Ok(_) => Ok(StatusCode::ACCEPTED),
        Err(err) => {
            tracing::error!(error = %err, "webchat flow execution failed");
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

fn base_scopes(has_attachments: bool) -> Vec<String> {
    let mut scopes = vec!["chat".to_string()];
    if has_attachments {
        scopes.push("attachments".to_string());
    }
    scopes
}

fn parse_timestamp(raw: Option<&str>) -> Result<DateTime<Utc>, StatusCode> {
    if let Some(value) = raw {
        DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| StatusCode::BAD_REQUEST)
    } else {
        Ok(Utc::now())
    }
}

fn build_provider_ids(activity: &WebChatActivity) -> Result<ProviderIds, StatusCode> {
    let conversation_id = activity
        .conversation
        .as_ref()
        .and_then(|conv| conv.id.clone());
    let user_id = activity
        .from
        .as_ref()
        .and_then(|from| from.id.clone())
        .ok_or(StatusCode::BAD_REQUEST)?;

    Ok(ProviderIds {
        conversation_id,
        user_id: Some(user_id),
        message_id: activity.id.clone(),
        event_id: activity.id.clone(),
        ..ProviderIds::default()
    })
}

fn build_channel_data(activity: &WebChatActivity) -> Value {
    let mut data = Map::new();
    data.insert(
        "type".into(),
        Value::String(activity.kind.clone().unwrap_or_else(|| "message".into())),
    );
    data.insert(
        "channel_data".into(),
        activity
            .channel_data
            .clone()
            .unwrap_or(Value::Object(Map::new())),
    );
    Value::Object(data)
}

fn map_attachments(activity: &WebChatActivity) -> Vec<Value> {
    activity
        .attachments
        .iter()
        .map(|att| {
            CanonicalAttachment {
                attachment_type: infer_attachment_type(att.content_type.as_deref()),
                name: att.name.clone(),
                mime: att.content_type.clone(),
                size: att.size,
                url: att.content_url.clone(),
                data_inline_b64: att
                    .content
                    .as_ref()
                    .and_then(|value| value.as_str())
                    .map(|s| s.to_string()),
            }
            .into_value()
        })
        .collect()
}

fn infer_attachment_type(content_type: Option<&str>) -> String {
    match content_type {
        Some(value) if value.starts_with("image/") => "image".into(),
        Some(value) if value.starts_with("audio/") => "audio".into(),
        Some(value) if value.starts_with("video/") => "video".into(),
        Some(value) if value.contains("application/json") => "card".into(),
        Some(_) => "file".into(),
        None => "file".into(),
    }
}

#[derive(Debug, Deserialize)]
struct WebChatActivity {
    #[serde(rename = "type")]
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    from: Option<WebChatActor>,
    #[serde(default)]
    conversation: Option<WebChatConversation>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    attachments: Vec<WebChatAttachment>,
    #[serde(default)]
    channel_data: Option<Value>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebChatActor {
    #[serde(default)]
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebChatConversation {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebChatAttachment {
    #[serde(rename = "contentType")]
    #[serde(default)]
    content_type: Option<String>,
    #[serde(rename = "contentUrl")]
    #[serde(default)]
    content_url: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    content: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn webchat_activity_maps_to_canonical_payload() {
        let raw = json!({
            "type": "message",
            "id": "activity-id",
            "timestamp": "2025-11-11T09:00:00Z",
            "from": { "id": "user-123" },
            "conversation": { "id": "conv-abc" },
            "text": "Hello",
            "attachments": [{
                "contentType": "image/png",
                "contentUrl": "https://example.com/pic.png",
                "name": "pic.png",
                "size": 2048
            }],
            "channel_data": { "tenant": "demo" }
        });
        let activity: WebChatActivity = serde_json::from_value(raw.clone()).unwrap();
        let provider_ids = build_provider_ids(&activity).unwrap();
        assert_eq!(provider_ids.conversation_id.as_deref(), Some("conv-abc"));
        let session_key = canonical_session_key("demo", "webchat", &provider_ids);
        assert_eq!(session_key, "demo:webchat:conv-abc:user-123");
        let timestamp = parse_timestamp(activity.timestamp.as_deref()).unwrap();
        let attachments = map_attachments(&activity);
        let scopes = base_scopes(!attachments.is_empty());
        let channel_data = build_channel_data(&activity);
        let canonical = build_canonical_payload(
            "demo",
            "webchat",
            &provider_ids,
            session_key,
            &scopes,
            timestamp,
            activity.locale.clone(),
            activity.text.clone(),
            attachments,
            Vec::new(),
            empty_entities(),
            default_metadata(),
            channel_data,
            raw,
        );

        assert_eq!(canonical["provider"], json!("webchat"));
        assert_eq!(canonical["attachments"][0]["type"], json!("image"));
        assert_eq!(
            canonical["session"]["scopes"],
            json!(["chat", "attachments"])
        );
        assert_eq!(
            canonical["session"]["key"],
            json!("demo:webchat:conv-abc:user-123")
        );
    }
}
