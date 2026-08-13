use crate::catalog::is_json_content_type;
use crate::client::RawGatewayResponse;
use serde::Deserialize;
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_gateway_contract::{
    GatewayExecutionProfile, VIBECODER_OMNIROUTE_GATEWAY_ID, VIBECODER_OMNIROUTE_PROFILE_ID,
    VIBECODER_OMNIROUTE_PROFILE_SHA256, VIBECODER_OMNIROUTE_UPSTREAM_VERSION,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeProfileEnvelope {
    schema: u8,
    gateway_id: String,
    upstream_version: String,
    profile_id: String,
    profile_sha256: String,
    exact_model_only: bool,
    hidden_model_reroutes_disabled: bool,
}

pub(crate) fn interpret_runtime_profile_response(
    response: RawGatewayResponse,
) -> Result<GatewayExecutionProfile> {
    if response.status != 200 {
        return Err(gateway_error(match response.status {
            401 => "runtime_profile_authentication_rejected",
            403 => "runtime_profile_access_denied",
            404 => "runtime_profile_endpoint_not_found",
            429 => "runtime_profile_rate_limited",
            500..=599 => "runtime_profile_gateway_unavailable",
            _ => "unexpected_runtime_profile_status",
        }));
    }
    if !is_json_content_type(response.content_type.as_deref()) {
        return Err(gateway_error("invalid_runtime_profile_content_type"));
    }
    if response.body.is_empty() {
        return Err(gateway_error("empty_runtime_profile_response"));
    }
    let profile: RuntimeProfileEnvelope = serde_json::from_slice(&response.body)
        .map_err(|_| gateway_error("invalid_runtime_profile_json"))?;
    if profile.schema != 1
        || profile.gateway_id != VIBECODER_OMNIROUTE_GATEWAY_ID
        || profile.upstream_version != VIBECODER_OMNIROUTE_UPSTREAM_VERSION
        || profile.profile_id != VIBECODER_OMNIROUTE_PROFILE_ID
        || profile.profile_sha256 != VIBECODER_OMNIROUTE_PROFILE_SHA256
        || !profile.exact_model_only
        || !profile.hidden_model_reroutes_disabled
    {
        return Err(gateway_error("runtime_profile_attestation_mismatch"));
    }
    Ok(GatewayExecutionProfile {
        gateway_id: profile.gateway_id,
        upstream_version: profile.upstream_version,
        profile_id: profile.profile_id,
        profile_sha256: profile.profile_sha256,
        exact_model_only: profile.exact_model_only,
        hidden_model_reroutes_disabled: profile.hidden_model_reroutes_disabled,
    })
}

fn gateway_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Gateway(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct RuntimeProfileFixture {
        schema: u8,
        profile_id: String,
        profile_sha256: String,
        cases: Vec<RuntimeProfileCase>,
    }

    #[derive(Debug, Deserialize)]
    struct RuntimeProfileCase {
        name: String,
        status: u16,
        content_type: Option<String>,
        body: String,
        expected: RuntimeProfileExpected,
    }

    #[derive(Debug, Deserialize)]
    struct RuntimeProfileExpected {
        kind: String,
        code: Option<String>,
    }

    #[test]
    fn part24_runtime_profile_fixtures_fail_closed() {
        let fixture: RuntimeProfileFixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/part24/runtime_profiles.json"
        ))
        .expect("Part 24 runtime-profile fixture must parse");
        assert_eq!(fixture.schema, 1);
        assert_eq!(fixture.profile_id, VIBECODER_OMNIROUTE_PROFILE_ID);
        assert_eq!(fixture.profile_sha256, VIBECODER_OMNIROUTE_PROFILE_SHA256);

        for case in fixture.cases {
            let result = interpret_runtime_profile_response(RawGatewayResponse {
                status: case.status,
                content_type: case.content_type,
                body: case.body.into_bytes(),
            });
            match case.expected.kind.as_str() {
                "accepted" => {
                    let profile = result.unwrap_or_else(|error| {
                        panic!("fixture {} unexpectedly failed: {error}", case.name)
                    });
                    assert!(profile.permits_exact_model_execution(), "{}", case.name);
                    assert_eq!(profile.profile_id, VIBECODER_OMNIROUTE_PROFILE_ID);
                    assert_eq!(profile.profile_sha256, VIBECODER_OMNIROUTE_PROFILE_SHA256);
                    assert!(case.expected.code.is_none(), "{}", case.name);
                }
                "rejected" => {
                    let error =
                        result.expect_err(&format!("fixture {} unexpectedly accepted", case.name));
                    let VibeCoderError::Gateway(code) = error else {
                        panic!("fixture {} returned wrong error variant", case.name);
                    };
                    assert_eq!(Some(code), case.expected.code, "{}", case.name);
                }
                other => panic!("fixture {} has unknown expected kind {other}", case.name),
            }
        }
    }
}
