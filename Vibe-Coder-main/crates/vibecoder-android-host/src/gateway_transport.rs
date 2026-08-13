use super::{AndroidHostRuntime, OmniRouteServiceHandle, host_error};
use crate::omniroute_service::LOOPBACK_BASE_URL;
use serde::Serialize;
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_gateway_contract::{
    GatewayCredential, GatewayHealth, GatewayHealthStatus, ModelGateway,
    VIBECODER_OMNIROUTE_GATEWAY_ID, VIBECODER_OMNIROUTE_PROFILE_ID,
    VIBECODER_OMNIROUTE_PROFILE_SHA256, VIBECODER_OMNIROUTE_UPSTREAM_VERSION,
};
use vibecoder_gateway_omniroute::{OmniRouteClient, OmniRouteConfig};

const GATEWAY_REQUEST_TIMEOUT_MS: u64 = 5_000;
const GATEWAY_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Sanitized Part-34.4 proof that the Android-local host can reach the exact running OmniRoute
/// gateway through its hardened adapter. This is deliberately a health/catalog transport probe,
/// not an inference request; Part 34.5 owns the first model request.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OmniRouteGatewayTransportProbe {
    pub schema: u32,
    pub component_id: &'static str,
    pub status: &'static str,
    pub base_url: &'static str,
    pub credential_mode: &'static str,
    pub credential_persisted: bool,
    pub service_attested: bool,
    pub service_active_before: bool,
    pub service_active_after: bool,
    pub runtime_profile_verified: bool,
    pub runtime_profile_id: Option<String>,
    pub local_transport_round_trip_proven: bool,
    pub catalog_probe_attempted: bool,
    pub catalog_round_trip_reached: bool,
    pub catalog_ready: bool,
    pub health_status: Option<GatewayHealthStatus>,
    pub usable_models: usize,
    pub detail: Option<String>,
    pub inference_request_sent: bool,
    pub first_model_request_proven: bool,
}

impl AndroidHostRuntime {
    pub(crate) fn probe_omniroute_gateway_transport(
        &self,
        service: &OmniRouteServiceHandle,
        credential: GatewayCredential<'_>,
    ) -> Result<OmniRouteGatewayTransportProbe> {
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
                active_before,
                self.omniroute_service_active()?,
                service_attested,
                Some("active_omniroute_service_attestation_required".into()),
            ));
        }

        let client = OmniRouteClient::new(OmniRouteConfig {
            base_url: LOOPBACK_BASE_URL.into(),
            request_timeout_ms: GATEWAY_REQUEST_TIMEOUT_MS,
            max_response_bytes: GATEWAY_MAX_RESPONSE_BYTES,
        })?;

        let profile_result = self
            .async_executor()
            .block_on(client.execution_profile(credential));
        let profile = match profile_result {
            Ok(profile) => profile,
            Err(error) => {
                return Ok(OmniRouteGatewayTransportProbe {
                    status: "runtime_profile_probe_failed",
                    runtime_profile_verified: false,
                    runtime_profile_id: None,
                    local_transport_round_trip_proven: false,
                    catalog_probe_attempted: false,
                    catalog_round_trip_reached: false,
                    catalog_ready: false,
                    health_status: None,
                    usable_models: 0,
                    detail: Some(stable_error_code(&error)),
                    service_active_after: self.omniroute_service_active()?,
                    ..base_probe_fields(credential_mode, active_before, service_attested)
                });
            }
        };
        let runtime_profile_verified = profile.gateway_id == VIBECODER_OMNIROUTE_GATEWAY_ID
            && profile.upstream_version == VIBECODER_OMNIROUTE_UPSTREAM_VERSION
            && profile.profile_id == VIBECODER_OMNIROUTE_PROFILE_ID
            && profile.profile_sha256 == VIBECODER_OMNIROUTE_PROFILE_SHA256
            && profile.permits_exact_model_execution();
        if !runtime_profile_verified {
            return Err(host_error("android_host_omniroute_gateway_profile_mismatch"));
        }

        let health_result = self.async_executor().block_on(client.health(credential));
        let active_after = self.omniroute_service_active()?;
        match health_result {
            Ok(health) => {
                let reached = catalog_round_trip_reached(&health);
                Ok(OmniRouteGatewayTransportProbe {
                    status: if active_after && health.ready {
                        "catalog_ready"
                    } else if active_after && reached {
                        "catalog_classified_not_ready"
                    } else if active_after {
                        "catalog_transport_unavailable"
                    } else {
                        "service_exited_during_probe"
                    },
                    runtime_profile_verified: true,
                    runtime_profile_id: Some(profile.profile_id),
                    local_transport_round_trip_proven: active_after,
                    catalog_probe_attempted: true,
                    catalog_round_trip_reached: reached,
                    catalog_ready: active_after && health.ready,
                    health_status: Some(health.status),
                    usable_models: if active_after { health.usable_models } else { 0 },
                    detail: health.detail,
                    service_active_after: active_after,
                    ..base_probe_fields(credential_mode, active_before, service_attested)
                })
            }
            Err(error) => Ok(OmniRouteGatewayTransportProbe {
                status: if active_after {
                    "catalog_probe_failed"
                } else {
                    "service_exited_during_probe"
                },
                runtime_profile_verified: true,
                runtime_profile_id: Some(profile.profile_id),
                local_transport_round_trip_proven: active_after,
                catalog_probe_attempted: true,
                catalog_round_trip_reached: false,
                catalog_ready: false,
                health_status: None,
                usable_models: 0,
                detail: Some(stable_error_code(&error)),
                service_active_after: active_after,
                ..base_probe_fields(credential_mode, active_before, service_attested)
            }),
        }
    }
}

