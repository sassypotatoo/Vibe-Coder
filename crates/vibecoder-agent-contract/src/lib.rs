//! Stable application-facing contract for coding-agent runtimes.
//!
//! The first adapter targets Jcode's versioned harness API/SDK. Nothing in this contract exposes
//! Jcode protocol types, so the rest of VibeCoder is not coupled to a specific agent runtime.

use async_trait::async_trait;
use vibecoder_domain::{
    AgentEvent, ModelRef, PermissionDecision, ProjectRef, Result, RuntimeCapabilities, SessionId,
    TurnResult,
};

pub type EventHandler = Box<dyn FnMut(AgentEvent) + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelGatewayBridgeIdentity {
    pub gateway_id: String,
    pub transport_provider: String,
    pub exact_model_id_passthrough: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CreateSessionOptions {
    /// Optional runtime-specific creation-time model request. Adapters must reject this when the
    /// upstream runtime cannot make session creation + model selection atomic.
    pub model: Option<ModelRef>,
}

#[derive(Debug, Clone, Default)]
pub struct RunTurnOptions {
    /// Ensure this model is selected before the turn. For persistent-session runtimes this may
    /// change the session's active model beyond this one turn; adapters must document semantics.
    pub model: Option<ModelRef>,
}

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Stable persistence identity for this adapter. It is metadata only, never an executable name.
    fn runtime_id(&self) -> &'static str;

    /// Passive snapshot of capabilities already negotiated with the runtime.
    fn capabilities(&self) -> RuntimeCapabilities;

    /// Optional attestation that this agent runtime sends model traffic through a specific
    /// VibeCoder-controlled gateway while preserving exact model ids. The default is unbridged.
    fn model_gateway_bridge_identity(&self) -> Option<ModelGatewayBridgeIdentity> {
        None
    }

    /// Actively prepare/connect the runtime and return capabilities from the verified runtime
    /// handshake. Preflight uses this instead of assuming a disconnected adapter has no features.
    async fn ensure_ready(&self) -> Result<RuntimeCapabilities>;

    async fn create_session(
        &self,
        project: &ProjectRef,
        options: CreateSessionOptions,
    ) -> Result<SessionId>;

    /// Re-attach an existing persisted agent session to the supplied project.
    ///
    /// Implementations must verify that the runtime-reported working directory still belongs to
    /// the expected project before considering the session resumed. A session id alone is not a
    /// sufficient project-authorization boundary.
    async fn resume_session(&self, project: &ProjectRef, session_id: &SessionId) -> Result<()>;

    /// Verify that this already-known session is still bound to the supplied project in the
    /// runtime's current connection/generation. This is an authorization check, not a resume.
    async fn verify_session_project_binding(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()>;

    /// Reject workspace snapshot/rollback while this agent can still be mutating project files.
    /// This is a live quiescence check, not durable authorization.
    async fn ensure_workspace_quiescent(&self, project: &ProjectRef) -> Result<()>;

    /// Rebuild runtime-specific project/session attachment after the project directory has been
    /// atomically replaced by rollback. The same path string must not be treated as proof that a
    /// long-lived runtime still targets the new directory identity.
    async fn refresh_session_after_workspace_replacement(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()>;

    async fn run_turn(
        &self,
        session_id: &SessionId,
        prompt: &str,
        options: RunTurnOptions,
        on_event: Option<EventHandler>,
    ) -> Result<TurnResult>;

    async fn cancel(&self, session_id: &SessionId) -> Result<()>;

    async fn respond_to_permission(
        &self,
        session_id: &SessionId,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<()>;

    /// Discover the switchable model catalog for one verified agent session.
    ///
    /// Model catalogs are session-scoped for runtimes such as Jcode, so callers must never reuse
    /// a catalog discovered for a different session.
    async fn list_models(&self, session_id: &SessionId) -> Result<Vec<ModelRef>>;

    /// Select and independently corroborate one exact model identity through fresh runtime state.
    /// Implementations must return the observed active model/provider, not echo the request.
    async fn corroborate_model_identity(
        &self,
        session_id: &SessionId,
        model: &ModelRef,
    ) -> Result<ModelRef>;

    async fn set_model(&self, session_id: &SessionId, model: &ModelRef) -> Result<()>;
}
