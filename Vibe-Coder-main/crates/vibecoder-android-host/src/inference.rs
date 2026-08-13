use super::{AndroidHostRuntime, OmniRouteServiceHandle, host_error};
use crate::omniroute_service::LOOPBACK_BASE_URL;
use serde::Serialize;
use vibecoder_domain::{Result, TokenUsage, VibeCoderError};
use vibecoder_gateway_contract::{
    GatewayChatMessage, GatewayChatRequest, GatewayChatRole, GatewayCredential, ModelGateway,
    VIBECODER_OMNIROUTE_GATEWAY_ID, VIBECODER_OMNIROUTE_PROFILE_ID,
    VIBECODER_OMNIROUTE_PROFILE_SHA256, VIBECODER_OMNIROUTE_UPSTREAM_VERSION,
};
use vibecoder_gateway_omniroute::{OmniRouteClient, OmniRouteConfig};

const INFERENCE_REQUEST_TIMEOUT_MS: u64 = 60_000;
const INFERENCE_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const DIAGNOSTIC_MAX_OUTPUT_TOKENS: u32 = 256;

/// Sanitized proof for the Part-34.5 first exact-model inference request.
///
/// Prompt and assistant text intentionally never enter this serializable report. Part 34.6 owns
/// user-visible conversation integration; this diagnostic only proves that one bounded inference
/// request reached the attested local gateway and returned a text completion.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OmniRouteInferenceProbe {
    pub schema: u32,
    pub component_id: &'static str,
    pub status: &'static str,
    pub base_url: &'static str,
    pub credential_mode: &'static str,
    pub credential_persisted: bool,
    pub prompt_persisted: bool,
    pub response_text_persisted: bool,
    pub service_attested: bool,
    pub service_active_before: bool,
    pub service_active_after: bool,
    pub runtime_profile_verified: bool,
    pub catalog_model_verified: bool,
    pub requested_model_id: String,
    pub observed_model_id: Option<String>,
    pub observed_model_matches_request: Option<bool>,
    pub inference_request_sent: bool,
    pub inference_requests_count: u8,
    pub automatic_retry_or_model_fallback: bool,
    pub response_received: bool,
    pub response_nonempty: bool,
    pub response_utf8_bytes: usize,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    pub first_model_request_proven: bool,
    pub detail: Option<String>,
}

