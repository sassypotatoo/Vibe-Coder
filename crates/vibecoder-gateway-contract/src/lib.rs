//! Contract for model-routing gateways such as OmniRoute.
//!
//! Jcode may ultimately speak to the gateway directly for inference. This separate contract
//! still belongs in the application because VibeCoder needs independent health/model discovery
//! and configuration validation without reaching through Jcode internals.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use vibecoder_domain::{ModelRef, Result};

pub const VIBECODER_OMNIROUTE_GATEWAY_ID: &str = "omniroute";
pub const VIBECODER_OMNIROUTE_UPSTREAM_VERSION: &str = "3.8.50";
pub const VIBECODER_OMNIROUTE_PROFILE_ID: &str = "vibecoder-omniroute-exact-model-v1";
pub const VIBECODER_OMNIROUTE_PROFILE_SHA256: &str =
    "aec0f63fb0dec08f24fffde9209504ec447e9428bec1cd64c033649ed275fe3d";

/// Ephemeral gateway credential supplied by a caller that resolved the secret.
///
/// The credential is deliberately borrowed, non-serializable, and redacted from `Debug`. Part 10
/// will own resolving secret references into this short-lived value; gateway adapters must never
/// persist it.
#[derive(Clone, Copy)]
pub enum GatewayCredential<'a> {
    Anonymous,
    Secret(&'a str),
}

impl fmt::Debug for GatewayCredential<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("Anonymous"),
            Self::Secret(_) => formatter.write_str("Secret([REDACTED])"),
        }
    }
}

impl GatewayCredential<'_> {
    pub const fn is_anonymous(self) -> bool {
        matches!(self, Self::Anonymous)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayHealthStatus {
    Ready,
    AuthenticationRequired,
    AuthenticationRejected,
    AccessDenied,
    NoUsableModels,
    RateLimited,
    EndpointNotFound,
    InvalidResponse,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub ready: bool,
    pub status: GatewayHealthStatus,
    pub usable_models: usize,
    /// Stable diagnostic code only. Concrete adapters must not place raw transport/server prose
    /// or credentials here.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub base_url: String,
}

impl GatewayConfig {
    /// Provider-neutral shape check only. Concrete gateway adapters MUST apply their own strict
    /// transport/security validation before constructing a network client.
    pub fn validate(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            return Err(vibecoder_domain::VibeCoderError::InvalidRequest(
                "gateway base_url cannot be empty".into(),
            ));
        }
        if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err(vibecoder_domain::VibeCoderError::InvalidRequest(
                "gateway base_url must use http:// or https://".into(),
            ));
        }
        Ok(())
    }
}

/// Runtime-reported execution semantics fetched independently before inference.
/// Adapters construct this only after validating their gateway-specific attestation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayExecutionProfile {
    pub gateway_id: String,
    pub upstream_version: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub exact_model_only: bool,
    pub hidden_model_reroutes_disabled: bool,
}

impl GatewayExecutionProfile {
    pub const fn permits_exact_model_execution(&self) -> bool {
        self.exact_model_only && self.hidden_model_reroutes_disabled
    }
}

#[async_trait]
pub trait ModelGateway: Send + Sync {
    /// Fetch and validate the running gateway's execution-profile attestation. Static config or a
    /// model catalog alone is not proof of deterministic inference behavior.
    async fn execution_profile(
        &self,
        credential: GatewayCredential<'_>,
    ) -> Result<GatewayExecutionProfile>;

    /// Health is credential-scoped. The secret is borrowed for this call and must not be retained.
    async fn health(&self, credential: GatewayCredential<'_>) -> Result<GatewayHealth>;

    /// Return models usable for conversational/coding inference only. Specialty-only media,
    /// embedding, rerank, moderation, and similar entries must not leak into this catalog.
    async fn list_models(&self, credential: GatewayCredential<'_>) -> Result<Vec<ModelRef>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_credential_debug_redacts_secret() {
        let debug = format!("{:?}", GatewayCredential::Secret("top-secret"));
        assert!(!debug.contains("top-secret"));
        assert!(debug.contains("REDACTED"));
    }
}
