//! Part 24 data-driven integration contracts for the Part 23 backend-task boundary.
//!
//! These tests intentionally use provider-neutral fakes. They execute the real Core and task state
//! machine at the first full compile in Part 25; Part 24 validates their source and fixture wiring.

use async_trait::async_trait;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use vibecoder_agent_contract::{AgentRuntime, CreateSessionOptions, EventHandler, RunTurnOptions};
use vibecoder_command_policy::{
    CommandApprovalDecision, CommandApprovalRequest, CommandDecisionOutcome, CommandPolicyConfig,
    CommandProgram, CommandRequestOutcome, CommandSpec,
};
use vibecoder_core::VibeCoderCore;
use vibecoder_domain::{
    AgentEvent, ModelRef, PermissionDecision, ProjectId, ProjectRef, Result, RuntimeCapabilities,
    SessionId, TurnResult, VibeCoderError,
};
use vibecoder_gateway_contract::{
    GatewayCredential, GatewayExecutionProfile, GatewayHealth, GatewayHealthStatus, ModelGateway,
};
use vibecoder_process_contract::{
    ProcessExecutionOptions, ProcessId, ProcessRuntime, ProcessRuntimeCapabilities, RunningProcess,
};
use vibecoder_routing::ModelRoutePolicyConfig;
use vibecoder_workspace_contract::{
    ProjectFileList, ProjectTextSearchResult, TextEditResult, TextPatchHunk, TextPatchResult,
    WorkspaceCapabilities, WorkspaceRuntime, WorkspaceSpec,
};

const FIXTURE_PROMPT: &str = "Part 24 fixture prompt";

#[derive(Debug, Deserialize)]
struct BackendTaskFixture {
    schema: u8,
    profiles: HashMap<String, GatewayExecutionProfile>,
    cases: Vec<BackendTaskCase>,
}

#[derive(Clone, Debug, Deserialize)]
struct BackendTaskCase {
    name: String,
    profile: String,
    gateway_catalog: Vec<ModelRef>,
    policy: ModelRoutePolicyConfig,
    agent_catalogs: Vec<Vec<ModelRef>>,
    active_models: Vec<ModelRef>,
    events: Vec<String>,
    turn: FixtureTurn,
    active_processes: usize,
    expected: ExpectedOutcome,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FixtureTurn {
    Success { text: String },
    CancelledResult,
    AgentError { code: String },
    CancelledError,
}

#[derive(Clone, Debug, Deserialize)]
struct ExpectedOutcome {
    kind: String,
    attempt_index: Option<usize>,
    model_id: Option<String>,
    error_variant: Option<String>,
    error_code: Option<String>,
    profile_calls: usize,
    gateway_catalog_calls: usize,
    agent_catalog_calls: usize,
    active_identity_calls: usize,
    run_turn_calls: usize,
    forwarded_events: usize,
}

#[derive(Default)]
struct GatewayCalls {
    profile: AtomicUsize,
    catalog: AtomicUsize,
}

#[derive(Default)]
struct AgentCalls {
    catalog: AtomicUsize,
    active_identity: AtomicUsize,
    run_turn: AtomicUsize,
    run_models: Mutex<Vec<ModelRef>>,
}

#[derive(Default)]
struct ProcessCalls {
    start: AtomicUsize,
}

#[derive(Clone)]
struct FakeGateway {
    profile: GatewayExecutionProfile,
    catalog: Vec<ModelRef>,
    calls: Arc<GatewayCalls>,
}

#[async_trait]
impl ModelGateway for FakeGateway {
    async fn execution_profile(
        &self,
        _credential: GatewayCredential<'_>,
    ) -> Result<GatewayExecutionProfile> {
        self.calls.profile.fetch_add(1, Ordering::SeqCst);
        Ok(self.profile.clone())
    }

    async fn health(&self, _credential: GatewayCredential<'_>) -> Result<GatewayHealth> {
        Ok(GatewayHealth {
            ready: true,
            status: GatewayHealthStatus::Ready,
            usable_models: self.catalog.len(),
            detail: None,
        })
    }

