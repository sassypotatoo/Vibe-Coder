//! Authority-free end-to-end backend task state for VibeCoder Part 23.
//!
//! This crate observes normalized agent events and decides route transitions, but has no network,
//! process, file, command, secret, gateway, or agent-runtime dependency. Core retains authority and
//! must obtain a deterministic gateway attestation before preparing this machine.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use vibecoder_domain::{
    AgentEvent, ModelRef, ProjectId, Result, SessionId, TurnResult, VibeCoderError,
};
use vibecoder_routing::{
    ResolvedModelRoutePolicy, RouteAttemptState, RouteDecision, RouteFailureClass, RouteStopReason,
};

const MAX_PROMPT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BackendTaskId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTaskPhase {
    Prepared,
    AgentCatalogCorroborated,
    ActiveModelCorroborated,
    Running,
    Stopped,
}

#[derive(Debug, Default)]
struct AttemptProgress {
    response_started: bool,
    tool_activity_started: bool,
}

#[derive(Clone, Default)]
pub struct BackendTaskEventObserver {
    progress: Arc<Mutex<AttemptProgress>>,
}

impl std::fmt::Debug for BackendTaskEventObserver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BackendTaskEventObserver(..)")
    }
}

impl BackendTaskEventObserver {
    pub fn observe(&self, event: &AgentEvent) {
        let mut progress = match self.progress.lock() {
            Ok(progress) => progress,
            Err(poisoned) => poisoned.into_inner(),
        };
        match event {
            AgentEvent::TextDelta { .. } | AgentEvent::BackgroundProgress { .. } => {
                progress.response_started = true;
            }
            AgentEvent::ToolStarted { .. } | AgentEvent::ToolFinished { .. } => {
                progress.tool_activity_started = true;
            }
            AgentEvent::MessageAccepted
            | AgentEvent::SessionStatus { .. }
            | AgentEvent::TokenUsage(_)
            | AgentEvent::PermissionRequired(_)
            | AgentEvent::TurnCompleted
            | AgentEvent::Warning { .. } => {}
        }
    }

