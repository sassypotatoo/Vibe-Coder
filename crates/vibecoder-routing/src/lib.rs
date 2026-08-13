//! Provider-neutral model route policy for VibeCoder.
//!
//! Part 9 deliberately models routing without executing inference. A later orchestration layer can
//! consume the resolved ordered plan, but it must obey the safety boundary encoded here: automatic
//! fallback is allowed only before any response/tool activity has started for the failed attempt.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use vibecoder_domain::{ModelRef, Result, VibeCoderError};

const MAX_ROUTE_TARGETS: usize = 8;
const MAX_MODEL_ID_BYTES: usize = 512;
const MAX_PROVIDER_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteTargetConfig {
    pub model_id: String,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackTrigger {
    RateLimited,
    Timeout,
    ProviderUnavailable,
    ModelUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FallbackBoundary {
    /// Fallback may advance only before assistant output, tool execution, or other observable turn
    /// progress has begun. This is the only safe automatic boundary currently supported.
    #[default]
    BeforeResponseOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoutePolicyConfig {
    pub primary: ModelRouteTargetConfig,
    #[serde(default)]
    pub fallbacks: Vec<ModelRouteTargetConfig>,
    #[serde(default = "default_fallback_triggers")]
    pub fallback_on: Vec<FallbackTrigger>,
    #[serde(default)]
    pub fallback_boundary: FallbackBoundary,
}

fn default_fallback_triggers() -> Vec<FallbackTrigger> {
    vec![
        FallbackTrigger::RateLimited,
        FallbackTrigger::Timeout,
        FallbackTrigger::ProviderUnavailable,
        FallbackTrigger::ModelUnavailable,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteFailureClass {
    RateLimited,
    Timeout,
    ProviderUnavailable,
    GatewayUnavailable,
    ModelUnavailable,
    Authentication,
    AccessDenied,
    InvalidRequest,
    Cancelled,
    ProtocolError,
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RouteAttemptState {
    route_index: usize,
    response_started: bool,
    tool_activity_started: bool,
}

impl RouteAttemptState {
    const fn pristine(route_index: usize) -> Self {
        Self {
            route_index,
            response_started: false,
            tool_activity_started: false,
        }
    }

    pub const fn route_index(&self) -> usize {
        self.route_index
    }

    pub const fn observable_progress_started(&self) -> bool {
        self.response_started || self.tool_activity_started
    }

    pub fn mark_response_started(&mut self) {
        self.response_started = true;
    }

    pub fn mark_tool_activity_started(&mut self) {
        self.tool_activity_started = true;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStopReason {
    ObservableProgressAlreadyStarted,
    FailureNotSafeForFallback,
    FailureNotEnabledForFallback,
    FallbacksExhausted,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RouteDecision {
    Fallback {
        next_attempt: RouteAttemptState,
        model: ModelRef,
    },
    Stop {
        reason: RouteStopReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelRoutePolicy {
    routes: Vec<ModelRef>,
    fallback_on: Vec<FallbackTrigger>,
    fallback_boundary: FallbackBoundary,
}

impl ModelRouteTargetConfig {
    /// Validate one persisted exact model identity without consulting a live catalog.
    pub fn validate(&self) -> Result<()> {
        validate_target(self)
    }
}

impl ModelRoutePolicyConfig {
    /// Validate persisted policy shape and target identities without requiring a live catalog.
    pub fn validate(&self) -> Result<()> {
        validate_policy_shape(self)?;
        let mut seen = HashSet::new();
        for target in std::iter::once(&self.primary).chain(self.fallbacks.iter()) {
            validate_target(target)?;
            if !seen.insert(target.model_id.as_str()) {
                return Err(routing_error("duplicate_route_model_id"));
            }
        }
        Ok(())
    }
}

impl ResolvedModelRoutePolicy {
    /// Resolve an explicit route policy against one fresh gateway catalog.
    ///
    /// Every configured target must exist exactly. A caller-supplied provider, when present, must
    /// equal catalog `owned_by`; VibeCoder never guesses aliases or silently drops a bad fallback.
    pub fn resolve(config: &ModelRoutePolicyConfig, catalog: &[ModelRef]) -> Result<Self> {
        config.validate()?;
        let catalog_by_id = index_catalog(catalog)?;

        let mut routes = Vec::with_capacity(1 + config.fallbacks.len());
        let mut seen_route_ids = HashSet::new();
        for target in std::iter::once(&config.primary).chain(config.fallbacks.iter()) {
            validate_target(target)?;
            if !seen_route_ids.insert(target.model_id.as_str()) {
                return Err(routing_error("duplicate_route_model_id"));
            }
            let model = catalog_by_id
                .get(target.model_id.as_str())
                .ok_or_else(|| routing_error("configured_route_model_unavailable"))?;
            if let Some(expected_provider) = target.provider.as_deref() {
                if model.provider.as_deref() != Some(expected_provider) {
                    return Err(routing_error("configured_route_provider_mismatch"));
                }
            }
            routes.push((*model).clone());
        }

        Ok(Self {
            routes,
            fallback_on: config.fallback_on.clone(),
            fallback_boundary: config.fallback_boundary,
        })
    }

    pub fn primary(&self) -> &ModelRef {
        &self.routes[0]
    }

    /// Start automatic routing at the explicit primary. Callers cannot construct an arbitrary
    /// attempt index; subsequent states are issued only by `decision_after_failure`.
    pub fn start_attempt(&self) -> RouteAttemptState {
        RouteAttemptState::pristine(0)
    }

    pub fn routes(&self) -> &[ModelRef] {
        &self.routes
    }

    pub fn fallback_on(&self) -> &[FallbackTrigger] {
        &self.fallback_on
    }

    pub const fn fallback_boundary(&self) -> FallbackBoundary {
        self.fallback_boundary
    }

    /// Decide whether an already-failed attempt may advance to the next explicitly configured
    /// route. This function never chooses a model outside `routes` and never retries after visible
    /// output/tool activity, preventing duplicate coding side effects.
    pub fn decision_after_failure(
        &self,
        state: RouteAttemptState,
        failure: RouteFailureClass,
    ) -> Result<RouteDecision> {
        if state.route_index >= self.routes.len() {
            return Err(routing_error("route_attempt_index_out_of_bounds"));
        }

        match self.fallback_boundary {
            FallbackBoundary::BeforeResponseOnly => {
                if state.observable_progress_started() {
                    return Ok(RouteDecision::Stop {
                        reason: RouteStopReason::ObservableProgressAlreadyStarted,
                    });
                }
            }
        }

        let Some(trigger) = safe_fallback_trigger(failure) else {
            return Ok(RouteDecision::Stop {
                reason: RouteStopReason::FailureNotSafeForFallback,
            });
        };
        if !self.fallback_on.contains(&trigger) {
            return Ok(RouteDecision::Stop {
                reason: RouteStopReason::FailureNotEnabledForFallback,
            });
        }

        let next_route_index = state.route_index + 1;
        let Some(model) = self.routes.get(next_route_index) else {
            return Ok(RouteDecision::Stop {
                reason: RouteStopReason::FallbacksExhausted,
            });
        };

        Ok(RouteDecision::Fallback {
            next_attempt: RouteAttemptState::pristine(next_route_index),
            model: model.clone(),
        })
    }
}

fn validate_policy_shape(config: &ModelRoutePolicyConfig) -> Result<()> {
    let route_count = 1usize.saturating_add(config.fallbacks.len());
    if route_count > MAX_ROUTE_TARGETS {
        return Err(routing_error("too_many_route_targets"));
    }

    let mut seen_triggers = HashSet::new();
    for trigger in &config.fallback_on {
        if !seen_triggers.insert(*trigger) {
            return Err(routing_error("duplicate_fallback_trigger"));
        }
    }

    if !config.fallbacks.is_empty() && config.fallback_on.is_empty() {
        return Err(routing_error("fallback_routes_without_triggers"));
    }
    Ok(())
}

fn validate_target(target: &ModelRouteTargetConfig) -> Result<()> {
    validate_bounded_identity(
        &target.model_id,
        MAX_MODEL_ID_BYTES,
        "invalid_route_model_id",
    )?;
    if let Some(provider) = target.provider.as_deref() {
        validate_bounded_identity(provider, MAX_PROVIDER_BYTES, "invalid_route_provider")?;
    }
    Ok(())
}

fn validate_bounded_identity(value: &str, max_bytes: usize, code: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(routing_error(code));
    }
    Ok(())
}

fn index_catalog(catalog: &[ModelRef]) -> Result<HashMap<&str, &ModelRef>> {
    let mut by_id = HashMap::with_capacity(catalog.len());
    for model in catalog {
        validate_bounded_identity(&model.id, MAX_MODEL_ID_BYTES, "invalid_catalog_model_id")?;
        if let Some(provider) = model.provider.as_deref() {
            validate_bounded_identity(provider, MAX_PROVIDER_BYTES, "invalid_catalog_provider")?;
        }
        if by_id.insert(model.id.as_str(), model).is_some() {
            return Err(routing_error("ambiguous_catalog_model_id"));
        }
    }
    Ok(by_id)
}

fn safe_fallback_trigger(failure: RouteFailureClass) -> Option<FallbackTrigger> {
    match failure {
        RouteFailureClass::RateLimited => Some(FallbackTrigger::RateLimited),
        RouteFailureClass::Timeout => Some(FallbackTrigger::Timeout),
        RouteFailureClass::ProviderUnavailable => Some(FallbackTrigger::ProviderUnavailable),
        RouteFailureClass::ModelUnavailable => Some(FallbackTrigger::ModelUnavailable),
        RouteFailureClass::GatewayUnavailable
        | RouteFailureClass::Authentication
        | RouteFailureClass::AccessDenied
        | RouteFailureClass::InvalidRequest
        | RouteFailureClass::Cancelled
        | RouteFailureClass::ProtocolError
        | RouteFailureClass::Unknown => None,
    }
}

fn routing_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Routing(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, provider: &str) -> ModelRef {
        ModelRef {
            id: id.into(),
            display_name: None,
            provider: Some(provider.into()),
        }
    }

    fn policy() -> ModelRoutePolicyConfig {
        ModelRoutePolicyConfig {
            primary: ModelRouteTargetConfig {
                model_id: "provider-a/model-a".into(),
                provider: Some("provider-a".into()),
            },
            fallbacks: vec![ModelRouteTargetConfig {
                model_id: "provider-b/model-b".into(),
                provider: Some("provider-b".into()),
            }],
            fallback_on: vec![FallbackTrigger::RateLimited, FallbackTrigger::Timeout],
            fallback_boundary: FallbackBoundary::BeforeResponseOnly,
        }
    }

    #[test]
    fn resolves_exact_order_and_provider() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-b/model-b", "provider-b"),
                model("provider-a/model-a", "provider-a"),
            ],
        )
        .unwrap();
        assert_eq!(resolved.routes()[0].id, "provider-a/model-a");
        assert_eq!(resolved.routes()[1].id, "provider-b/model-b");
    }

    #[test]
    fn provider_mismatch_fails_closed() {
        let mut config = policy();
        config.primary.provider = Some("wrong-provider".into());
        assert!(
            ResolvedModelRoutePolicy::resolve(
                &config,
                &[
                    model("provider-a/model-a", "provider-a"),
                    model("provider-b/model-b", "provider-b"),
                ],
            )
            .is_err()
        );
    }

    #[test]
    fn no_fallback_after_observable_progress() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-a/model-a", "provider-a"),
                model("provider-b/model-b", "provider-b"),
            ],
        )
        .unwrap();
        let decision = resolved
            .decision_after_failure(
                {
                    let mut state = resolved.start_attempt();
                    state.mark_response_started();
                    state
                },
                RouteFailureClass::RateLimited,
            )
            .unwrap();
        assert_eq!(
            decision,
            RouteDecision::Stop {
                reason: RouteStopReason::ObservableProgressAlreadyStarted
            }
        );
    }

    #[test]
    fn transient_failure_advances_only_to_configured_next_route() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-a/model-a", "provider-a"),
                model("provider-b/model-b", "provider-b"),
            ],
        )
        .unwrap();
        let decision = resolved
            .decision_after_failure(resolved.start_attempt(), RouteFailureClass::RateLimited)
            .unwrap();
        assert_eq!(
            decision,
            RouteDecision::Fallback {
                next_attempt: RouteAttemptState::pristine(1),
                model: model("provider-b/model-b", "provider-b"),
            }
        );
    }

    #[test]
    fn primary_only_policy_is_valid() {
        let config = ModelRoutePolicyConfig {
            primary: ModelRouteTargetConfig {
                model_id: "provider-a/model-a".into(),
                provider: Some("provider-a".into()),
            },
            fallbacks: Vec::new(),
            fallback_on: default_fallback_triggers(),
            fallback_boundary: FallbackBoundary::BeforeResponseOnly,
        };
        let resolved = ResolvedModelRoutePolicy::resolve(
            &config,
            &[model("provider-a/model-a", "provider-a")],
        )
        .unwrap();
        assert_eq!(resolved.routes().len(), 1);
    }

    #[test]
    fn tool_activity_permanently_blocks_automatic_fallback() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-a/model-a", "provider-a"),
                model("provider-b/model-b", "provider-b"),
            ],
        )
        .unwrap();
        let mut state = resolved.start_attempt();
        state.mark_tool_activity_started();
        let decision = resolved
            .decision_after_failure(state, RouteFailureClass::RateLimited)
            .unwrap();
        assert_eq!(
            decision,
            RouteDecision::Stop {
                reason: RouteStopReason::ObservableProgressAlreadyStarted
            }
        );
    }

    #[test]
    fn local_gateway_outage_never_changes_models() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-a/model-a", "provider-a"),
                model("provider-b/model-b", "provider-b"),
            ],
        )
        .unwrap();
        let decision = resolved
            .decision_after_failure(
                resolved.start_attempt(),
                RouteFailureClass::GatewayUnavailable,
            )
            .unwrap();
        assert_eq!(
            decision,
            RouteDecision::Stop {
                reason: RouteStopReason::FailureNotSafeForFallback
            }
        );
    }

    #[test]
    fn auth_failure_never_falls_back() {
        let resolved = ResolvedModelRoutePolicy::resolve(
            &policy(),
            &[
                model("provider-a/model-a", "provider-a"),
                model("provider-b/model-b", "provider-b"),
            ],
        )
        .unwrap();
        let decision = resolved
            .decision_after_failure(resolved.start_attempt(), RouteFailureClass::Authentication)
            .unwrap();
        assert_eq!(
            decision,
            RouteDecision::Stop {
                reason: RouteStopReason::FailureNotSafeForFallback
            }
        );
    }
}