    async fn list_models(&self, _credential: GatewayCredential<'_>) -> Result<Vec<ModelRef>> {
        self.calls.catalog.fetch_add(1, Ordering::SeqCst);
        Ok(self.catalog.clone())
    }
}

struct FakeAgent {
    project: ProjectRef,
    session_id: SessionId,
    catalogs: Mutex<VecDeque<Vec<ModelRef>>>,
    active_models: Mutex<VecDeque<ModelRef>>,
    events: Vec<String>,
    turn: FixtureTurn,
    calls: Arc<AgentCalls>,
}

impl FakeAgent {
    fn verify_project(&self, project: &ProjectRef) -> Result<()> {
        if project != &self.project {
            return Err(VibeCoderError::Agent("fixture_project_mismatch".into()));
        }
        Ok(())
    }

    fn verify_session(&self, session_id: &SessionId) -> Result<()> {
        if session_id != &self.session_id {
            return Err(VibeCoderError::Agent("fixture_session_mismatch".into()));
        }
        Ok(())
    }
}

#[async_trait]
impl AgentRuntime for FakeAgent {
    fn runtime_id(&self) -> &'static str {
        "part24-fixture-agent"
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        fixture_capabilities()
    }

    async fn ensure_ready(&self) -> Result<RuntimeCapabilities> {
        Ok(fixture_capabilities())
    }

    async fn create_session(
        &self,
        project: &ProjectRef,
        _options: CreateSessionOptions,
    ) -> Result<SessionId> {
        self.verify_project(project)?;
        Ok(self.session_id.clone())
    }

    async fn resume_session(&self, project: &ProjectRef, session_id: &SessionId) -> Result<()> {
        self.verify_project(project)?;
        self.verify_session(session_id)
    }

    async fn verify_session_project_binding(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        self.verify_project(project)?;
        self.verify_session(session_id)
    }

    async fn ensure_workspace_quiescent(&self, project: &ProjectRef) -> Result<()> {
        self.verify_project(project)
    }

    async fn refresh_session_after_workspace_replacement(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        self.verify_project(project)?;
        self.verify_session(session_id)
    }

    async fn run_turn(
        &self,
        session_id: &SessionId,
        prompt: &str,
        options: RunTurnOptions,
        mut on_event: Option<EventHandler>,
    ) -> Result<TurnResult> {
        self.verify_session(session_id)?;
        if prompt != FIXTURE_PROMPT {
            return Err(VibeCoderError::Agent("fixture_prompt_mismatch".into()));
        }
        let model = options
            .model
            .ok_or_else(|| VibeCoderError::Agent("fixture_model_missing".into()))?;
        self.calls.run_turn.fetch_add(1, Ordering::SeqCst);
        self.calls
            .run_models
            .lock()
            .map_err(|_| VibeCoderError::Agent("fixture_call_log_poisoned".into()))?
            .push(model);
        if let Some(handler) = on_event.as_mut() {
            for kind in &self.events {
                handler(fixture_agent_event(kind)?);
            }
        }
        match &self.turn {
            FixtureTurn::Success { text } => Ok(TurnResult {
                text: text.clone(),
                cancelled: false,
                tool_calls: Vec::new(),
                usage: None,
            }),
            FixtureTurn::CancelledResult => Ok(TurnResult {
                text: String::new(),
                cancelled: true,
                tool_calls: Vec::new(),
                usage: None,
            }),
            FixtureTurn::AgentError { code } => Err(VibeCoderError::Agent(code.clone())),
            FixtureTurn::CancelledError => Err(VibeCoderError::Cancelled),
        }
    }

    async fn cancel(&self, session_id: &SessionId) -> Result<()> {
        self.verify_session(session_id)
    }

