use jcode_sdk::{ApiEvent, EventStream, JcodeClient, ModelRouteInfo};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use vibecoder_domain::{ModelRef, Result, SessionId, VibeCoderError};

const MAX_MODEL_ID_BYTES: usize = 512;
const MAX_PROVIDER_BYTES: usize = 128;

/// Tracks whether model discovery was operationally verified on the current transport generation.
///
/// The reviewed Jcode 0.73.0 bridge implements `list_models`/`set_model`, but does not advertise a
/// dedicated `model_selection` hello capability. VibeCoder therefore reports model selection only
/// after a real model-catalog request succeeds on a specific connection generation.
pub(crate) struct ModelCapabilityRegistry {
    verified_generation: Mutex<Option<u64>>,
}

impl ModelCapabilityRegistry {
    pub(crate) fn new() -> Self {
        Self {
            verified_generation: Mutex::new(None),
        }
    }

    pub(crate) fn is_verified(&self, generation: u64) -> bool {
        self.verified_generation
            .lock()
            .map(|value| *value == Some(generation))
            .unwrap_or(false)
    }

    pub(crate) fn mark_verified(&self, generation: u64) -> Result<()> {
        let mut state = self
            .verified_generation
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode model capability lock poisoned".into()))?;
        *state = Some(generation);
        Ok(())
    }
}

/// Wait for the fresh model probe emitted by Jcode after an attach/re-attach.
///
/// This wait is used only on a fresh sidecar API connection to the same live Jcode runtime. The
/// reviewed bridge can retain non-empty catalog fields across attachment changes, so an event wait
/// alone is not a cache reset on a reused client. The sidecar starts with fresh BridgeState; the
/// target session's post-attach `ModelInfo` proves its catalog probe was processed before VibeCoder
/// asks `ListModels`.
pub(crate) fn wait_for_fresh_model_probe(
    events: &EventStream,
    session_id: &SessionId,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(VibeCoderError::Agent(
                "Jcode did not publish fresh model metadata after session attachment".into(),
            ));
        }
        match events.next_timeout(remaining) {
            Some(ApiEvent::ModelInfo {
                session_id: observed,
                ..
            }) if observed.as_str() == session_id.0.as_str() => return Ok(()),
            Some(_) => continue,
            None => {
                return Err(VibeCoderError::Agent(
                    "Jcode model metadata stream ended or timed out after session attachment"
                        .into(),
                ));
            }
        }
    }
}

pub(crate) fn validate_model_ref(model: &ModelRef) -> Result<()> {
    validate_component("model id", &model.id, MAX_MODEL_ID_BYTES)?;
    if let Some(provider) = model.provider.as_deref() {
        validate_component("model provider", provider, MAX_PROVIDER_BYTES)?;
    }
    Ok(())
}

fn validate_component(label: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.trim() != value || value.len() > max_bytes {
        return Err(VibeCoderError::InvalidRequest(format!(
            "{label} is empty, padded, or too long"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(VibeCoderError::InvalidRequest(format!(
            "{label} contains control characters"
        )));
    }
    Ok(())
}

/// Fetch a fresh session-scoped model list and enrich it with provider identity from runtime routes.
///
/// The exact upstream model id is preserved. We do not normalize aliases, strip route suffixes, or
/// synthesize provider prefixes because Jcode expects the id returned by `ListModels` verbatim.
pub(crate) fn discover_models(
    client: &JcodeClient,
    session_id: &SessionId,
) -> Result<Vec<ModelRef>> {
    let (models, _current) = client
        .list_models(&session_id.0)
        .map_err(|error| crate::error::map_operation_error("list_models", error))?;
    let runtime = client
        .get_runtime_info(&session_id.0)
        .map_err(|error| crate::error::map_operation_error("model_runtime_info", error))?;

    if runtime.session_id.as_str() != session_id.0.as_str() {
        return Err(VibeCoderError::Agent(
            "Jcode returned model metadata for a different session".into(),
        ));
    }

    let providers = providers_by_model(&runtime.routes)?;
    let mut seen = HashSet::new();
    let mut catalog = Vec::with_capacity(models.len());
    for id in models {
        let candidate = ModelRef {
            id,
            display_name: None,
            provider: None,
        };
        validate_model_ref(&candidate).map_err(|_| {
            VibeCoderError::Agent("Jcode returned a malformed model identifier".into())
        })?;
        if !seen.insert(candidate.id.clone()) {
            return Err(VibeCoderError::Agent(
                "Jcode returned duplicate model identifiers".into(),
            ));
        }
        let provider = providers.get(&candidate.id).and_then(|values| {
            if values.len() == 1 {
                values.iter().next().cloned()
            } else {
                None
            }
        });
        catalog.push(ModelRef {
            provider,
            ..candidate
        });
    }
    Ok(catalog)
}

fn providers_by_model(routes: &[ModelRouteInfo]) -> Result<HashMap<String, HashSet<String>>> {
    let mut by_model: HashMap<String, HashSet<String>> = HashMap::new();
    for route in routes.iter().filter(|route| route.available) {
        let model = ModelRef {
            id: route.model.clone(),
            display_name: None,
            provider: Some(route.provider.clone()),
        };
        validate_model_ref(&model).map_err(|_| {
            VibeCoderError::Agent("Jcode returned malformed model-route metadata".into())
        })?;
        by_model
            .entry(route.model.clone())
            .or_default()
            .insert(route.provider.clone());
    }
    Ok(by_model)
}

pub(crate) fn select_model_from_catalog(
    client: &JcodeClient,
    session_id: &SessionId,
    requested: &ModelRef,
    catalog: &[ModelRef],
) -> Result<()> {
    validate_model_ref(requested)?;
    let available = catalog
        .iter()
        .find(|candidate| candidate.id == requested.id)
        .ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "requested model is not in this session's current Jcode catalog".into(),
            )
        })?;

    if let Some(expected_provider) = requested.provider.as_deref() {
        match available.provider.as_deref() {
            Some(actual_provider) if actual_provider == expected_provider => {}
            Some(_) => {
                return Err(VibeCoderError::InvalidRequest(
                    "requested model provider does not match Jcode runtime metadata".into(),
                ));
            }
            None => {
                return Err(VibeCoderError::InvalidRequest(
                    "requested model provider cannot be unambiguously verified from Jcode routes"
                        .into(),
                ));
            }
        }
    }

    client
        .set_model(&session_id.0, &requested.id)
        .map_err(|error| crate::error::map_operation_error("set_model", error))?;
    Ok(())
}

