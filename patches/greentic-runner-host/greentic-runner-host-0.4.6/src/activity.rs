use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// High-level activity payload exchanged with Greentic hosts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    #[serde(default)]
    pub(crate) kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flow_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
    #[serde(default)]
    payload: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityKind {
    /// Messaging-style activity (default).
    #[default]
    Message,
    /// Custom activity with user-specified action + optional flow type override.
    Custom {
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flow_type: Option<String>,
    },
}

impl Activity {
    /// Create a text messaging activity payload.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Message,
            tenant: None,
            flow_id: None,
            flow_type: Some("messaging".into()),
            session_id: None,
            provider_id: None,
            user_id: None,
            channel_id: None,
            conversation_id: None,
            payload: json!({ "text": text.into() }),
        }
    }

    /// Build a custom activity with a raw payload body.
    pub fn custom(action: impl Into<String>, payload: Value) -> Self {
        Self {
            kind: ActivityKind::Custom {
                action: action.into(),
                flow_type: None,
            },
            tenant: None,
            flow_id: None,
            flow_type: None,
            session_id: None,
            provider_id: None,
            user_id: None,
            channel_id: None,
            conversation_id: None,
            payload,
        }
    }

    /// Attach a tenant identifier to the activity.
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Target a specific flow identifier.
    pub fn with_flow(mut self, flow_id: impl Into<String>) -> Self {
        self.flow_id = Some(flow_id.into());
        self
    }

    /// Hint which flow type should handle the activity.
    pub fn with_flow_type(mut self, flow_type: impl Into<String>) -> Self {
        let flow_type = flow_type.into();
        self.flow_type = Some(flow_type.clone());
        if let ActivityKind::Custom {
            flow_type: inner, ..
        } = &mut self.kind
        {
            *inner = Some(flow_type);
        }
        self
    }

    /// Attach a session identifier used for retries/idempotency.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Attach a provider identifier for telemetry scoping.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider_id = Some(provider.into());
        self
    }

    /// Attach the originating user for messaging activities.
    pub fn from_user(mut self, user: impl Into<String>) -> Self {
        self.user_id = Some(user.into());
        self
    }

    /// Attach a channel identifier (chat, room, or queue) for canonical session keys.
    pub fn in_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel_id = Some(channel.into());
        self
    }

    /// Attach a conversation/thread identifier for canonical session keys.
    pub fn in_conversation(mut self, conversation: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation.into());
        self
    }

    /// Return the resolved tenant identifier, if any.
    pub fn tenant(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    /// Return the resolved flow identifier hint.
    pub fn flow_id(&self) -> Option<&str> {
        self.flow_id.as_deref()
    }

    /// Return the resolved flow type hint.
    pub fn flow_type(&self) -> Option<&str> {
        self.flow_type
            .as_deref()
            .or_else(|| self.kind.flow_type_hint())
    }

    /// Return the originating session identifier, if supplied.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Return the originating provider identifier, if supplied.
    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }

    /// Return the originating user identifier, if supplied.
    pub fn user(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Return the channel identifier, if supplied.
    pub fn channel(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    /// Return the conversation identifier, if supplied.
    pub fn conversation(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// Underlying payload body.
    pub fn payload(&self) -> &Value {
        &self.payload
    }

    pub(crate) fn action(&self) -> Option<&str> {
        self.kind.action_hint()
    }

    pub(crate) fn into_payload(self) -> Value {
        self.payload
    }

    pub(crate) fn ensure_tenant(mut self, tenant: &str) -> Self {
        if self.tenant.is_none() {
            self.tenant = Some(tenant.to_string());
        }
        self
    }

    pub(crate) fn from_output(payload: Value, tenant: &str) -> Self {
        Activity::custom("response", payload).ensure_tenant(tenant)
    }
}

impl ActivityKind {
    fn flow_type_hint(&self) -> Option<&str> {
        match self {
            ActivityKind::Message => Some("messaging"),
            ActivityKind::Custom { flow_type, .. } => flow_type.as_deref(),
        }
    }

    fn action_hint(&self) -> Option<&str> {
        match self {
            ActivityKind::Message => Some("messaging"),
            ActivityKind::Custom { action, .. } => Some(action.as_str()),
        }
    }
}
