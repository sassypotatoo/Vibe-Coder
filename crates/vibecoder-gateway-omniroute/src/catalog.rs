use crate::client::RawGatewayResponse;
use serde::Deserialize;
use std::collections::HashSet;
use vibecoder_domain::{ModelRef, Result, VibeCoderError};
use vibecoder_gateway_contract::{GatewayCredential, GatewayHealthStatus};

const MAX_CATALOG_ENTRIES: usize = 20_000;
const MAX_MODEL_ID_BYTES: usize = 512;
const MAX_DISPLAY_NAME_BYTES: usize = 512;
const MAX_PROVIDER_BYTES: usize = 256;
const MAX_TYPE_BYTES: usize = 64;
const MAX_ENDPOINTS_PER_MODEL: usize = 32;
const MAX_ENDPOINT_NAME_BYTES: usize = 64;

#[derive(Debug, Deserialize)]
struct CatalogEnvelope {
    object: String,
    data: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    id: String,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "type")]
    model_type: Option<String>,
    #[serde(default)]
    supported_endpoints: Option<Vec<String>>,
}

pub(crate) enum CatalogInterpretation {
    Models(Vec<ModelRef>),
    NotReady {
        status: GatewayHealthStatus,
        code: &'static str,
    },
}

pub(crate) fn interpret_catalog_response(
    response: RawGatewayResponse,
    credential: GatewayCredential<'_>,
) -> Result<CatalogInterpretation> {
    match response.status {
        200 => {}
        401 => {
            return Ok(CatalogInterpretation::NotReady {
                status: if credential.is_anonymous() {
                    GatewayHealthStatus::AuthenticationRequired
                } else {
                    GatewayHealthStatus::AuthenticationRejected
                },
                code: if credential.is_anonymous() {
                    "authentication_required"
                } else {
                    "authentication_rejected"
                },
            });
        }
        403 => {
            return Ok(CatalogInterpretation::NotReady {
                status: GatewayHealthStatus::AccessDenied,
                code: "catalog_access_denied",
            });
        }
        404 => {
            return Ok(CatalogInterpretation::NotReady {
                status: GatewayHealthStatus::EndpointNotFound,
                code: "models_endpoint_not_found",
            });
        }
        429 => {
            return Ok(CatalogInterpretation::NotReady {
                status: GatewayHealthStatus::RateLimited,
                code: "catalog_rate_limited",
            });
        }
        500..=599 => {
            return Ok(CatalogInterpretation::NotReady {
                status: GatewayHealthStatus::Unavailable,
                code: "gateway_server_error",
            });
        }
        _ => {
            return Ok(CatalogInterpretation::NotReady {
                status: GatewayHealthStatus::InvalidResponse,
                code: "unexpected_models_status",
            });
        }
    }

    if !is_json_content_type(response.content_type.as_deref()) {
        return Ok(CatalogInterpretation::NotReady {
            status: GatewayHealthStatus::InvalidResponse,
            code: "invalid_models_content_type",
        });
    }
    if response.body.is_empty() {
        return Ok(CatalogInterpretation::NotReady {
            status: GatewayHealthStatus::InvalidResponse,
            code: "empty_models_response",
        });
    }

    let models = parse_catalog(&response.body)?;
    if models.is_empty() {
        return Ok(CatalogInterpretation::NotReady {
            status: GatewayHealthStatus::NoUsableModels,
            code: "no_usable_chat_models",
        });
    }
    Ok(CatalogInterpretation::Models(models))
}

fn parse_catalog(body: &[u8]) -> Result<Vec<ModelRef>> {
    let envelope: CatalogEnvelope =
        serde_json::from_slice(body).map_err(|_| gateway_error("invalid_models_json"))?;
    if envelope.object != "list" {
        return Err(gateway_error("invalid_models_envelope"));
    }
    if envelope.data.len() > MAX_CATALOG_ENTRIES {
        return Err(gateway_error("too_many_models"));
    }

    let mut output = Vec::new();
    let mut seen_ids = HashSet::new();
    for entry in envelope.data {
        validate_catalog_classification(&entry)?;
        if !is_usable_chat_entry(&entry)? {
            continue;
        }
        validate_usable_catalog_entry(&entry)?;
        if !seen_ids.insert(entry.id.clone()) {
            return Err(gateway_error("duplicate_usable_model_id"));
        }
        output.push(ModelRef {
            id: entry.id,
            display_name: entry.name,
            provider: entry.owned_by,
        });
    }
    Ok(output)
}

fn validate_catalog_classification(entry: &CatalogEntry) -> Result<()> {
    if let Some(model_type) = entry.model_type.as_deref() {
        validate_bounded_text(model_type, MAX_TYPE_BYTES, "invalid_model_type")?;
    }
    if let Some(endpoints) = entry.supported_endpoints.as_deref() {
        if endpoints.len() > MAX_ENDPOINTS_PER_MODEL {
            return Err(gateway_error("too_many_model_endpoints"));
        }
        for endpoint in endpoints {
            validate_bounded_text(endpoint, MAX_ENDPOINT_NAME_BYTES, "invalid_model_endpoint")?;
        }
    }
    Ok(())
}