impl AndroidHostRuntime {
    pub(crate) fn probe_omniroute_first_inference(
        &self,
        service: &OmniRouteServiceHandle,
        credential: GatewayCredential<'_>,
        requested_model_id: &str,
        prompt: &str,
    ) -> Result<OmniRouteInferenceProbe> {
        validate_model_id(requested_model_id)?;
        validate_prompt(prompt)?;
        let credential_mode = if credential.is_anonymous() {
            "anonymous"
        } else {
            "ephemeral_bearer"
        };
        let active_before = self.omniroute_service_active()?;
        let readiness = service.readiness();
        let service_attested = active_before
            && readiness.base_url == LOOPBACK_BASE_URL
            && readiness.upstream_version == VIBECODER_OMNIROUTE_UPSTREAM_VERSION
            && readiness.routing_profile_id == VIBECODER_OMNIROUTE_PROFILE_ID
            && readiness.routing_profile_sha256 == VIBECODER_OMNIROUTE_PROFILE_SHA256
            && readiness.exact_model_only
            && readiness.hidden_model_reroutes_disabled;
        if !service_attested {
            return Ok(base_probe(
                "service_not_attested",
                credential_mode,
                requested_model_id,
                active_before,
                self.omniroute_service_active()?,
                false,
                Some("active_omniroute_service_attestation_required".into()),
            ));
        }

        let client = OmniRouteClient::new(OmniRouteConfig {
            base_url: LOOPBACK_BASE_URL.into(),
            request_timeout_ms: INFERENCE_REQUEST_TIMEOUT_MS,
            max_response_bytes: INFERENCE_MAX_RESPONSE_BYTES,
        })?;

        let profile = match self
            .async_executor()
            .block_on(client.execution_profile(credential))
        {
            Ok(profile) => profile,
            Err(error) => {
                return Ok(OmniRouteInferenceProbe {
                    status: "runtime_profile_probe_failed",
                    service_active_after: self.omniroute_service_active()?,
                    detail: Some(stable_error_code(&error)),
                    ..base_probe_fields(
                        credential_mode,
                        requested_model_id,
                        active_before,
                        service_attested,
                    )
                });
            }
        };
        let runtime_profile_verified = profile.gateway_id == VIBECODER_OMNIROUTE_GATEWAY_ID
            && profile.upstream_version == VIBECODER_OMNIROUTE_UPSTREAM_VERSION
            && profile.profile_id == VIBECODER_OMNIROUTE_PROFILE_ID
            && profile.profile_sha256 == VIBECODER_OMNIROUTE_PROFILE_SHA256
            && profile.permits_exact_model_execution();
        if !runtime_profile_verified {
            return Err(host_error("android_host_omniroute_inference_profile_mismatch"));
        }

        let catalog = match self.async_executor().block_on(client.list_models(credential)) {
            Ok(models) => models,
            Err(error) => {
                return Ok(OmniRouteInferenceProbe {
                    status: "catalog_probe_failed",
                    runtime_profile_verified: true,
                    service_active_after: self.omniroute_service_active()?,
                    detail: Some(stable_error_code(&error)),
                    ..base_probe_fields(
                        credential_mode,
                        requested_model_id,
                        active_before,
                        service_attested,
                    )
                });
            }
        };
        let Some(model) = catalog.into_iter().find(|model| model.id == requested_model_id) else {
            return Ok(OmniRouteInferenceProbe {
                status: "requested_model_not_in_catalog",
                runtime_profile_verified: true,
                service_active_after: self.omniroute_service_active()?,
                detail: Some("requested_model_not_in_usable_catalog".into()),
                ..base_probe_fields(
                    credential_mode,
                    requested_model_id,
                    active_before,
                    service_attested,
                )
            });
        };

        let request = GatewayChatRequest {
            model,
            messages: vec![GatewayChatMessage {
                role: GatewayChatRole::User,
                content: prompt.to_owned(),
            }],
            max_output_tokens: DIAGNOSTIC_MAX_OUTPUT_TOKENS,
        };

        // Exactly one inference call. VibeCoder does not retry, fallback, or advance to another
        // model in Part 34.5. Same-model provider/account handling inside the attested OmniRoute
        // runtime remains governed by the pinned deterministic profile.
        let response = self
            .async_executor()
            .block_on(client.chat_completion(credential, &request));
        let active_after = self.omniroute_service_active()?;
        match response {
            Ok(response) => {
                let observed_match = response
                    .observed_model_id
                    .as_deref()
                    .map(|value| value == requested_model_id);
                if observed_match == Some(false) {
                    return Ok(OmniRouteInferenceProbe {
                        status: "observed_model_mismatch",
                        runtime_profile_verified: true,
                        catalog_model_verified: true,
                        service_active_after: active_after,
                        observed_model_id: response.observed_model_id,
                        observed_model_matches_request: observed_match,
                        inference_request_sent: true,
                        inference_requests_count: 1,
                        response_received: true,
                        response_nonempty: !response.text.is_empty(),
                        response_utf8_bytes: response.text.len(),
                        finish_reason: response.finish_reason,
                        usage: response.usage,
                        detail: Some("observed_model_identity_mismatch".into()),
                        ..base_probe_fields(
                            credential_mode,
                            requested_model_id,
                            active_before,
                            service_attested,
                        )
                    });
                }
                let response_nonempty = !response.text.is_empty();
                Ok(OmniRouteInferenceProbe {
                    status: if active_after && response_nonempty {
                        "first_model_response_received"
                    } else if !active_after {
                        "service_exited_during_inference"
                    } else {
                        "empty_model_response"
                    },
                    runtime_profile_verified: true,
                    catalog_model_verified: true,
                    service_active_after: active_after,
                    observed_model_id: response.observed_model_id,
                    observed_model_matches_request: observed_match,
                    inference_request_sent: true,
                    inference_requests_count: 1,
                    response_received: true,
                    response_nonempty,
                    response_utf8_bytes: response.text.len(),
                    finish_reason: response.finish_reason,
                    usage: response.usage,
                    first_model_request_proven: active_after && response_nonempty,
                    ..base_probe_fields(
                        credential_mode,
                        requested_model_id,
                        active_before,
                        service_attested,
                    )
                })
            }
            Err(error) => Ok(OmniRouteInferenceProbe {
                status: if active_after {
                    "inference_failed"
                } else {
                    "service_exited_during_inference"
                },
                runtime_profile_verified: true,
                catalog_model_verified: true,
                service_active_after: active_after,
                inference_request_sent: true,
                inference_requests_count: 1,
                detail: Some(stable_error_code(&error)),
                ..base_probe_fields(
                    credential_mode,
                    requested_model_id,
                    active_before,
                    service_attested,
                )
            }),
        }
    }
}

