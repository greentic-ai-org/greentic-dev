use std::collections::HashSet;

use anyhow::Result;
use axum::body::Body;
use axum::extract::Form;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::engine::runtime::IngressEnvelope;
use crate::ingress::{
    CanonicalAttachment, CanonicalButton, ProviderIds, build_canonical_payload,
    canonical_session_key, default_metadata, empty_entities,
};
use crate::routing::TenantRuntimeHandle;
use crate::runner::ingress_util::{collect_body, mark_processed};

type HmacSha256 = Hmac<Sha256>;

pub async fn events(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let bytes = collect_body(body).await?;
    verify_slack_signature(&headers, &bytes)?;

    let raw_value: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let payload: SlackEventEnvelope =
        serde_json::from_value(raw_value.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    if payload.payload_type == "url_verification" {
        if let Some(challenge) = payload.challenge {
            let response = axum::Json(json!({ "challenge": challenge })).into_response();
            return Ok(response);
        }
        return Err(StatusCode::BAD_REQUEST);
    }

    let event = payload.event.as_ref().ok_or(StatusCode::BAD_REQUEST)?;

    if payload
        .event_id
        .as_deref()
        .is_some_and(|event_id| mark_processed(runtime.webhook_cache(), event_id))
    {
        return Ok(StatusCode::OK.into_response());
    }

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    let mapped = map_slack_event(&tenant, &payload, event, &raw_value)?;
    let envelope = IngressEnvelope {
        tenant,
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(mapped.session_key.clone()),
        provider: Some("slack".into()),
        channel: mapped
            .provider_ids
            .channel_id
            .clone()
            .or_else(|| mapped.provider_ids.conversation_id.clone()),
        conversation: mapped.provider_ids.conversation_id.clone(),
        user: mapped.provider_ids.user_id.clone(),
        activity_id: mapped
            .provider_ids
            .message_id
            .clone()
            .or_else(|| mapped.provider_ids.event_id.clone()),
        timestamp: Some(mapped.timestamp.to_rfc3339()),
        payload: mapped.payload,
        metadata: None,
    }
    .canonicalize();

    runtime
        .state_machine()
        .handle(envelope)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "slack flow execution failed");
            StatusCode::BAD_GATEWAY
        })?;
    Ok(StatusCode::OK.into_response())
}

pub async fn interactive(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    headers: HeaderMap,
    Form(body): Form<SlackInteractiveForm>,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(secret) = signing_secret() {
        let timestamp = headers
            .get("X-Slack-Request-Timestamp")
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let sig = headers
            .get("X-Slack-Signature")
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let base_string = format!("v0:{timestamp}:payload={}", body.payload);
        if !verify_hmac(&secret, &base_string, sig) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    let raw_value: Value =
        serde_json::from_str(&body.payload).map_err(|_| StatusCode::BAD_REQUEST)?;
    let payload: SlackInteractivePayload =
        serde_json::from_value(raw_value.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    let mapped = map_slack_interactive(&tenant, &payload, &raw_value)?;
    if mapped
        .provider_ids
        .event_id
        .as_deref()
        .is_some_and(|dedupe| mark_processed(runtime.webhook_cache(), dedupe))
    {
        return Ok(StatusCode::OK);
    }

    let envelope = IngressEnvelope {
        tenant,
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(mapped.session_key.clone()),
        provider: Some("slack".into()),
        channel: mapped.provider_ids.channel_id.clone(),
        conversation: mapped.provider_ids.conversation_id.clone(),
        user: mapped.provider_ids.user_id.clone(),
        activity_id: mapped.provider_ids.event_id.clone(),
        timestamp: Some(mapped.timestamp.to_rfc3339()),
        payload: mapped.payload,
        metadata: None,
    }
    .canonicalize();

    runtime
        .state_machine()
        .handle(envelope)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "slack interactive flow failed");
            StatusCode::BAD_GATEWAY
        })?;
    Ok(StatusCode::OK)
}

