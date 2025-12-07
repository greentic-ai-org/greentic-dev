use anyhow::{Result, bail};
use axum::extract::Json;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::runtime::IngressEnvelope;
use crate::ingress::{
    ProviderIds, build_canonical_payload, canonical_session_key, default_metadata, empty_entities,
};
use crate::routing::TenantRuntimeHandle;
use crate::runtime::TenantRuntime;

#[derive(Debug, Serialize, Deserialize)]
pub struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TelegramMessage {
    #[serde(default)]
    text: Option<String>,
    chat: TelegramChat,
    #[serde(default)]
    from: Option<TelegramUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TelegramChat {
    id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TelegramUser {
    id: i64,
}

pub async fn telegram_webhook(
    TenantRuntimeHandle { tenant, runtime }: TenantRuntimeHandle,
    Json(update): Json<TelegramUpdate>,
) -> StatusCode {
    if let Some(status) = {
        let mut cache = runtime.telegram_cache().lock();
        cache.get(&update.update_id).copied()
    } {
        tracing::debug!(
            update_id = update.update_id,
            status = %status,
            "duplicate telegram update skipped"
        );
        return status;
    }

    let message = match update.message.as_ref() {
        Some(msg) => msg,
        None => {
            tracing::debug!(update_id = update.update_id, "no message payload in update");
            return remember_status(runtime.as_ref(), update.update_id, StatusCode::NO_CONTENT);
        }
    };

    let text = match message.text.as_ref() {
        Some(text) if !text.trim().is_empty() => text.clone(),
        _ => {
            tracing::debug!(update_id = update.update_id, "ignoring non-text message");
            return remember_status(runtime.as_ref(), update.update_id, StatusCode::NO_CONTENT);
        }
    };

    let engine = runtime.engine();
    let flow = match engine.flow_by_type("messaging") {
        Some(flow) => flow,
        None => {
            tracing::error!("no messaging flow registered in pack");
            return StatusCode::NOT_FOUND;
        }
    };

    let raw_value = serde_json::to_value(&update).unwrap_or(Value::Null);
    let mapped = map_telegram_activity(&tenant, message, &text, update.update_id, raw_value);

    let envelope = IngressEnvelope {
        tenant: tenant.clone(),
        env: None,
        flow_id: flow.id.clone(),
        flow_type: Some(flow.flow_type.clone()),
        action: Some("messaging".into()),
        session_hint: Some(mapped.session_key.clone()),
        provider: Some("telegram".into()),
        channel: mapped.channel.clone(),
        conversation: mapped.conversation.clone(),
        user: mapped.user.clone(),
        activity_id: mapped.provider_ids.message_id.clone(),
        timestamp: Some(mapped.timestamp.to_rfc3339()),
        payload: mapped.payload,
        metadata: None,
    }
    .canonicalize();

    match runtime.state_machine().handle(envelope).await {
        Ok(response) => {
            let replies = collect_text_responses(&response);
            if replies.is_empty() {
                tracing::info!(
                    flow_id = %flow.id,
                    update_id = update.update_id,
                    "flow completed without telegram replies"
                );
                return remember_status(runtime.as_ref(), update.update_id, StatusCode::NO_CONTENT);
            }

            for text in &replies {
                if let Err(err) =
                    send_telegram_message(runtime.as_ref(), message.chat.id, text).await
                {
                    tracing::error!(
                        flow_id = %flow.id,
                        update_id = update.update_id,
                        error = %err,
                        "failed to send telegram message"
                    );
                    return remember_status(
                        runtime.as_ref(),
                        update.update_id,
                        StatusCode::BAD_GATEWAY,
                    );
                }
            }

            tracing::info!(
                flow_id = %flow.id,
                update_id = update.update_id,
                replies = replies.len(),
                "flow completed"
            );
            remember_status(runtime.as_ref(), update.update_id, StatusCode::OK)
        }
        Err(err) => {
            let chained = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
            tracing::error!(
                flow_id = %flow.id,
                update_id = update.update_id,
                error.cause_chain = ?chained,
                "flow execution failed"
            );
            remember_status(
                runtime.as_ref(),
                update.update_id,
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        }
    }
}

async fn send_telegram_message(runtime: &TenantRuntime, chat_id: i64, text: &str) -> Result<()> {
    if !runtime.messaging_rate().lock().try_acquire() {
        bail!("messaging send rate exceeded");
    }

    let token = runtime.get_secret("TELEGRAM_BOT_TOKEN")?;
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "MarkdownV2",
    });
    runtime
        .http_client()
        .post(url)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn remember_status(runtime: &TenantRuntime, update_id: i64, status: StatusCode) -> StatusCode {
    let mut cache = runtime.telegram_cache().lock();
    cache.put(update_id, status);
    status
}

fn collect_text_responses(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Null => Vec::new(),
        serde_json::Value::String(text) => vec![text.to_owned()],
        serde_json::Value::Array(items) => {
            let mut replies = Vec::new();
            for item in items {
                replies.extend(collect_text_responses(item));
            }
            replies
        }
        serde_json::Value::Object(map) => {
            if let Some(messages) = map.get("messages").and_then(|v| v.as_array()) {
                let mut replies = Vec::new();
                for entry in messages {
                    replies.extend(collect_text_responses(entry));
                }
                return replies;
            }
            map.get("text")
                .and_then(|v| v.as_str())
                .map(|text| vec![text.to_owned()])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

struct MappedTelegram {
    provider_ids: ProviderIds,
    session_key: String,
    timestamp: DateTime<Utc>,
    payload: Value,
    channel: Option<String>,
    conversation: Option<String>,
    user: Option<String>,
}

fn map_telegram_activity(
    tenant: &str,
    message: &TelegramMessage,
    text: &str,
    update_id: i64,
    raw: Value,
) -> MappedTelegram {
    let chat_id = message.chat.id.to_string();
    let user = message.from.as_ref().map(|user| user.id.to_string());
    let provider_ids = ProviderIds {
        channel_id: Some(chat_id.clone()),
        conversation_id: Some(chat_id.clone()),
        user_id: user.clone(),
        message_id: Some(update_id.to_string()),
        event_id: Some(update_id.to_string()),
        ..ProviderIds::default()
    };
    let timestamp = Utc::now();
    let session_key = canonical_session_key(tenant, "telegram", &provider_ids);
    let payload = build_canonical_payload(
        tenant,
        "telegram",
        &provider_ids,
        session_key.clone(),
        &["chat".into()],
        timestamp,
        None,
        Some(text.to_string()),
        Vec::new(),
        Vec::new(),
        empty_entities(),
        default_metadata(),
        json!({ "chat_id": message.chat.id }),
        raw,
    );

    MappedTelegram {
        provider_ids,
        session_key,
        timestamp,
        payload,
        channel: Some(chat_id.clone()),
        conversation: Some(chat_id),
        user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collect_text_from_array_and_objects() {
        let payload = json!([
            { "text": "hello" },
            { "messages": [{ "text": "nested" }, "raw"] },
            null,
            "world"
        ]);
        let replies = collect_text_responses(&payload);
        assert_eq!(replies, vec!["hello", "nested", "raw", "world"]);
    }

    #[test]
    fn collect_text_from_single_object() {
        let payload = json!({ "text": "only" });
        let replies = collect_text_responses(&payload);
        assert_eq!(replies, vec!["only"]);
    }

    #[test]
    fn telegram_activity_maps_to_canonical_payload() {
        let update = TelegramUpdate {
            update_id: 42,
            message: Some(TelegramMessage {
                text: Some("Hello".into()),
                chat: TelegramChat { id: 123 },
                from: Some(TelegramUser { id: 777 }),
            }),
        };
        let message = update.message.clone().unwrap();
        let raw = serde_json::to_value(&update).unwrap();
        let mapped = map_telegram_activity("demo", &message, "Hello", update.update_id, raw);
        assert_eq!(mapped.session_key, "demo:telegram:123:777");
        assert_eq!(mapped.provider_ids.conversation_id.as_deref(), Some("123"));
        assert_eq!(mapped.provider_ids.user_id.as_deref(), Some("777"));
        assert_eq!(mapped.payload["provider"], json!("telegram"));
        assert_eq!(mapped.payload["text"], json!("Hello"));
    }
}
