use crate::catalog::{CatalogInterpretation, interpret_catalog_response};
use crate::chat::execute_chat_completion;
use crate::client::OmniRouteClient;
use crate::profile::interpret_runtime_profile_response;
use async_trait::async_trait;
use vibecoder_domain::{ModelRef, Result, VibeCoderError};
use vibecoder_gateway_contract::{
    GatewayChatRequest, GatewayChatResponse, GatewayCredential, GatewayExecutionProfile, GatewayHealth, GatewayHealthStatus, ModelGateway,
};

#[async_trait]
impl ModelGateway for OmniRouteClient {
    async fn execution_profile(
        &self,
        credential: GatewayCredential<'_>,
    ) -> Result<GatewayExecutionProfile> {
        let raw = self.get_runtime_profile_raw(credential).await?;
        interpret_runtime_profile_response(raw)
    }

    async fn health(&self, credential: GatewayCredential<'_>) -> Result<GatewayHealth> {
        let raw = match self.get_models_raw(credential).await {
            Ok(raw) => raw,
            Err(VibeCoderError::Gateway(code)) => {
                return Ok(GatewayHealth {
                    ready: false,
                    status: transport_health_status(&code),
                    usable_models: 0,
                    detail: Some(code),
                });
            }
            Err(error) => return Err(error),
        };

        let interpreted = match interpret_catalog_response(raw, credential) {
            Ok(interpreted) => interpreted,
            Err(VibeCoderError::Gateway(code)) => {
                return Ok(GatewayHealth {
                    ready: false,
                    status: GatewayHealthStatus::InvalidResponse,
                    usable_models: 0,
                    detail: Some(code),
                });
            }
            Err(error) => return Err(error),
        };

        match interpreted {
            CatalogInterpretation::Models(models) => Ok(GatewayHealth {
                ready: true,
                status: GatewayHealthStatus::Ready,
                usable_models: models.len(),
                detail: None,
            }),
            CatalogInterpretation::NotReady { status, code } => Ok(GatewayHealth {
                ready: false,
                status,
                usable_models: 0,
                detail: Some(code.into()),
            }),
        }
    }

    async fn list_models(&self, credential: GatewayCredential<'_>) -> Result<Vec<ModelRef>> {
        let raw = self.get_models_raw(credential).await?;
        match interpret_catalog_response(raw, credential)? {
            CatalogInterpretation::Models(models) => Ok(models),
            CatalogInterpretation::NotReady { code, .. } => {
                Err(VibeCoderError::Gateway(code.into()))
            }
        }
    }

    async fn chat_completion(
        &self,
        credential: GatewayCredential<'_>,
        request: &GatewayChatRequest,
    ) -> Result<GatewayChatResponse> {
        execute_chat_completion(self, credential, request).await
    }
}

fn transport_health_status(code: &str) -> GatewayHealthStatus {
    match code {
        "response_too_large" => GatewayHealthStatus::InvalidResponse,
        _ => GatewayHealthStatus::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_response_is_invalid_not_network_unavailable() {
        assert_eq!(
            transport_health_status("response_too_large"),
            GatewayHealthStatus::InvalidResponse
        );
        assert_eq!(
            transport_health_status("http_timeout"),
            GatewayHealthStatus::Unavailable
        );
    }
}