fn map_slack_event(
    tenant: &str,
    payload: &SlackEventEnvelope,
    event: &SlackEvent,
    raw: &Value,
) -> Result<MappedCanonical, StatusCode> {
    let provider_ids = ProviderIds {
        workspace_id: payload.team_id.clone(),
        channel_id: event.channel.clone(),
        thread_id: event.thread_ts.clone(),
        conversation_id: event.thread_ts.clone().or(event.channel.clone()),
        user_id: event.user.clone(),
        message_id: event.ts.clone(),
        event_id: payload.event_id.clone(),
        ..ProviderIds::default()
    };
    if provider_ids.user_id.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let session_key = canonical_session_key(tenant, "slack", &provider_ids);
    let timestamp = parse_slack_timestamp(event.ts.as_deref(), payload.event_time)?;
    let mut attachments = map_slack_files(event.files.as_deref());
    let buttons = buttons_from_event(event);
    let mut scopes = base_scopes(!attachments.is_empty());
    if !buttons.is_empty() {
        scopes.insert("buttons".into());
    }
    let scopes_vec: Vec<String> = scopes.into_iter().collect();
    let payload_value = build_canonical_payload(
        tenant,
        "slack",
        &provider_ids,
        session_key.clone(),
        &scopes_vec,
        timestamp,
        event.locale.clone(),
        event.text.clone(),
        {
            let mut vals = Vec::new();
            std::mem::swap(&mut vals, &mut attachments);
            vals
        },
        buttons,
        empty_entities(),
        default_metadata(),
        json!({"type": event.event_type, "subtype": event.subtype}),
        raw.clone(),
    );
    Ok(MappedCanonical {
        provider_ids,
        session_key,
        timestamp,
        payload: payload_value,
    })
}

fn map_slack_interactive(
    tenant: &str,
    payload: &SlackInteractivePayload,
    raw: &Value,
) -> Result<MappedCanonical, StatusCode> {
    let provider_ids = ProviderIds {
        workspace_id: payload.team.as_ref().map(|team| team.id.clone()),
        channel_id: payload.channel.as_ref().map(|c| c.id.clone()),
        conversation_id: payload.channel.as_ref().map(|c| c.id.clone()),
        user_id: payload.user.as_ref().map(|u| u.id.clone()),
        event_id: payload.trigger_id.clone().or(payload.action_ts.clone()),
        ..ProviderIds::default()
    };
    if provider_ids.user_id.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let session_key = canonical_session_key(tenant, "slack", &provider_ids);
    let timestamp = parse_slack_timestamp(payload.action_ts.as_deref(), None)?;
    let buttons = payload
        .actions
        .iter()
        .map(|action| {
            CanonicalButton {
                id: action.action_id.clone(),
                title: action
                    .text
                    .as_ref()
                    .and_then(|text| text.get("text").and_then(Value::as_str))
                    .unwrap_or("Button")
                    .to_string(),
                payload: action
                    .value
                    .clone()
                    .or_else(|| {
                        action
                            .selected_option
                            .as_ref()
                            .and_then(|opt| opt.value.clone())
                    })
                    .unwrap_or_default(),
            }
            .into_value()
        })
        .collect::<Vec<_>>();
    let scopes = vec!["chat".to_string(), "buttons".to_string()];
    let payload_value = build_canonical_payload(
        tenant,
        "slack",
        &provider_ids,
        session_key.clone(),
        &scopes,
        timestamp,
        None,
        payload.message.as_ref().and_then(|msg| msg.text.clone()),
        Vec::new(),
        buttons,
        empty_entities(),
        default_metadata(),
        json!({"type": payload.payload_type}),
        raw.clone(),
    );
    Ok(MappedCanonical {
        provider_ids,
        session_key,
        timestamp,
        payload: payload_value,
    })
}

