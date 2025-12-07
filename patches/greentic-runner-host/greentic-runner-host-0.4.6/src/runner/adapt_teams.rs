use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    let raw_value: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let activity: TeamsActivity =
        serde_json::from_value(raw_value.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(id) = activity.id.as_deref()
        && mark_processed(runtime.webhook_cache(), id)
    {
        return Ok(StatusCode::ACCEPTED);
    }

    let provider_ids = build_provider_ids(&activity)?;
    let session_key = canonical_session_key(&tenant, "teams", &provider_ids);
    let timestamp = parse_timestamp(activity.timestamp.as_deref())?;
    let attachments = map_attachments(&activity);
    let scopes = base_scopes(!attachments.is_empty());
    let channel_data = build_channel_data(&activity);
    let metadata = default_metadata();

    let payload = build_canonical_payload(
        &tenant,
        "teams",
        &provider_ids,
        session_key.clone(),
        &scopes,
        timestamp,
        activity.locale.clone(),
        activity.text.clone(),
        attachments,
        Vec::new(),
        empty_entities(),
        metadata,
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
        provider: Some("teams".into()),
        channel: provider_ids
            .thread_id
            .clone()
            .or_else(|| provider_ids.channel_id.clone())
            .or_else(|| provider_ids.conversation_id.clone()),
        conversation: provider_ids.conversation_id.clone(),
        user: provider_ids.user_id.clone(),
        activity_id: provider_ids.message_id.clone(),
        timestamp: Some(timestamp.to_rfc3339()),
        payload,
        metadata: None,
    }
    .canonicalize();

    runtime
        .state_machine()
        .handle(envelope)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "teams flow execution failed");
            StatusCode::BAD_GATEWAY
        })?;
    Ok(StatusCode::ACCEPTED)
}

fn build_provider_ids(activity: &TeamsActivity) -> Result<ProviderIds, StatusCode> {
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
        team_id: activity
            .channel_data
            .as_ref()
            .and_then(|cd| cd.team.as_ref())
            .and_then(|team| team.id.clone()),
        conversation_id,
        thread_id: activity.reply_to_id.clone(),
        channel_id: activity
            .channel_data
            .as_ref()
            .and_then(|cd| cd.channel.as_ref())
            .and_then(|channel| channel.id.clone()),
        user_id: Some(user_id),
        message_id: activity.id.clone(),
        event_id: activity.reply_to_id.clone(),
        ..ProviderIds::default()
    })
}

fn base_scopes(has_attachments: bool) -> Vec<String> {
    let mut scopes = vec!["chat".to_string()];
    if has_attachments {
        scopes.push("attachments".into());
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

fn map_attachments(activity: &TeamsActivity) -> Vec<Value> {
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
        Some(_) => "file".into(),
        None => "file".into(),
    }
}

fn build_channel_data(activity: &TeamsActivity) -> Value {
    let mut data = Map::new();
    if let Some(channel_data) = activity.channel_data.as_ref() {
        data.insert(
            "channel_data".into(),
            serde_json::to_value(channel_data).unwrap_or(Value::Null),
        );
    } else {
        data.insert("channel_data".into(), Value::Null);
    }
    data.insert(
        "service_url".into(),
        activity
            .service_url
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    Value::Object(data)
}

#[derive(Debug, Deserialize)]
struct TeamsActivity {
    #[serde(rename = "type")]
    #[serde(default)]
    #[allow(dead_code)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    service_url: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    from: Option<TeamsUser>,
    #[serde(default)]
    conversation: Option<TeamsConversation>,
    #[serde(rename = "replyToId")]
    #[serde(default)]
    reply_to_id: Option<String>,
    #[serde(default)]
    attachments: Vec<TeamsAttachment>,
    #[serde(rename = "channelData")]
    #[serde(default)]
    channel_data: Option<TeamsChannelData>,
}

#[derive(Debug, Deserialize)]
struct TeamsUser {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamsConversation {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "tenantId")]
    #[serde(default)]
    #[allow(dead_code)]
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamsAttachment {
    #[serde(rename = "contentType")]
    #[serde(default)]
    content_type: Option<String>,
    #[serde(rename = "contentUrl")]
    #[serde(default)]
    content_url: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TeamsChannelData {
    #[serde(default)]
    team: Option<TeamsResourceId>,
    #[serde(default)]
    tenant: Option<TeamsResourceId>,
    #[serde(default)]
    channel: Option<TeamsResourceId>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TeamsResourceId {
    #[serde(default)]
    id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn teams_activity_maps_to_canonical_payload() {
        let raw = json!({
            "type": "message",
            "id": "activity-id",
            "timestamp": "2025-11-11T09:00:00Z",
            "serviceUrl": "https://smba.trafficmanager.net/amer/",
            "from": { "id": "user-123" },
            "conversation": { "id": "conv-abc", "tenantId": "tenant-guid" },
            "replyToId": "thread-xyz",
            "text": "Hello",
            "channelData": {
                "team": { "id": "team-1" },
                "channel": { "id": "channel-1" }
            }
        });
        let activity: TeamsActivity = serde_json::from_value(raw.clone()).unwrap();
        let provider_ids = build_provider_ids(&activity).unwrap();
        assert_eq!(provider_ids.conversation_id.as_deref(), Some("conv-abc"));
        assert_eq!(provider_ids.thread_id.as_deref(), Some("thread-xyz"));
        assert_eq!(provider_ids.team_id.as_deref(), Some("team-1"));
        let session_key = canonical_session_key("acme", "teams", &provider_ids);
        assert_eq!(session_key, "acme:teams:thread-xyz:user-123");
        let timestamp = parse_timestamp(activity.timestamp.as_deref()).unwrap();
        let payload = build_canonical_payload(
            "acme",
            "teams",
            &provider_ids,
            session_key,
            &["chat".into()],
            timestamp,
            activity.locale.clone(),
            activity.text.clone(),
            Vec::new(),
            Vec::new(),
            empty_entities(),
            default_metadata(),
            build_channel_data(&activity),
            raw,
        );
        assert_eq!(payload["provider"], json!("teams"));
        assert_eq!(payload["text"], json!("Hello"));
    }
}
