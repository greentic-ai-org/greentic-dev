use thiserror::Error;

/// Unified error across the new runner stack.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("flow '{flow_id}' not registered")]
    FlowNotFound { flow_id: String },

    #[error("adapter '{adapter}' not registered")]
    AdapterMissing { adapter: String },

    #[error("adapter call failed: {reason}")]
    AdapterCall { reason: String },

    #[error("session error: {reason}")]
    Session { reason: String },

    #[error("state error: {reason}")]
    State { reason: String },

    #[error("policy violation: {reason}")]
    Policy { reason: String },

    #[error("telemetry error: {reason}")]
    Telemetry { reason: String },

    #[error("secret error: {reason}")]
    Secrets { reason: String },

    #[error("serialization error: {reason}")]
    Serialization { reason: String },
}

/// Result alias for runner operations.
pub type GResult<T> = Result<T, RunnerError>;