    fn snapshot(&self) -> AttemptProgress {
        let progress = match self.progress.lock() {
            Ok(progress) => progress,
            Err(poisoned) => poisoned.into_inner(),
        };
        AttemptProgress {
            response_started: progress.response_started,
            tool_activity_started: progress.tool_activity_started,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTaskFailureDecision {
    RetryConfiguredFallback,
    Stop(RouteStopReason),
}

pub struct BackendTaskStateMachine {
    task_id: BackendTaskId,
    project_id: ProjectId,
    session_id: SessionId,
    route_policy: ResolvedModelRoutePolicy,
    attempt: Option<RouteAttemptState>,
    selected_model: ModelRef,
    phase: BackendTaskPhase,
    observer: Option<BackendTaskEventObserver>,
}

impl std::fmt::Debug for BackendTaskStateMachine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackendTaskStateMachine")
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("session_id", &self.session_id)
            .field("selected_model", &self.selected_model)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}

impl BackendTaskStateMachine {
    pub fn prepare(
        project_id: ProjectId,
        session_id: SessionId,
        prompt: &str,
        route_policy: ResolvedModelRoutePolicy,
    ) -> Result<Self> {
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
            return Err(task_error("invalid_backend_task_prompt"));
        }
        let selected_model = route_policy.primary().clone();
        let attempt = route_policy.start_attempt();
        Ok(Self {
            task_id: BackendTaskId(Uuid::new_v4()),
            project_id,
            session_id,
            route_policy,
            attempt: Some(attempt),
            selected_model,
            phase: BackendTaskPhase::Prepared,
            observer: None,
        })
    }

    pub const fn task_id(&self) -> BackendTaskId {
        self.task_id
    }

    pub const fn phase(&self) -> BackendTaskPhase {
        self.phase
    }

    pub fn selected_model(&self) -> &ModelRef {
        &self.selected_model
    }

    pub fn corroborate_agent_catalog(&mut self, catalog: &[ModelRef]) -> Result<bool> {
        self.require_phase(BackendTaskPhase::Prepared)?;
        require_provider(&self.selected_model, "gateway_model_provider_missing")?;
        let mut seen = HashSet::new();
        let mut exact = None;
        for candidate in catalog {
            if !seen.insert(candidate.id.as_str()) {
                return Err(task_error("ambiguous_agent_catalog_model_id"));
            }
            if candidate.id == self.selected_model.id {
                exact = Some(candidate);
            }
        }
        let Some(candidate) = exact else {
            return Ok(false);
        };
        require_exact_identity(
            &self.selected_model,
            candidate,
            "agent_catalog_identity_mismatch",
        )?;
        self.phase = BackendTaskPhase::AgentCatalogCorroborated;
        Ok(true)
    }

    /// Corroborate an agent catalog when the agent is itself an exact-id transport client of the
    /// already-attested model gateway. Gateway `provider` metadata names the upstream owner, while
    /// the agent runtime reports its local transport provider (for Jcode: `OpenAI-compatible`).
    /// In this mode provider equality would be a category error, so require the exact model id plus
    /// the independently attested transport-provider identity instead.
    pub fn corroborate_bridged_agent_catalog(
        &mut self,
        catalog: &[ModelRef],
        transport_provider: &str,
    ) -> Result<bool> {
        self.require_phase(BackendTaskPhase::Prepared)?;
        require_provider(&self.selected_model, "gateway_model_provider_missing")?;
        if transport_provider.is_empty()
            || transport_provider.trim() != transport_provider
            || transport_provider.chars().any(char::is_control)
        {
            return Err(task_error("agent_bridge_transport_provider_invalid"));
        }
        let mut seen = HashSet::new();
        let mut exact = None;
        for candidate in catalog {
            if !seen.insert(candidate.id.as_str()) {
                return Err(task_error("ambiguous_agent_catalog_model_id"));
            }
            if candidate.id == self.selected_model.id {
                exact = Some(candidate);
            }
        }
        let Some(candidate) = exact else {
            return Ok(false);
        };
        if candidate.provider.as_deref() != Some(transport_provider) {
            return Err(task_error("agent_bridge_transport_provider_mismatch"));
        }
        self.phase = BackendTaskPhase::AgentCatalogCorroborated;
        Ok(true)
    }

    pub fn corroborate_active_model(&mut self, active: &ModelRef) -> Result<()> {
        self.require_phase(BackendTaskPhase::AgentCatalogCorroborated)?;
        require_exact_identity(
            &self.selected_model,
            active,
            "agent_active_identity_mismatch",
        )?;
        self.phase = BackendTaskPhase::ActiveModelCorroborated;
        Ok(())
    }

    pub fn begin_inference(&mut self) -> Result<BackendTaskEventObserver> {
        self.require_phase(BackendTaskPhase::ActiveModelCorroborated)?;
        let observer = BackendTaskEventObserver::default();
        self.observer = Some(observer.clone());
        self.phase = BackendTaskPhase::Running;
        Ok(observer)
    }

    pub fn decide_failure(
        &mut self,
        failure: RouteFailureClass,
    ) -> Result<BackendTaskFailureDecision> {
        if !matches!(
            self.phase,
            BackendTaskPhase::Prepared
                | BackendTaskPhase::AgentCatalogCorroborated
                | BackendTaskPhase::ActiveModelCorroborated
                | BackendTaskPhase::Running
        ) {
            return Err(task_error("backend_task_failure_in_invalid_phase"));
        }
        let mut attempt = self
            .attempt
            .take()
            .ok_or_else(|| task_error("backend_task_attempt_already_consumed"))?;
        if let Some(observer) = self.observer.take() {
            let progress = observer.snapshot();
            if progress.response_started {
                attempt.mark_response_started();
            }
            if progress.tool_activity_started {
                attempt.mark_tool_activity_started();
            }
        }
        match self.route_policy.decision_after_failure(attempt, failure)? {
            RouteDecision::Fallback {
                next_attempt,
                model,
            } => {
                self.attempt = Some(next_attempt);
                self.selected_model = model;
                self.phase = BackendTaskPhase::Prepared;
                Ok(BackendTaskFailureDecision::RetryConfiguredFallback)
            }
            RouteDecision::Stop { reason } => {
                self.phase = BackendTaskPhase::Stopped;
                Ok(BackendTaskFailureDecision::Stop(reason))
            }
        }
    }

    pub fn complete(mut self, turn: TurnResult) -> Result<BackendTaskOutcome> {
        self.require_phase(BackendTaskPhase::Running)?;
        if turn.cancelled {
            return Err(VibeCoderError::Cancelled);
        }
        let attempt_index = self
            .attempt
            .take()
            .ok_or_else(|| task_error("backend_task_attempt_already_consumed"))?
            .route_index();
        Ok(BackendTaskOutcome {
            task_id: self.task_id,
            project_id: self.project_id,
            session_id: self.session_id,
            model: self.selected_model,
            attempt_index,
            turn,
        })
    }

    fn require_phase(&self, expected: BackendTaskPhase) -> Result<()> {
        if self.phase != expected {
            return Err(task_error("backend_task_phase_violation"));
        }
        Ok(())
    }
}

pub struct BackendTaskOutcome {
    task_id: BackendTaskId,
    project_id: ProjectId,
    session_id: SessionId,
    model: ModelRef,
    attempt_index: usize,
    turn: TurnResult,
}

impl std::fmt::Debug for BackendTaskOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackendTaskOutcome")
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("session_id", &self.session_id)
            .field("model", &self.model)
            .field("attempt_index", &self.attempt_index)
            .field(
                "turn_text",
                &format_args!("[REDACTED; {} byte(s)]", self.turn.text.len()),
            )
            .field("tool_call_count", &self.turn.tool_calls.len())
            .field("usage_present", &self.turn.usage.is_some())
            .finish()
    }
}

