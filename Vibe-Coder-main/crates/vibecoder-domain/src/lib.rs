//! Provider-neutral types shared by every VibeCoder backend component.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, VibeCoderError>;

#[derive(Debug, Error)]
pub enum VibeCoderError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("agent runtime error: {0}")]
    Agent(String),
    #[error("model gateway error: {0}")]
    Gateway(String),
    #[error("model routing error: {0}")]
    Routing(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("secret resolution error: {0}")]
    Secret(String),
    #[error("workspace error: {0}")]
    Workspace(String),
    #[error("command policy error: {0}")]
    Command(String),
    #[error("process runtime error: {0}")]
    Process(String),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("checkpoint error: {0}")]
    Checkpoint(String),
    #[error("build job error: {0}")]
    Build(String),
    #[error("capability '{capability}' is not available from {component}")]
    MissingCapability {
        component: &'static str,
        capability: &'static str,
    },
    #[error("operation cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationId(pub Uuid);

impl ConversationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConversationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(VibeCoderError::InvalidRequest(
                "session id cannot be empty".into(),
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: ProjectId,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub id: String,
    pub display_name: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionDecision {
    AllowOnce,
    AllowSession,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub session_id: SessionId,
    pub action: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentEvent {
    /// User-visible assistant text only. Provider/private reasoning is deliberately not exposed.
    TextDelta {
        text: String,
    },
    MessageAccepted,
    ToolStarted {
        tool: String,
        call_id: String,
    },
    ToolFinished {
        tool: String,
        call_id: String,
        ok: bool,
        output: String,
        error: Option<String>,
    },
    BackgroundProgress {
        task_id: String,
        label: String,
        percent: Option<f32>,
        summary: String,
        done: bool,
    },
    SessionStatus {
        status: String,
    },
    TokenUsage(TokenUsage),
    PermissionRequired(PermissionRequest),
    TurnCompleted,
    Warning {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub call_id: String,
    pub tool: String,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read_input: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnResult {
    pub text: String,
    pub cancelled: bool,
    pub tool_calls: Vec<ToolCallResult>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub sessions: bool,
    pub streaming_events: bool,
    pub permissions: bool,
    pub model_selection: bool,
    pub file_tools: bool,
    pub command_tools: bool,
}

impl RuntimeCapabilities {
    pub const fn none() -> Self {
        Self {
            sessions: false,
            streaming_events: false,
            permissions: false,
            model_selection: false,
            file_tools: false,
            command_tools: false,
        }
    }
}