fn base_probe(
    status: &'static str,
    credential_mode: &'static str,
    requested_model_id: &str,
    active_before: bool,
    active_after: bool,
    service_attested: bool,
    detail: Option<String>,
) -> OmniRouteInferenceProbe {
    OmniRouteInferenceProbe {
        status,
        service_active_after: active_after,
        detail,
        ..base_probe_fields(
            credential_mode,
            requested_model_id,
            active_before,
            service_attested,
        )
    }
}

fn base_probe_fields(
    credential_mode: &'static str,
    requested_model_id: &str,
    active_before: bool,
    service_attested: bool,
) -> OmniRouteInferenceProbe {
    OmniRouteInferenceProbe {
        schema: 1,
        component_id: "omniroute",
        status: "not_probed",
        base_url: LOOPBACK_BASE_URL,
        credential_mode,
        credential_persisted: false,
        prompt_persisted: false,
        response_text_persisted: false,
        service_attested,
        service_active_before: active_before,
        service_active_after: false,
        runtime_profile_verified: false,
        catalog_model_verified: false,
        requested_model_id: requested_model_id.to_owned(),
        observed_model_id: None,
        observed_model_matches_request: None,
        inference_request_sent: false,
        inference_requests_count: 0,
        automatic_retry_or_model_fallback: false,
        response_received: false,
        response_nonempty: false,
        response_utf8_bytes: 0,
        finish_reason: None,
        usage: None,
        first_model_request_proven: false,
        detail: None,
    }
}

fn validate_model_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.trim() != value
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(host_error("android_host_omniroute_inference_model_invalid"));
    }
    Ok(())
}

fn validate_prompt(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 * 1024 || value.contains('\0') {
        return Err(host_error("android_host_omniroute_inference_prompt_invalid"));
    }
    Ok(())
}

fn stable_error_code(error: &VibeCoderError) -> String {
    match error {
        VibeCoderError::Gateway(code) => bounded_stable_code(code, "gateway_error"),
        VibeCoderError::InvalidRequest(_) => "invalid_request".into(),
        VibeCoderError::Cancelled => "cancelled".into(),
        _ => "inference_probe_failed".into(),
    }
}

fn bounded_stable_code(value: &str, fallback: &'static str) -> String {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        value.to_owned()
    } else {
        fallback.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_bounds_reject_empty_model_and_prompt() {
        assert!(validate_model_id("").is_err());
        assert!(validate_prompt("").is_err());
        assert!(validate_model_id("provider/model").is_ok());
        assert!(validate_prompt("hello").is_ok());
    }

    #[test]
    fn stable_error_code_never_forwards_arbitrary_text() {
        assert_eq!(
            stable_error_code(&VibeCoderError::InvalidRequest("prompt secret".into())),
            "invalid_request"
        );
        assert_eq!(
            stable_error_code(&VibeCoderError::Gateway("HTTP secret=abc".into())),
            "gateway_error"
        );
    }
}