    async fn respond_to_permission(
        &self,
        session_id: &SessionId,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<()> {
        self.verify_session(session_id)
    }

    async fn list_models(&self, session_id: &SessionId) -> Result<Vec<ModelRef>> {
        self.verify_session(session_id)?;
        self.calls.catalog.fetch_add(1, Ordering::SeqCst);
        let mut catalogs = self
            .catalogs
            .lock()
            .map_err(|_| VibeCoderError::Agent("fixture_catalog_poisoned".into()))?;
        if catalogs.len() > 1 {
            return catalogs
                .pop_front()
                .ok_or_else(|| VibeCoderError::Agent("fixture_catalog_missing".into()));
        }
        catalogs
            .front()
            .cloned()
            .ok_or_else(|| VibeCoderError::Agent("fixture_catalog_missing".into()))
    }

    async fn corroborate_model_identity(
        &self,
        session_id: &SessionId,
        _model: &ModelRef,
    ) -> Result<ModelRef> {
        self.verify_session(session_id)?;
        self.calls.active_identity.fetch_add(1, Ordering::SeqCst);
        let mut models = self
            .active_models
            .lock()
            .map_err(|_| VibeCoderError::Agent("fixture_active_model_poisoned".into()))?;
        if models.len() > 1 {
            return models
                .pop_front()
                .ok_or_else(|| VibeCoderError::Agent("fixture_active_model_missing".into()));
        }
        models
            .front()
            .cloned()
            .ok_or_else(|| VibeCoderError::Agent("fixture_active_model_missing".into()))
    }

    async fn set_model(&self, session_id: &SessionId, _model: &ModelRef) -> Result<()> {
        self.verify_session(session_id)
    }
}

#[derive(Clone)]
struct FakeWorkspace {
    project: ProjectRef,
}

impl FakeWorkspace {
    fn verify(&self, project: &ProjectRef) -> Result<()> {
        if project != &self.project {
            return Err(VibeCoderError::Workspace("fixture_project_mismatch".into()));
        }
        Ok(())
    }
}

#[async_trait]
impl WorkspaceRuntime for FakeWorkspace {
    fn capabilities(&self) -> WorkspaceCapabilities {
        WorkspaceCapabilities {
            read_write_files: true,
            managed_project_roots: true,
            canonical_path_containment: true,
            text_edit: true,
            project_search: true,
            commands: false,
            process_isolation: false,
            resource_limits: true,
            snapshots: true,
            max_file_read_bytes: 1024 * 1024,
            max_file_write_bytes: 1024 * 1024,
        }
    }

    async fn create_project(&self, _spec: WorkspaceSpec) -> Result<ProjectRef> {
        Ok(self.project.clone())
    }

    async fn open_project(&self, id: ProjectId) -> Result<ProjectRef> {
        if id != self.project.id {
            return Err(VibeCoderError::Workspace("fixture_project_mismatch".into()));
        }
        Ok(self.project.clone())
    }

    async fn remove_project(&self, project: &ProjectRef) -> Result<()> {
        self.verify(project)
    }

    async fn verify_project(&self, project: &ProjectRef) -> Result<()> {
        self.verify(project)
    }

    async fn resolve_project_path(&self, project: &ProjectRef, relative: &Path) -> Result<PathBuf> {
        self.verify(project)?;
        Ok(project.root.join(relative))
    }

    async fn create_dir_all(&self, project: &ProjectRef, _relative: &Path) -> Result<()> {
        self.verify(project)
    }

    async fn read_file(
        &self,
        project: &ProjectRef,
        _relative: &Path,
        _max_bytes: usize,
    ) -> Result<Vec<u8>> {
        self.verify(project)?;
        Ok(Vec::new())
    }

    async fn regular_file_exists(&self, project: &ProjectRef, _relative: &Path) -> Result<bool> {
        self.verify(project)?;
        Ok(false)
    }

    async fn atomic_write_file(
        &self,
        project: &ProjectRef,
        _relative: &Path,
        _contents: &[u8],
    ) -> Result<()> {
        self.verify(project)
    }

    async fn edit_text_file(
        &self,
        project: &ProjectRef,
        _relative: &Path,
        _expected: &str,
        _replacement: &str,
    ) -> Result<TextEditResult> {
        self.verify(project)?;
        Ok(TextEditResult {
            replacements: 1,
            bytes_before: 0,
            bytes_after: 0,
        })
    }

    async fn apply_text_patch(
        &self,
        project: &ProjectRef,
        _relative: &Path,
        hunks: &[TextPatchHunk],
    ) -> Result<TextPatchResult> {
        self.verify(project)?;
        Ok(TextPatchResult {
            hunks_applied: hunks.len() as u32,
            bytes_before: 0,
            bytes_after: 0,
        })
    }

    async fn list_project_files(
        &self,
        project: &ProjectRef,
        _max_entries: usize,
    ) -> Result<ProjectFileList> {
        self.verify(project)?;
        Ok(ProjectFileList {
            files: Vec::new(),
            skipped_entries: 0,
            truncated: false,
        })
    }

