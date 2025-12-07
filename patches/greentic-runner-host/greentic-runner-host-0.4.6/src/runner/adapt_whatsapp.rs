use axum::body::Body;
use axum::extract::Query;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
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

pub async fn verify(Query(query): Query<VerifyQuery>) -> impl IntoResponse {
    let expected = std::env::var("WHATSAPP_VERIFY_TOKEN").ok();
    match (&query.mode, &query.challenge, &query.verify_token, expected) {
        (Some(mode), Some(challenge), Some(token), Some(expected))
            if mode == "subscribe" && token == &expected =>
        {
            (StatusCode::OK, challenge.clone())
        }
        _ => (StatusCode::FORBIDDEN, String::new()),
    }
}

pub async fn webhook(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    request: Request<Body>,
) -> Result<StatusCode, StatusCode> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let bytes = collect_body(body).await?;
    verify_signature(&headers, &bytes)?;

    let raw_value: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let webhook: WhatsappWebhook =
        serde_json::from_value(raw_value.clone()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let message = webhook
        .entry
        .iter()
        .flat_map(|entry| &entry.changes)
        .find_map(|change| {
            change
                .value
                .messages
                .as_ref()
                .and_then(|msgs| msgs.first().cloned())
        });

    if message.is_none() {
        return Ok(StatusCode::OK);
    }
    let message = message.unwrap();

    if mark_processed(runtime.webhook_cache(), &message.id) {
        return Ok(StatusCode::ACCEPTED);
    }

    let flow = runtime
        .engine()
        .flow_by_type("messaging")
        .ok_or(StatusCode::NOT_FOUND)?;

    let provider_ids = ProviderIds {
        conversation_id: Some(message.from.clone()),
        user_id: Some(message.from.clone()),
        message_id: Some(message.id.clone()),
        ..ProviderIds::default()
    };
    let session_key = canonical_session_key(&tenant, "whatsapp", &provider_ids);
    let timestamp = parse_timestamp(message.timestamp.as_deref())?;

    let (text, attachments, buttons, scopes) = map_message_content(&message);

    let canonical_payload = build_canonical_payload(
        &tenant,
        "whatsapp",
        &provider_ids,
        session_key.clone(),
        &scopes,
        timestamp,
        None,
        text,
        attachments,
        buttons,
        empty_entities(),
        default_metadata(),
        json!({ "type": message.r#type }),
        raw_value,
    );

    let envelope = IngressEnvelope {
        tenant,
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(session_key),
        provider: Some("whatsapp".into()),
        channel: Some(message.from.clone()),
        conversation: Some(message.from.clone()),
        user: Some(message.from.clone()),
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
            tracing::error!(error = %err, "whatsapp flow execution failed");
            StatusCode::BAD_GATEWAY
        })?;
    Ok(StatusCode::ACCEPTED)
}

fn map_message_content(
    message: &WhatsappMessage,
) -> (Option<String>, Vec<Value>, Vec<Value>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut buttons = Vec::new();
    let mut scopes = vec!["chat".to_string()];
    let mut text = message.text.as_ref().map(|text| text.body.clone());

    match message.r#type.as_str() {
        "image" => {
            if let Some(image) = &message.image {
                attachments.push(
                    CanonicalAttachment {
                        attachment_type: "image".into(),
                        name: image.caption.clone(),
                        mime: None,
                        size: None,
                        url: image.link.clone(),
                        data_inline_b64: None,
                    }
                    .into_value(),
                );
            }
        }
        "audio" => {
            if let Some(audio) = &message.audio {
                attachments.push(
                    CanonicalAttachment {
                        attachment_type: "audio".into(),
                        name: None,
                        mime: None,
                        size: None,
                        url: audio.link.clone(),
                        data_inline_b64: None,
                    }
                    .into_value(),
                );
            }
        }
        "video" => {
            if let Some(video) = &message.video {
                attachments.push(
                    CanonicalAttachment {
                        attachment_type: "video".into(),
                        name: video.caption.clone(),
                        mime: None,
                        size: None,
                        url: video.link.clone(),
                        data_inline_b64: None,
                    }
                    .into_value(),
                );
            }
        }
        "document" => {
            if let Some(doc) = &message.document {
                attachments.push(
                    CanonicalAttachment {
                        attachment_type: "file".into(),
                        name: doc.filename.clone(),
                        mime: None,
                        size: None,
                        url: doc.link.clone(),
                        data_inline_b64: None,
                    }
                    .into_value(),
                );
            }
        }
        "location" => {
            if let Some(location) = &message.location {
                text = Some(location.name.clone().unwrap_or_else(|| "location".into()));
            }
        }
        "interactive" => {
            if let Some(interactive) = &message.interactive {
                if let Some(reply) = &interactive.button_reply {
                    buttons.push(
                        CanonicalButton {
                            id: reply.id.clone(),
                            title: reply.title.clone(),
                            payload: reply.id.clone(),
                        }
                        .into_value(),
                    );
                    text = Some(reply.title.clone());
                } else if let Some(list) = &interactive.list_reply {
                    buttons.push(
                        CanonicalButton {
                            id: list.id.clone(),
                            title: list.title.clone(),
                            payload: list.id.clone(),
                        }
                        .into_value(),
                    );
                    text = Some(list.title.clone());
                }
            }
        }
        _ => {}
    }

    if !attachments.is_empty() {
        scopes.push("attachments".into());
    }
    if !buttons.is_empty() {
        scopes.push("buttons".into());
    }

    (text, attachments, buttons, scopes)
}