impl BackendTaskOutcome {
    pub const fn task_id(&self) -> BackendTaskId {
        self.task_id
    }
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
    pub fn model(&self) -> &ModelRef {
        &self.model
    }
    pub const fn attempt_index(&self) -> usize {
        self.attempt_index
    }
    pub fn turn(&self) -> &TurnResult {
        &self.turn
    }
    pub fn into_turn(self) -> TurnResult {
        self.turn
    }
}

pub const fn classify_agent_failure(error: &VibeCoderError) -> RouteFailureClass {
    match error {
        VibeCoderError::Cancelled => RouteFailureClass::Cancelled,
        // Agent errors are prose-backed today. Never infer rate-limit/timeout/provider state from
        // strings; a later typed adapter error can extend this mapping safely.
        VibeCoderError::Agent(_) => RouteFailureClass::Unknown,
        VibeCoderError::InvalidRequest(_) => RouteFailureClass::InvalidRequest,
        _ => RouteFailureClass::ProtocolError,
    }
}

fn require_provider<'a>(model: &'a ModelRef, code: &'static str) -> Result<&'a str> {
    model
        .provider
        .as_deref()
        .filter(|provider| !provider.is_empty())
        .ok_or_else(|| task_error(code))
}

fn require_exact_identity(
    expected: &ModelRef,
    actual: &ModelRef,
    code: &'static str,
) -> Result<()> {
    let expected_provider = require_provider(expected, "gateway_model_provider_missing")?;
    let actual_provider = require_provider(actual, "agent_model_provider_missing")?;
    if expected.id != actual.id || expected_provider != actual_provider {
        return Err(task_error(code));
    }
    Ok(())
}