    async fn search_project_text(
        &self,
        project: &ProjectRef,
        _needle: &str,
        _max_matches: usize,
    ) -> Result<ProjectTextSearchResult> {
        self.verify(project)?;
        Ok(ProjectTextSearchResult {
            matches: Vec::new(),
            files_scanned: 0,
            files_skipped: 0,
            bytes_scanned: 0,
            truncated: false,
        })
    }
}

#[derive(Clone)]
struct FakeProcessRuntime {
    active_processes: usize,
    calls: Arc<ProcessCalls>,
}

impl ProcessRuntime for FakeProcessRuntime {
    fn capabilities(&self) -> ProcessRuntimeCapabilities {
        ProcessRuntimeCapabilities {
            local_execution: true,
            cancellation: true,
            timeout: true,
            bounded_output_capture: true,
            process_group_termination: true,
            strong_process_isolation: false,
        }
    }

    fn start(
        &self,
        _project: &ProjectRef,
        _envelope: vibecoder_command_policy::CommandExecutionEnvelope,
        _options: ProcessExecutionOptions,
    ) -> Result<RunningProcess> {
        self.calls.start.fetch_add(1, Ordering::SeqCst);
        Err(VibeCoderError::Process(
            "fixture_process_start_forbidden".into(),
        ))
    }

    fn active_for_project(&self, _project_id: ProjectId) -> Result<usize> {
        Ok(self.active_processes)
    }

