//! Provider-neutral persisted project/session state.
//!
//! Persisted state is deliberately narrow. It contains stable identifiers and user choices, not
//! filesystem authority, secrets, tool output, process output, resolved model catalogs, command
//! approvals, or connection-generation state. Every loaded identifier must be re-corroborated by
//! the owning runtime before it becomes live authority again.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use vibecoder_domain::{ProjectId, Result, SessionId, VibeCoderError};
use vibecoder_routing::{ModelRoutePolicyConfig, ModelRouteTargetConfig};

pub const PROJECT_STATE_SCHEMA_V1: u32 = 1;
pub const MAX_PERSISTED_PROJECTS: usize = 4096;
pub const MAX_PERSISTED_STATE_BYTES: usize = 256 * 1024;
pub const MAX_RUNTIME_ID_BYTES: usize = 64;
pub const MAX_SESSION_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistenceCapabilities {
    pub project_registry: bool,
    pub session_binding: bool,
    pub model_preference: bool,
    pub route_policy: bool,
    pub atomic_replace: bool,
    pub secrets_persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedAgentSession {
    /// Stable adapter id such as `jcode-harness`. This is identity metadata, not executable input.
    pub runtime_id: String,
    pub session_id: SessionId,
    #[serde(default)]
    pub preferred_model: Option<ModelRouteTargetConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedProjectState {
    pub schema: u32,
    /// Monotonic compare-and-swap revision. Revision zero is valid only for first creation.
    pub revision: u64,
    pub project_id: ProjectId,
    /// Crash marker written before creating a runtime session. A true value blocks automatic
    /// resume until recovery/reconciliation confirms what happened upstream.
    #[serde(default)]
    pub session_creation_pending: bool,
    #[serde(default)]
    pub agent_session: Option<PersistedAgentSession>,
    #[serde(default)]
    pub route_policy: Option<ModelRoutePolicyConfig>,
}

impl PersistedProjectState {
    pub const fn new(project_id: ProjectId) -> Self {
        Self {
            schema: PROJECT_STATE_SCHEMA_V1,
            revision: 0,
            project_id,
            session_creation_pending: false,
            agent_session: None,
            route_policy: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != PROJECT_STATE_SCHEMA_V1 {
            return Err(persistence_error("project_state_schema_unsupported"));
        }
        if self.session_creation_pending && self.agent_session.is_some() {
            return Err(persistence_error("project_state_session_pending_conflict"));
        }
        if let Some(session) = &self.agent_session {
            validate_runtime_id(&session.runtime_id)?;
            validate_session_id(&session.session_id)?;
            if let Some(model) = &session.preferred_model {
                model.validate()?;
            }
        }
        if let Some(policy) = &self.route_policy {
            policy.validate()?;
        }
        Ok(())
    }
}

#[async_trait]
pub trait ProjectStateStore: Send + Sync {
    fn capabilities(&self) -> PersistenceCapabilities;

    /// Create revision-zero state. Must fail if state already exists for this project.
    async fn create_project_state(&self, state: &PersistedProjectState) -> Result<()>;

    /// Compare-and-swap one existing state revision. `state.revision` must equal
    /// `expected_revision`; the store persists and returns revision+1.
    async fn update_project_state(
        &self,
        expected_revision: u64,
        state: &PersistedProjectState,
    ) -> Result<PersistedProjectState>;

    async fn load_project_state(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<PersistedProjectState>>;

    async fn list_project_ids(&self, max_projects: usize) -> Result<Vec<ProjectId>>;

    async fn remove_project_state(&self, project_id: ProjectId) -> Result<()>;
}

fn validate_runtime_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_RUNTIME_ID_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(persistence_error("project_state_runtime_id_invalid"));
    }
    Ok(())
}

fn validate_session_id(value: &SessionId) -> Result<()> {
    if value.0.is_empty()
        || value.0.len() > MAX_SESSION_ID_BYTES
        || !value
            .0
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(persistence_error("project_state_session_id_invalid"));
    }
    Ok(())
}

fn persistence_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Persistence(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn state_contains_no_root_or_secret_fields() {
        let state = PersistedProjectState::new(ProjectId(Uuid::nil()));
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("root"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("api_key"));
    }

    #[test]
    fn rejects_bad_runtime_identity() {
        let mut state = PersistedProjectState::new(ProjectId(Uuid::nil()));
        state.agent_session = Some(PersistedAgentSession {
            runtime_id: "../../jcode".into(),
            session_id: SessionId("safe-session".into()),
            preferred_model: None,
        });
        assert!(state.validate().is_err());
    }
}