fn validate_usable_catalog_entry(entry: &CatalogEntry) -> Result<()> {
    validate_bounded_text(&entry.id, MAX_MODEL_ID_BYTES, "invalid_model_id")?;
    if let Some(object) = entry.object.as_deref() {
        if object != "model" {
            return Err(gateway_error("invalid_model_object"));
        }
    }
    if let Some(provider) = entry.owned_by.as_deref() {
        validate_bounded_text(provider, MAX_PROVIDER_BYTES, "invalid_model_provider")?;
    }
    if let Some(name) = entry.name.as_deref() {
        validate_bounded_text(name, MAX_DISPLAY_NAME_BYTES, "invalid_model_name")?;
    }
    Ok(())
}

fn is_usable_chat_entry(entry: &CatalogEntry) -> Result<bool> {
    // OmniRoute exposes combo aliases in the same /v1/models catalog with owned_by="combo".
    // A combo may internally select among multiple targets/strategies, so it is not an exact model
    // identity suitable for VibeCoder's deterministic Part 9 route policy.
    if entry.owned_by.as_deref() == Some("combo") {
        return Ok(false);
    }

    if let Some(endpoints) = entry.supported_endpoints.as_deref() {
        let mut seen = HashSet::new();
        for endpoint in endpoints {
            if !seen.insert(endpoint.as_str()) {
                return Err(gateway_error("duplicate_model_endpoint"));
            }
        }
        if endpoints
            .iter()
            .any(|endpoint| endpoint == "chat" || endpoint == "responses")
        {
            return Ok(true);
        }
        if !endpoints.is_empty() {
            return Ok(false);
        }
    }

    Ok(match entry.model_type.as_deref() {
        None | Some("") | Some("chat") => true,
        Some(_) => false,
    })
}

fn validate_bounded_text(value: &str, max_bytes: usize, code: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(gateway_error(code));
    }
    Ok(())
}

pub(crate) fn is_json_content_type(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let media_type = value.split(';').next().unwrap_or("").trim();
    media_type.eq_ignore_ascii_case("application/json")
        || media_type
            .to_ascii_lowercase()
            .strip_prefix("application/")
            .is_some_and(|suffix| suffix.ends_with("+json"))
}

fn gateway_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Gateway(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(body: &str) -> RawGatewayResponse {
        RawGatewayResponse {
            status: 200,
            content_type: Some("application/json; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn maps_chat_and_filters_specialty_only_entries() {
        let raw = response(
            r#"{"object":"list","data":[
                {"id":"anthropic/claude","object":"model","owned_by":"anthropic","name":"Claude"},
                {"id":"openai/gpt","object":"model","owned_by":"openai","type":"chat"},
                {"id":"openai/vision-chat","object":"model","owned_by":"openai","type":"image","supported_endpoints":["images","chat"]},
                {"id":"openai/embed","object":"model","owned_by":"openai","type":"embedding"},
                {"id":"openai/image","object":"model","owned_by":"openai","type":"image","supported_endpoints":["images"]},
                {"id":"smart-combo","object":"model","owned_by":"combo","type":"chat"}
            ]}"#,
        );
        let CatalogInterpretation::Models(models) =
            interpret_catalog_response(raw, GatewayCredential::Anonymous).unwrap()
        else {
            panic!("expected models");
        };
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "anthropic/claude");
        assert_eq!(models[0].provider.as_deref(), Some("anthropic"));
    }

    #[test]
    fn rejects_duplicate_usable_chat_ids() {
        let raw = response(
            r#"{"object":"list","data":[
                {"id":"openai/gpt","object":"model"},
                {"id":"openai/gpt","object":"model","type":"chat"}
            ]}"#,
        );
        assert!(interpret_catalog_response(raw, GatewayCredential::Anonymous).is_err());
    }

    #[test]
    fn distinguishes_missing_and_rejected_auth() {
        let raw = RawGatewayResponse {
            status: 401,
            content_type: Some("application/json".into()),
            body: Vec::new(),
        };
        let CatalogInterpretation::NotReady { status, .. } =
            interpret_catalog_response(raw, GatewayCredential::Anonymous).unwrap()
        else {
            panic!("expected not ready");
        };
        assert_eq!(status, GatewayHealthStatus::AuthenticationRequired);

        let raw = RawGatewayResponse {
            status: 401,
            content_type: Some("application/json".into()),
            body: Vec::new(),
        };
        let CatalogInterpretation::NotReady { status, .. } =
            interpret_catalog_response(raw, GatewayCredential::Secret("bad-key")).unwrap()
        else {
            panic!("expected not ready");
        };
        assert_eq!(status, GatewayHealthStatus::AuthenticationRejected);
    }

    #[test]
    fn rejects_non_json_success_content_type() {
        let raw = RawGatewayResponse {
            status: 200,
            content_type: Some("text/html".into()),
            body: br#"{"object":"list","data":[]}"#.to_vec(),
        };
        let CatalogInterpretation::NotReady { code, .. } =
            interpret_catalog_response(raw, GatewayCredential::Anonymous).unwrap()
        else {
            panic!("expected not ready");
        };
        assert_eq!(code, "invalid_models_content_type");
    }
}