    fn cancel(&self, _process_id: ProcessId) -> Result<()> {
        Ok(())
    }
}

type TestCore = VibeCoderCore<FakeAgent, FakeGateway, FakeWorkspace>;

struct BuiltCore {
    core: TestCore,
    project: ProjectRef,
    session_id: SessionId,
    gateway_calls: Arc<GatewayCalls>,
    agent_calls: Arc<AgentCalls>,
    process_calls: Arc<ProcessCalls>,
}

#[test]
fn part24_backend_task_fixtures_cover_terminal_paths() {
    let fixture = load_fixture();
    assert_eq!(fixture.schema, 1);
    for case in &fixture.cases {
        let built = build_core(&fixture, case, CommandPolicyConfig::deny_all());
        let forwarded = Arc::new(AtomicUsize::new(0));
        let forwarded_for_handler = Arc::clone(&forwarded);
        let result = block_on(built.core.run_backend_task(
            &built.project,
            &built.session_id,
            GatewayCredential::Anonymous,
            &case.policy,
            FIXTURE_PROMPT,
            Some(Box::new(move |_event| {
                forwarded_for_handler.fetch_add(1, Ordering::SeqCst);
            })),
        ));

        match case.expected.kind.as_str() {
            "success" => {
                let outcome = result.unwrap_or_else(|error| {
                    panic!("fixture {} unexpectedly failed: {error}", case.name)
                });
                assert_eq!(
                    Some(outcome.attempt_index()),
                    case.expected.attempt_index,
                    "{}",
                    case.name
                );
                assert_eq!(
                    Some(outcome.model().id.as_str()),
                    case.expected.model_id.as_deref(),
                    "{}",
                    case.name
                );
                assert!(case.expected.error_variant.is_none(), "{}", case.name);
                assert!(case.expected.error_code.is_none(), "{}", case.name);
            }
            "error" => {
                let error =
                    result.expect_err(&format!("fixture {} unexpectedly succeeded", case.name));
                let (variant, code) = error_identity(error);
                assert_eq!(
                    Some(variant.as_str()),
                    case.expected.error_variant.as_deref(),
                    "{}",
                    case.name
                );
                assert_eq!(
                    code.as_deref(),
                    case.expected.error_code.as_deref(),
                    "{}",
                    case.name
                );
            }
            other => panic!("fixture {} has unknown expected kind {other}", case.name),
        }

        assert_eq!(
            built.gateway_calls.profile.load(Ordering::SeqCst),
            case.expected.profile_calls,
            "{}",
            case.name
        );
        assert_eq!(
            built.gateway_calls.catalog.load(Ordering::SeqCst),
            case.expected.gateway_catalog_calls,
            "{}",
            case.name
        );
        assert_eq!(
            built.agent_calls.catalog.load(Ordering::SeqCst),
            case.expected.agent_catalog_calls,
            "{}",
            case.name
        );
        assert_eq!(
            built.agent_calls.active_identity.load(Ordering::SeqCst),
            case.expected.active_identity_calls,
            "{}",
            case.name
        );
        assert_eq!(
            built.agent_calls.run_turn.load(Ordering::SeqCst),
            case.expected.run_turn_calls,
            "{}",
            case.name
        );
        let run_models = built
            .agent_calls
            .run_models
            .lock()
            .expect("fixture run-model log");
        assert_eq!(
            run_models.len(),
            case.expected.run_turn_calls,
            "{}",
            case.name
        );
        if case.expected.run_turn_calls == 1 {
            assert_eq!(
                run_models.first(),
                case.active_models.last(),
                "{}",
                case.name
            );
        }
        assert_eq!(
            forwarded.load(Ordering::SeqCst),
            case.expected.forwarded_events,
            "{}",
            case.name
        );
        assert_eq!(
            built.process_calls.start.load(Ordering::SeqCst),
            0,
            "{}",
            case.name
        );
    }
}

#[test]
fn part24_backend_task_invalidates_authority_before_and_after_turn() {
    let fixture = load_fixture();
    let case = fixture
        .cases
        .iter()
        .find(|case| case.name == "exact_primary_success")
        .expect("success fixture");
    let policy = CommandPolicyConfig::new(["node"], false).expect("command policy");
    let built = build_core(&fixture, case, policy);
    let project = built.project.clone();
    let session_id = built.session_id.clone();
    let process_calls = Arc::clone(&built.process_calls);
    let core = Arc::new(built.core);

    let approval = approval_required(
        block_on(core.request_project_command(&project, &session_id, runtime_command()))
            .expect("pre-turn command request"),
    );
    let stale_envelope = authorized(
        block_on(core.decide_project_command(
            &project,
            &session_id,
            &approval,
            CommandApprovalDecision::AllowOnce,
        ))
        .expect("pre-turn command approval"),
    );

    let minted_during_turn = Arc::new(Mutex::new(None::<CommandApprovalRequest>));
    let minted_for_handler = Arc::clone(&minted_during_turn);
    let handler_core = Arc::clone(&core);
    let handler_project = project.clone();
    let handler_session = session_id.clone();
    let minted_once = Arc::new(AtomicBool::new(false));
    let minted_once_for_handler = Arc::clone(&minted_once);
    let on_event: EventHandler = Box::new(move |_event| {
        if minted_once_for_handler.swap(true, Ordering::SeqCst) {
            return;
        }
        let request_core = Arc::clone(&handler_core);
        let request_project = handler_project.clone();
        let request_session = handler_session.clone();
        let request = std::thread::spawn(move || {
            approval_required(
                block_on(request_core.request_project_command(
                    &request_project,
                    &request_session,
                    runtime_command(),
                ))
                .expect("during-turn command request"),
            )
        })
        .join()
        .expect("during-turn command request thread");
        *minted_for_handler
            .lock()
            .expect("during-turn approval slot") = Some(request);
    });

    block_on(core.run_backend_task(
        &project,
        &session_id,
        GatewayCredential::Anonymous,
        &case.policy,
        FIXTURE_PROMPT,
        Some(on_event),
    ))
    .expect("fixture backend task");

    let stale_error = block_on(core.start_authorized_project_command(
        &project,
        &session_id,
        stale_envelope,
        ProcessExecutionOptions::default(),
    ))
    .expect_err("pre-turn envelope must be stale");
    assert_eq!(
        error_identity(stale_error),
        (
            String::from("command"),
            Some(String::from(
                "command_execution_envelope_stale_project_epoch"
            ))
        )
    );

    let during_turn_approval = minted_during_turn
        .lock()
        .expect("during-turn approval slot")
        .take()
        .expect("an event must mint one during-turn approval");
    let removed_error = block_on(core.decide_project_command(
        &project,
        &session_id,
        &during_turn_approval,
        CommandApprovalDecision::AllowOnce,
    ))
    .expect_err("during-turn approval must be invalidated after the turn");
    assert_eq!(
        error_identity(removed_error),
        (
            String::from("command"),
            Some(String::from("command_request_not_pending"))
        )
    );
    assert_eq!(process_calls.start.load(Ordering::SeqCst), 0);
}

fn load_fixture() -> BackendTaskFixture {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/part24/backend_task_contracts.json"
    ))
    .expect("Part 24 backend-task fixture must parse")
}