/// Corroborate the active model through a fresh, target-session-bound API client.
///
/// The caller must pass a client whose BridgeState was created fresh and whose post-attach
/// `ModelInfo` probe has already been observed. This avoids treating provider/model fields retained
/// on a reused bridge as proof of the switch.
pub(crate) fn verify_active_model(
    client: &JcodeClient,
    session_id: &SessionId,
    requested: &ModelRef,
) -> Result<ModelRef> {
    validate_model_ref(requested)?;
    let runtime = client
        .get_runtime_info(&session_id.0)
        .map_err(|error| crate::error::map_operation_error("set_model_verify", error))?;
    if runtime.session_id.as_str() != session_id.0.as_str() {
        return Err(VibeCoderError::Agent(
            "Jcode verified a model change against a different session".into(),
        ));
    }
    let active_model = runtime.model.ok_or_else(|| {
        VibeCoderError::Agent(
            "Jcode fresh active-model probe did not report a model identity".into(),
        )
    })?;
    if active_model.as_str() != requested.id.as_str() {
        return Err(VibeCoderError::Agent(
            "Jcode acknowledged a model change but a fresh probe did not report the requested model active".into(),
        ));
    }
    let active_provider = runtime.provider.ok_or_else(|| {
        VibeCoderError::Agent(
            "Jcode fresh active-provider probe did not report a provider identity".into(),
        )
    })?;
    if requested.provider.as_deref() != Some(active_provider.as_str()) {
        return Err(VibeCoderError::Agent(
            "Jcode fresh active-provider probe does not match the requested model provider".into(),
        ));
    }
    Ok(ModelRef {
        id: active_model,
        display_name: None,
        provider: Some(active_provider),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, provider: Option<&str>) -> ModelRef {
        ModelRef {
            id: id.to_string(),
            display_name: None,
            provider: provider.map(str::to_string),
        }
    }

    #[test]
    fn model_identity_validation_rejects_padding_and_controls() {
        assert!(validate_model_ref(&model("gpt-5.6", Some("openai"))).is_ok());
        assert!(validate_model_ref(&model(" gpt-5.6", Some("openai"))).is_err());
        assert!(validate_model_ref(&model("gpt-5.6\n", Some("openai"))).is_err());
        assert!(validate_model_ref(&model("gpt-5.6", Some("openai\r"))).is_err());
    }

    #[test]
    fn provider_mapping_is_only_unambiguous_for_one_available_provider() {
        let routes = vec![
            ModelRouteInfo {
                model: "shared-model".into(),
                provider: "provider-a".into(),
                api_method: "a".into(),
                available: true,
                detail: String::new(),
            },
            ModelRouteInfo {
                model: "shared-model".into(),
                provider: "provider-b".into(),
                api_method: "b".into(),
                available: true,
                detail: String::new(),
            },
            ModelRouteInfo {
                model: "unique-model".into(),
                provider: "provider-a".into(),
                api_method: "a".into(),
                available: true,
                detail: String::new(),
            },
        ];
        let mapped = providers_by_model(&routes).unwrap();
        assert_eq!(mapped["shared-model"].len(), 2);
        assert_eq!(mapped["unique-model"].len(), 1);
    }

    #[test]
    fn unavailable_routes_do_not_authorize_provider_identity() {
        let routes = vec![ModelRouteInfo {
            model: "offline-model".into(),
            provider: "provider-a".into(),
            api_method: "a".into(),
            available: false,
            detail: "not configured".into(),
        }];
        let mapped = providers_by_model(&routes).unwrap();
        assert!(!mapped.contains_key("offline-model"));
    }
}
