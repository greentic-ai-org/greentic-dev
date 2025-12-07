use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderIds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

impl ProviderIds {
    pub fn anchor(&self) -> Option<&str> {
        self.thread_id
            .as_deref()
            .or(self.conversation_id.as_deref())
            .or(self.channel_id.as_deref())
    }

    pub fn user(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalAttachment {
    pub attachment_type: String,
    pub name: Option<String>,
    pub mime: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
    pub data_inline_b64: Option<String>,
}

impl CanonicalAttachment {
    pub fn into_value(self) -> Value {
        json!({
            "type": self.attachment_type,
            "name": self.name,
            "mime": self.mime,
            "size": self.size.unwrap_or(0),
            "url": self.url,
            "data_inline_b64": self.data_inline_b64,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalButton {
    pub id: String,
    pub title: String,
    pub payload: String,
}

impl CanonicalButton {
    pub fn into_value(self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "payload": self.payload,
        })
    }
}

pub fn canonical_session_key(tenant: &str, provider: &str, ids: &ProviderIds) -> String {
    let user = ids.user().unwrap_or("user");
    let conversation = ids.anchor().or(ids.user()).unwrap_or("conversation");
    format!("{tenant}:{provider}:{conversation}:{user}")
}

#[allow(clippy::too_many_arguments)]
pub fn build_canonical_payload(
    tenant: &str,
    provider: &str,
    provider_ids: &ProviderIds,
    session_key: String,
    scopes: &[String],
    timestamp: DateTime<Utc>,
    locale: Option<String>,
    text: Option<String>,
    attachments: Vec<Value>,
    buttons: Vec<Value>,
    entities: Value,
    metadata: Value,
    mut channel_data: Value,
    raw: Value,
) -> Value {
    let mut payload = Map::new();
    payload.insert("tenant".into(), Value::String(tenant.to_string()));
    payload.insert("provider".into(), Value::String(provider.to_string()));
    payload.insert("provider_ids".into(), provider_ids.to_value());
    payload.insert(
        "session".into(),
        json!({
            "key": session_key,
            "scopes": scopes,
        }),
    );
    payload.insert("timestamp".into(), Value::String(timestamp.to_rfc3339()));
    payload.insert(
        "locale".into(),
        locale.map(Value::String).unwrap_or(Value::Null),
    );
    payload.insert(
        "text".into(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    payload.insert("attachments".into(), Value::Array(attachments));
    payload.insert("buttons".into(), Value::Array(buttons));
    payload.insert("entities".into(), entities);
    payload.insert("metadata".into(), metadata);

    if channel_data.is_null() {
        channel_data = Value::Object(Default::default());
    }
    payload.insert("channel_data".into(), channel_data);
    payload.insert("raw".into(), raw);
    Value::Object(payload)
}

pub fn empty_entities() -> Value {
    json!({
        "mentions": [],
        "urls": []
    })
}

pub fn default_metadata() -> Value {
    json!({
        "raw_headers": {},
        "ip": Value::Null
    })
}