fn base_probe(
    status: &'static str,
    credential_mode: &'static str,
    active_before: bool,
    active_after: bool,
    service_attested: bool,
    detail: Option<String>,
) -> OmniRouteGatewayTransportProbe {
    OmniRouteGatewayTransportProbe {
        status,
        detail,
        service_active_after: active_after,
        ..base_probe_fields(credential_mode, active_before, service_attested)
    }
}

fn base_probe_fields(
    credential_mode: &'static str,
    active_before: bool,
    service_attested: bool,
) -> OmniRouteGatewayTransportProbe {
    OmniRouteGatewayTransportProbe {
        schema: 1,
        component_id: "omniroute",
        status: "not_probed",
        base_url: LOOPBACK_BASE_URL,
        credential_mode,
        credential_persisted: false,
        service_attested,
        service_active_before: active_before,
        service_active_after: false,
        runtime_profile_verified: false,
        runtime_profile_id: None,
        local_transport_round_trip_proven: false,
        catalog_probe_attempted: false,
        catalog_round_trip_reached: false,
        catalog_ready: false,
        health_status: None,
        usable_models: 0,
        detail: None,
        inference_request_sent: false,
        first_model_request_proven: false,
    }
}

fn catalog_round_trip_reached(health: &GatewayHealth) -> bool {
    !matches!(
        health.detail.as_deref(),
        Some(
            "http_timeout"
                | "http_connect_failed"
                | "http_request_failed"
                | "http_body_failed"
                | "http_transport_failed"
        )
    )
}

fn stable_error_code(error: &VibeCoderError) -> String {
    match error {
        VibeCoderError::Gateway(code) => bounded_stable_code(code, "gateway_error"),
        VibeCoderError::InvalidRequest(_) => "invalid_request".into(),
        VibeCoderError::Cancelled => "cancelled".into(),
        _ => "gateway_probe_failed".into(),
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
    fn transport_reachability_distinguishes_network_failure_from_http_classification() {
        let unavailable = GatewayHealth {
            ready: false,
            status: GatewayHealthStatus::Unavailable,
            usable_models: 0,
            detail: Some("http_connect_failed".into()),
        };
        assert!(!catalog_round_trip_reached(&unavailable));
        let auth = GatewayHealth {
            ready: false,
            status: GatewayHealthStatus::AuthenticationRequired,
            usable_models: 0,
            detail: Some("authentication_required".into()),
        };
        assert!(catalog_round_trip_reached(&auth));
    }

    #[test]
    fn stable_error_code_never_forwards_arbitrary_error_text() {
        assert_eq!(
            stable_error_code(&VibeCoderError::InvalidRequest("secret bearer value".into())),
            "invalid_request"
        );
        assert_eq!(
            stable_error_code(&VibeCoderError::Gateway("HTTP secret=abc".into())),
            "gateway_error"
        );
    }
}