fn task_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Routing(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use vibecoder_routing::{
        FallbackBoundary, FallbackTrigger, ModelRoutePolicyConfig, ModelRouteTargetConfig,
    };

    #[derive(Debug, Deserialize)]
    struct TaskStateFixture {
        schema: u8,
        catalog_cases: Vec<CatalogCase>,
        active_identity_cases: Vec<ActiveIdentityCase>,
        progress_cases: Vec<ProgressCase>,
        completion_cases: Vec<CompletionCase>,
    }

    #[derive(Debug, Deserialize)]
    struct CatalogCase {
        name: String,
        catalog: Vec<ModelRef>,
        expected: String,
        expected_error: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct ActiveIdentityCase {
        name: String,
        active: ModelRef,
        expected: String,
        expected_error: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct ProgressCase {
        name: String,
        event: String,
        failure: String,
        expected: String,
    }

    #[derive(Debug, Deserialize)]
    struct CompletionCase {
        name: String,
        cancelled: bool,
        expected: String,
    }

    fn model(id: &str, provider: &str) -> ModelRef {
        ModelRef {
            id: id.into(),
            display_name: None,
            provider: Some(provider.into()),
        }
    }

    fn machine() -> BackendTaskStateMachine {
        let policy = ModelRoutePolicyConfig {
            primary: ModelRouteTargetConfig {
                model_id: "a/m".into(),
                provider: Some("a".into()),
            },
            fallbacks: vec![ModelRouteTargetConfig {
                model_id: "b/m".into(),
                provider: Some("b".into()),
            }],
            fallback_on: vec![FallbackTrigger::ModelUnavailable],
            fallback_boundary: FallbackBoundary::BeforeResponseOnly,
        };
        BackendTaskStateMachine::prepare(
            ProjectId::new(),
            SessionId::parse("session").unwrap(),
            "build the project",
            ResolvedModelRoutePolicy::resolve(&policy, &[model("a/m", "a"), model("b/m", "b")])
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn bridged_catalog_uses_transport_provider_but_preserves_gateway_upstream_identity() {
        let mut task = machine();
        assert!(
            task.corroborate_bridged_agent_catalog(
                &[model("a/m", "OpenAI-compatible")],
                "OpenAI-compatible",
            )
            .unwrap()
        );
        task.corroborate_active_model(&model("a/m", "a")).unwrap();
        assert_eq!(task.phase(), BackendTaskPhase::ActiveModelCorroborated);
    }

    #[test]
    fn bridged_catalog_rejects_wrong_transport_provider() {
        let mut task = machine();
        let error = task
            .corroborate_bridged_agent_catalog(
                &[model("a/m", "OpenAI-compatible")],
                "Other-transport",
            )
            .expect_err("bridge transport mismatch must fail closed");
        assert_eq!(
            routing_error_code(error),
            Some("agent_bridge_transport_provider_mismatch".into())
        );
    }

    #[test]
    fn missing_primary_can_use_only_configured_fallback_before_inference() {
        let mut task = machine();
        assert!(
            !task
                .corroborate_agent_catalog(&[model("b/m", "b")])
                .unwrap()
        );
        assert_eq!(
            task.decide_failure(RouteFailureClass::ModelUnavailable)
                .unwrap(),
            BackendTaskFailureDecision::RetryConfiguredFallback
        );
        assert_eq!(task.selected_model().id, "b/m");
    }

    #[test]
    fn tool_activity_blocks_replay() {
        let mut task = machine();
        assert!(
            task.corroborate_agent_catalog(&[model("a/m", "a")])
                .unwrap()
        );
        task.corroborate_active_model(&model("a/m", "a")).unwrap();
        let observer = task.begin_inference().unwrap();
        observer.observe(&AgentEvent::ToolStarted {
            tool: "edit".into(),
            call_id: "1".into(),
        });
        assert_eq!(
            task.decide_failure(RouteFailureClass::ModelUnavailable)
                .unwrap(),
            BackendTaskFailureDecision::Stop(RouteStopReason::ObservableProgressAlreadyStarted)
        );
    }

    #[test]
    fn part24_state_machine_fixtures_fail_closed() {
        let fixture: TaskStateFixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/part24/task_state_contracts.json"
        ))
        .expect("Part 24 task-state fixture must parse");
        assert_eq!(fixture.schema, 1);

        for case in fixture.catalog_cases {
            let mut task = fixture_machine();
            let result = task.corroborate_agent_catalog(&case.catalog);
            match case.expected.as_str() {
                "accepted" => {
                    assert!(result.unwrap_or_else(|error| {
                        panic!("fixture {} unexpectedly failed: {error}", case.name)
                    }));
                    assert!(case.expected_error.is_none(), "{}", case.name);
                }
                "missing" => {
                    assert!(!result.unwrap_or_else(|error| {
                        panic!("fixture {} unexpectedly failed: {error}", case.name)
                    }));
                    assert!(case.expected_error.is_none(), "{}", case.name);
                }
                "error" => {
                    let error =
                        result.expect_err(&format!("fixture {} unexpectedly succeeded", case.name));
                    assert_eq!(
                        routing_error_code(error),
                        case.expected_error,
                        "{}",
                        case.name
                    );
                }
                other => panic!("fixture {} has unknown expectation {other}", case.name),
            }
        }

        for case in fixture.active_identity_cases {
            let mut task = fixture_machine();
            assert!(
                task.corroborate_agent_catalog(&[model("alpha/code", "alpha")])
                    .expect("fixture setup catalog")
            );
            let result = task.corroborate_active_model(&case.active);
            match case.expected.as_str() {
                "accepted" => {
                    result.unwrap_or_else(|error| {
                        panic!("fixture {} unexpectedly failed: {error}", case.name)
                    });
                    assert!(case.expected_error.is_none(), "{}", case.name);
                }
                "error" => {
                    let error =
                        result.expect_err(&format!("fixture {} unexpectedly succeeded", case.name));
                    assert_eq!(
                        routing_error_code(error),
                        case.expected_error,
                        "{}",
                        case.name
                    );
                }
                other => panic!("fixture {} has unknown expectation {other}", case.name),
            }
        }

        for case in fixture.progress_cases {
            let mut task = fixture_machine();
            assert!(
                task.corroborate_agent_catalog(&[model("alpha/code", "alpha")])
                    .expect("fixture setup catalog")
            );
            task.corroborate_active_model(&model("alpha/code", "alpha"))
                .expect("fixture setup active identity");
            let observer = task.begin_inference().expect("fixture setup inference");
            if let Some(event) = fixture_event(&case.event) {
                observer.observe(&event);
            }
            let failure = match case.failure.as_str() {
                "model_unavailable" => RouteFailureClass::ModelUnavailable,
                "unknown" => RouteFailureClass::Unknown,
                other => panic!("fixture {} has unknown failure {other}", case.name),
            };
            let decision = task.decide_failure(failure).unwrap_or_else(|error| {
                panic!("fixture {} unexpectedly failed: {error}", case.name)
            });
            let expected = match case.expected.as_str() {
                "retry" => BackendTaskFailureDecision::RetryConfiguredFallback,
                "stop_observable_progress" => BackendTaskFailureDecision::Stop(
                    RouteStopReason::ObservableProgressAlreadyStarted,
                ),
                "stop_not_safe" => {
                    BackendTaskFailureDecision::Stop(RouteStopReason::FailureNotSafeForFallback)
                }
                other => panic!("fixture {} has unknown expectation {other}", case.name),
            };
            assert_eq!(decision, expected, "{}", case.name);
        }

        for case in fixture.completion_cases {
            let mut task = fixture_machine();
            assert!(
                task.corroborate_agent_catalog(&[model("alpha/code", "alpha")])
                    .expect("fixture setup catalog")
            );
            task.corroborate_active_model(&model("alpha/code", "alpha"))
                .expect("fixture setup active identity");
            task.begin_inference().expect("fixture setup inference");
            let result = task.complete(TurnResult {
                text: "fixture text".into(),
                cancelled: case.cancelled,
                tool_calls: Vec::new(),
                usage: None,
            });
            match case.expected.as_str() {
                "success" => {
                    let outcome = result.unwrap_or_else(|error| {
                        panic!("fixture {} unexpectedly failed: {error}", case.name)
                    });
                    assert_eq!(outcome.model().id, "alpha/code", "{}", case.name);
                }
                "cancelled" => {
                    assert!(
                        matches!(result, Err(VibeCoderError::Cancelled)),
                        "{}",
                        case.name
                    );
                }
                other => panic!("fixture {} has unknown expectation {other}", case.name),
            }
        }
    }

    fn fixture_machine() -> BackendTaskStateMachine {
        let policy = ModelRoutePolicyConfig {
            primary: ModelRouteTargetConfig {
                model_id: "alpha/code".into(),
                provider: Some("alpha".into()),
            },
            fallbacks: vec![ModelRouteTargetConfig {
                model_id: "beta/code".into(),
                provider: Some("beta".into()),
            }],
            fallback_on: vec![FallbackTrigger::ModelUnavailable],
            fallback_boundary: FallbackBoundary::BeforeResponseOnly,
        };
        BackendTaskStateMachine::prepare(
            ProjectId::new(),
            SessionId::parse("part24-session").expect("fixture session"),
            "fixture prompt",
            ResolvedModelRoutePolicy::resolve(
                &policy,
                &[model("alpha/code", "alpha"), model("beta/code", "beta")],
            )
            .expect("fixture route"),
        )
        .expect("fixture task")
    }

    fn fixture_event(kind: &str) -> Option<AgentEvent> {
        match kind {
            "none" => None,
            "message_accepted" => Some(AgentEvent::MessageAccepted),
            "warning" => Some(AgentEvent::Warning {
                message: "fixture warning".into(),
            }),
            "text_delta" => Some(AgentEvent::TextDelta {
                text: "fixture delta".into(),
            }),
            "background_progress" => Some(AgentEvent::BackgroundProgress {
                task_id: "fixture-task".into(),
                label: "fixture".into(),
                percent: Some(50.0),
                summary: "fixture progress".into(),
                done: false,
            }),
            "tool_started" => Some(AgentEvent::ToolStarted {
                tool: "edit".into(),
                call_id: "fixture-call".into(),
            }),
            "tool_finished" => Some(AgentEvent::ToolFinished {
                tool: "edit".into(),
                call_id: "fixture-call".into(),
                ok: true,
                output: "fixture output".into(),
                error: None,
            }),
            other => panic!("unknown Part 24 event fixture {other}"),
        }
    }

    fn routing_error_code(error: VibeCoderError) -> Option<String> {
        match error {
            VibeCoderError::Routing(code) => Some(code),
            other => panic!("expected routing error, got {other}"),
        }
    }
}