fn build_core(
    fixture: &BackendTaskFixture,
    case: &BackendTaskCase,
    command_policy: CommandPolicyConfig,
) -> BuiltCore {
    let project = ProjectRef {
        id: ProjectId::new(),
        root: PathBuf::from("/part24/fixture/project"),
    };
    let session_id = SessionId::parse("part24-fixture-session").expect("fixture session");
    let gateway_calls = Arc::new(GatewayCalls::default());
    let agent_calls = Arc::new(AgentCalls::default());
    let process_calls = Arc::new(ProcessCalls::default());
    let gateway = FakeGateway {
        profile: fixture
            .profiles
            .get(&case.profile)
            .unwrap_or_else(|| panic!("fixture {} names an unknown profile", case.name))
            .clone(),
        catalog: case.gateway_catalog.clone(),
        calls: Arc::clone(&gateway_calls),
    };
    let agent = FakeAgent {
        project: project.clone(),
        session_id: session_id.clone(),
        catalogs: Mutex::new(case.agent_catalogs.clone().into()),
        active_models: Mutex::new(case.active_models.clone().into()),
        events: case.events.clone(),
        turn: case.turn.clone(),
        calls: Arc::clone(&agent_calls),
    };
    let workspace = FakeWorkspace {
        project: project.clone(),
    };
    let process = FakeProcessRuntime {
        active_processes: case.active_processes,
        calls: Arc::clone(&process_calls),
    };
    let core = VibeCoderCore::new_with_command_policy(agent, gateway, workspace, command_policy)
        .with_process_runtime(Arc::new(process));
    BuiltCore {
        core,
        project,
        session_id,
        gateway_calls,
        agent_calls,
        process_calls,
    }
}

fn fixture_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        sessions: true,
        streaming_events: true,
        permissions: true,
        model_selection: true,
        file_tools: true,
        command_tools: true,
    }
}

fn fixture_agent_event(kind: &str) -> Result<AgentEvent> {
    match kind {
        "message_accepted" => Ok(AgentEvent::MessageAccepted),
        "text_delta" => Ok(AgentEvent::TextDelta {
            text: "fixture delta".into(),
        }),
        "turn_completed" => Ok(AgentEvent::TurnCompleted),
        other => Err(VibeCoderError::Agent(format!(
            "unknown_fixture_event:{other}"
        ))),
    }
}

fn runtime_command() -> CommandSpec {
    CommandSpec {
        program: CommandProgram::RuntimeTool {
            tool_id: "node".into(),
        },
        args: vec!["--version".into()],
        working_dir: PathBuf::new(),
    }
}

fn approval_required(outcome: CommandRequestOutcome) -> CommandApprovalRequest {
    match outcome {
        CommandRequestOutcome::ApprovalRequired(request) => request,
        CommandRequestOutcome::Denied(denied) => {
            panic!("fixture command unexpectedly denied: {}", denied.code)
        }
    }
}

fn authorized(
    outcome: CommandDecisionOutcome,
) -> vibecoder_command_policy::CommandExecutionEnvelope {
    match outcome {
        CommandDecisionOutcome::Authorized(envelope) => envelope,
        CommandDecisionOutcome::Denied(denied) => {
            panic!("fixture command unexpectedly denied: {}", denied.code)
        }
    }
}

fn error_identity(error: VibeCoderError) -> (String, Option<String>) {
    match error {
        VibeCoderError::InvalidRequest(code) => ("invalid_request".into(), Some(code)),
        VibeCoderError::Agent(code) => ("agent".into(), Some(code)),
        VibeCoderError::Gateway(code) => ("gateway".into(), Some(code)),
        VibeCoderError::Routing(code) => ("routing".into(), Some(code)),
        VibeCoderError::Config(code) => ("config".into(), Some(code)),
        VibeCoderError::Secret(code) => ("secret".into(), Some(code)),
        VibeCoderError::Workspace(code) => ("workspace".into(), Some(code)),
        VibeCoderError::Command(code) => ("command".into(), Some(code)),
        VibeCoderError::Process(code) => ("process".into(), Some(code)),
        VibeCoderError::Persistence(code) => ("persistence".into(), Some(code)),
        VibeCoderError::Checkpoint(code) => ("checkpoint".into(), Some(code)),
        VibeCoderError::Build(code) => ("build".into(), Some(code)),
        VibeCoderError::MissingCapability {
            component,
            capability,
        } => (
            "missing_capability".into(),
            Some(format!("{component}:{capability}")),
        ),
        VibeCoderError::Cancelled => ("cancelled".into(), None),
    }
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