fn parse_timestamp(raw: Option<&str>) -> Result<DateTime<Utc>, StatusCode> {
    if let Some(epoch) = raw.and_then(|value| value.parse::<i64>().ok()) {
        return DateTime::from_timestamp(epoch, 0).ok_or(StatusCode::BAD_REQUEST);
    }
    Ok(Utc::now())
}

fn verify_signature(headers: &HeaderMap, body: &[u8]) -> Result<(), StatusCode> {
    if let Ok(secret) = std::env::var("WHATSAPP_APP_SECRET") {
        let signature = headers
            .get("X-Hub-Signature-256")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("sha256="))
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| StatusCode::UNAUTHORIZED)?;
        mac.update(body);
        let expected = hex::encode(mac.finalize().into_bytes());
        if !subtle_equals(signature, &expected) {
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

#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
    #[serde(rename = "hub.verify_token")]
    verify_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhatsappWebhook {
    entry: Vec<WhatsappEntry>,
}

#[derive(Debug, Deserialize)]
struct WhatsappEntry {
    changes: Vec<WhatsappChange>,
}

#[derive(Debug, Deserialize)]
struct WhatsappChange {
    value: WhatsappValue,
}

#[derive(Debug, Deserialize)]
struct WhatsappValue {
    #[serde(default)]
    messages: Option<Vec<WhatsappMessage>>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappMessage {
    id: String,
    #[serde(default)]
    from: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    text: Option<WhatsappText>,
    #[serde(default)]
    image: Option<WhatsappMedia>,
    #[serde(default)]
    audio: Option<WhatsappMedia>,
    #[serde(default)]
    video: Option<WhatsappMedia>,
    #[serde(default)]
    document: Option<WhatsappDocument>,
    #[serde(default)]
    location: Option<WhatsappLocation>,
    #[serde(default)]
    interactive: Option<WhatsappInteractive>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappText {
    body: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappMedia {
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    caption: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappDocument {
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappLocation {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappInteractive {
    #[serde(default)]
    button_reply: Option<WhatsappButtonReply>,
    #[serde(default)]
    list_reply: Option<WhatsappListReply>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappButtonReply {
    id: String,
    title: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsappListReply {
    id: String,
    title: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whatsapp_message_maps_to_canonical_payload() {
        let raw = json!({
            "entry": [{
                "changes": [{
                    "value": {
                        "messages": [{
                            "id": "wamid.HBgM",
                            "from": "447700900123",
                            "timestamp": "1731315600",
                            "type": "text",
                            "text": { "body": "Hi" }
                        }]
                    }
                }]
            }]
        });
        let webhook: WhatsappWebhook = serde_json::from_value(raw.clone()).unwrap();
        let message = webhook.entry[0].changes[0].value.messages.as_ref().unwrap()[0].clone();

        let provider_ids = ProviderIds {
            conversation_id: Some(message.from.clone()),
            user_id: Some(message.from.clone()),
            message_id: Some(message.id.clone()),
            ..ProviderIds::default()
        };
        let session_key = canonical_session_key("zain-kuwait", "whatsapp", &provider_ids);
        assert_eq!(
            session_key,
            "zain-kuwait:whatsapp:447700900123:447700900123"
        );
        let timestamp = parse_timestamp(message.timestamp.as_deref()).unwrap();
        let (text, attachments, buttons, scopes) = map_message_content(&message);
        let canonical = build_canonical_payload(
            "zain-kuwait",
            "whatsapp",
            &provider_ids,
            session_key,
            &scopes,
            timestamp,
            None,
            text,
            attachments,
            buttons,
            empty_entities(),
            default_metadata(),
            json!({ "type": message.r#type }),
            raw,
        );

        assert_eq!(canonical["provider"], json!("whatsapp"));
        assert_eq!(
            canonical["session"]["key"],
            json!("zain-kuwait:whatsapp:447700900123:447700900123")
        );
        assert_eq!(canonical["text"], json!("Hi"));
        assert_eq!(canonical["attachments"], json!([]));
        assert_eq!(canonical["buttons"], json!([]));
    }
}