fn buttons_from_event(event: &SlackEvent) -> Vec<Value> {
    let mut buttons = Vec::new();
    if let Some(blocks) = &event.blocks {
        for block in blocks {
            if let Some(elements) = block.get("elements").and_then(Value::as_array) {
                for elem in elements {
                    if elem.get("type") == Some(&Value::String("button".into())) {
                        let id = elem
                            .get("action_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let title = elem
                            .get("text")
                            .and_then(|text| text.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("Button");
                        let payload = elem
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        buttons.push(
                            CanonicalButton {
                                id: id.to_string(),
                                title: title.to_string(),
                                payload,
                            }
                            .into_value(),
                        );
                    }
                }
            }
        }
    }
    buttons
}

fn map_slack_files(files: Option<&[SlackFile]>) -> Vec<Value> {
    files
        .into_iter()
        .flat_map(|items| items.iter())
        .map(|file| {
            CanonicalAttachment {
                attachment_type: infer_slack_attachment_type(file),
                name: file.name.clone(),
                mime: file.mimetype.clone(),
                size: file.size.map(|s| s as u64),
                url: file.url_private.clone(),
                data_inline_b64: None,
            }
            .into_value()
        })
        .collect()
}

fn infer_slack_attachment_type(file: &SlackFile) -> String {
    match file.mimetype.as_deref().unwrap_or("") {
        mime if mime.starts_with("image/") => "image".into(),
        mime if mime.starts_with("audio/") => "audio".into(),
        mime if mime.starts_with("video/") => "video".into(),
        _ => "file".into(),
    }
}

fn base_scopes(has_attachments: bool) -> HashSet<String> {
    let mut scopes = HashSet::new();
    scopes.insert("chat".into());
    if has_attachments {
        scopes.insert("attachments".into());
    }
    scopes
}

fn parse_slack_timestamp(
    ts: Option<&str>,
    fallback: Option<i64>,
) -> Result<DateTime<Utc>, StatusCode> {
    if let Some(seconds) = ts
        .and_then(|value| value.split_once('.'))
        .and_then(|(secs, _)| secs.parse::<i64>().ok())
    {
        return DateTime::from_timestamp(seconds, 0).ok_or(StatusCode::BAD_REQUEST);
    }
    if let Some(seconds) = fallback {
        return DateTime::from_timestamp(seconds, 0).ok_or(StatusCode::BAD_REQUEST);
    }
    Ok(Utc::now())
}

fn verify_slack_signature(headers: &HeaderMap, body: &[u8]) -> Result<(), StatusCode> {
    if let Some(secret) = signing_secret() {
        let timestamp = headers
            .get("X-Slack-Request-Timestamp")
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let signature = headers
            .get("X-Slack-Signature")
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let base_string = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        if !verify_hmac(&secret, &base_string, signature) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(())
}

fn signing_secret() -> Option<String> {
    std::env::var("SLACK_SIGNING_SECRET").ok()
}

fn verify_hmac(secret: &str, base_string: &str, signature: &str) -> bool {
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(base_string.as_bytes());
    let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    subtle_equals(&expected, signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slack_event_maps_to_canonical_payload() {
        let raw = json!({
            "type": "event_callback",
            "team_id": "T123",
            "event_id": "Ev01ABC",
            "event_time": 1731315600,
            "event": {
                "type": "message",
                "user": "U456",
                "text": "Hi",
                "ts": "1731315600.000100",
                "channel": "C789",
                "thread_ts": "1731315600.000100"
            }
        });
        let envelope: SlackEventEnvelope = serde_json::from_value(raw.clone()).unwrap();
        let event = envelope.event.as_ref().unwrap();
        let mapped = map_slack_event("demo", &envelope, event, &raw).unwrap();
        assert_eq!(mapped.session_key, "demo:slack:1731315600.000100:U456");
        assert_eq!(mapped.provider_ids.workspace_id.as_deref(), Some("T123"));
        assert_eq!(
            mapped.provider_ids.thread_id.as_deref(),
            Some("1731315600.000100")
        );
        let canonical = mapped.payload;
        assert_eq!(canonical["provider"], json!("slack"));
        assert_eq!(
            canonical["session"]["key"],
            json!("demo:slack:1731315600.000100:U456")
        );
        assert_eq!(canonical["text"], json!("Hi"));
        assert_eq!(canonical["attachments"], json!([]));
        assert_eq!(canonical["buttons"], json!([]));
    }
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

struct MappedCanonical {
    provider_ids: ProviderIds,
    session_key: String,
    timestamp: DateTime<Utc>,
    payload: Value,
}

#[derive(Deserialize)]
struct SlackEventEnvelope {
    #[serde(rename = "type")]
    payload_type: String,
    #[allow(dead_code)]
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    challenge: Option<String>,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    event_time: Option<i64>,
    #[serde(default)]
    event: Option<SlackEvent>,
}

#[derive(Deserialize)]
struct SlackEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    thread_ts: Option<String>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    files: Option<Vec<SlackFile>>,
    #[serde(default)]
    blocks: Option<Vec<Value>>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Deserialize)]
struct SlackFile {
    #[allow(dead_code)]
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mimetype: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(rename = "url_private")]
    #[serde(default)]
    url_private: Option<String>,
}

#[derive(Deserialize)]
pub struct SlackInteractiveForm {
    pub payload: String,
}

#[derive(Deserialize, Serialize)]
struct SlackInteractivePayload {
    #[serde(rename = "type")]
    payload_type: String,
    #[serde(default)]
    team: Option<SlackTeam>,
    #[serde(default)]
    channel: Option<SlackChannel>,
    user: Option<SlackUser>,
    #[serde(default)]
    actions: Vec<SlackAction>,
    #[serde(default)]
    trigger_id: Option<String>,
    #[serde(default)]
    action_ts: Option<String>,
    #[serde(default)]
    message: Option<SlackMessageRef>,
}

#[derive(Deserialize, Serialize)]
struct SlackTeam {
    id: String,
}

#[derive(Deserialize, Serialize)]
struct SlackChannel {
    id: String,
}

#[derive(Deserialize, Serialize)]
struct SlackUser {
    id: String,
}

#[derive(Deserialize, Serialize)]
struct SlackMessageRef {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct SlackAction {
    action_id: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    selected_option: Option<SlackSelectedOption>,
    #[serde(default)]
    text: Option<Value>,
}

#[derive(Deserialize, Serialize)]
struct SlackSelectedOption {
    #[serde(default)]
    value: Option<String>,
}
