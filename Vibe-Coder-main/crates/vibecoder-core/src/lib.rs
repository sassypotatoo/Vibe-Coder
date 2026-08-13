//! Application orchestration layer. Provider-specific adapters stay outside this crate.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use vibecoder_agent_contract::{
    AgentRuntime, CreateSessionOptions, EventHandler, ModelGatewayBridgeIdentity, RunTurnOptions,
};
use vibecoder_build_contract::{BuildJobDescriptor, BuildResult, BuildTargetKind, RunningBuildJob};
use vibecoder_build_loop::{
    BuildRepairLoopGuard, BuildRepairLoopPolicy, BuildRepairLoopStopReason, RepairAuthorization,
};
use vibecoder_build_repair::{BuildFailureEvidence, BuildRepairPlan};
use vibecoder_checkpoint_contract::{
    CheckpointCapabilities, CheckpointId, CheckpointMetadata, CheckpointReason, CheckpointStore,
    RollbackResult,
};
use vibecoder_command_policy::{
    CommandApprovalDecision, CommandApprovalRequest, CommandDecisionOutcome,
    CommandExecutionEnvelope, CommandPolicyConfig, CommandPolicyEngine, CommandRequestOutcome,
    CommandSpec,
};
use vibecoder_domain::{
    AgentEvent, ConversationId, ModelRef, ProjectId, ProjectRef, Result, RuntimeCapabilities,
    SessionId, TokenUsage, TurnResult, VibeCoderError,
};
use vibecoder_gateway_contract::{
    GatewayChatMessage, GatewayChatRequest, GatewayChatResponse, GatewayChatRole, GatewayCredential,
    GatewayExecutionProfile, GatewayHealth, ModelGateway,
};
use vibecoder_persistence_contract::{
    ConversationRole, ConversationStore, MAX_CONVERSATION_MESSAGES, MAX_CONVERSATION_MESSAGE_BYTES,
    MAX_CONVERSATION_TEXT_BYTES, MAX_PERSISTED_CONVERSATIONS_PER_PROJECT, PersistedAgentSession, PersistedConversation,
    PersistedProjectState, PersistenceCapabilities,
    ProjectStateStore,
};
use vibecoder_process_contract::{
    ProcessExecutionOptions, ProcessId, ProcessRuntime, ProcessRuntimeCapabilities, RunningProcess,
};
use vibecoder_routing::{
    ModelRoutePolicyConfig, ModelRouteTargetConfig, ResolvedModelRoutePolicy, RouteFailureClass,
};
use vibecoder_secrets::{SecretReference, SecretResolver, SecretValue};
use vibecoder_task_orchestration::{
    BackendTaskFailureDecision, BackendTaskOutcome, BackendTaskStateMachine, classify_agent_failure,
};
use vibecoder_web_build_pipeline::{
    RunningWebsiteBuildStage, WebsiteBuildPipeline, WebsiteBuildPipelineState, WebsiteBuildPolicy,
    WebsiteBuildStageCompletion,
};
use vibecoder_web_toolchain::{WebsiteToolchainReport, inspect_website_project};
use vibecoder_workspace_contract::{
    ProjectFileList, ProjectTextSearchResult, TextEditResult, TextPatchHunk, TextPatchResult,
    WorkspaceRuntime, WorkspaceSpec,
};

const MAX_CONVERSATION_MODEL_MESSAGES: usize = 64;
const MAX_CONVERSATION_MODEL_MESSAGE_BYTES: usize = 128 * 1024;
const MAX_CONVERSATION_MODEL_CONTEXT_BYTES: usize = 256 * 1024;
const MAX_CONVERSATION_MODEL_OUTPUT_TOKENS: u32 = 8192;
const MAX_CONVERSATION_MODEL_ID_BYTES: usize = 512;
const MAX_AGENT_ACTION_TOOL_CALLS: usize = 32;
const MAX_AGENT_ACTION_CALL_ID_BYTES: usize = 256;
const AGENT_ACTION_FILE_TOOLS: &[&str] = &[
    "read",
    "write",
    "edit",
    "multiedit",
    "apply_patch",
    "patch",
    "agentgrep",
    "ls",
];
const AGENT_ACTION_MUTATION_TOOLS: &[&str] =
    &["write", "edit", "multiedit", "apply_patch", "patch"];

const DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TURNS: u8 = 4;
const MAX_EXPLICIT_AGENT_LOOP_TURNS: u8 = 8;
const DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TOTAL_TOOL_CALLS: usize = 96;
const MAX_EXPLICIT_AGENT_LOOP_TOTAL_TOOL_CALLS: usize = 256;
const DEFAULT_EXPLICIT_AGENT_LOOP_MAX_SAME_WORKSPACE_OCCURRENCES: u8 = 2;
const MAX_EXPLICIT_AGENT_LOOP_SAME_WORKSPACE_OCCURRENCES: u8 = 3;
const EXPLICIT_AGENT_LOOP_COMPLETE_MARKER: &str = "VIBECODER_LOOP_STATUS=complete";
const EXPLICIT_AGENT_LOOP_CONTINUE_MARKER: &str = "VIBECODER_LOOP_STATUS=continue";

#[derive(Clone)]
pub struct ConversationModelTurnCancellation {
    requested: Arc<AtomicBool>,
}

impl std::fmt::Debug for ConversationModelTurnCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConversationModelTurnCancellation")
            .field("requested", &self.is_requested())
            .finish()
    }
}

impl ConversationModelTurnCancellation {
    pub fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

impl Default for ConversationModelTurnCancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplicitAgentLoopPolicy {
    pub max_turns: u8,
    pub max_total_tool_calls: usize,
    pub max_same_workspace_occurrences: u8,
}

impl Default for ExplicitAgentLoopPolicy {
    fn default() -> Self {
        Self {
            max_turns: DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TURNS,
            max_total_tool_calls: DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TOTAL_TOOL_CALLS,
            max_same_workspace_occurrences:
                DEFAULT_EXPLICIT_AGENT_LOOP_MAX_SAME_WORKSPACE_OCCURRENCES,
        }
    }
}

impl ExplicitAgentLoopPolicy {
    pub fn validate(self) -> Result<Self> {
        if self.max_turns == 0 || self.max_turns > MAX_EXPLICIT_AGENT_LOOP_TURNS {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_turn_budget_invalid".into(),
            ));
        }
        if self.max_total_tool_calls == 0
            || self.max_total_tool_calls > MAX_EXPLICIT_AGENT_LOOP_TOTAL_TOOL_CALLS
        {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_tool_budget_invalid".into(),
            ));
        }
        if self.max_same_workspace_occurrences < 2
            || self.max_same_workspace_occurrences
                > MAX_EXPLICIT_AGENT_LOOP_SAME_WORKSPACE_OCCURRENCES
        {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_repeat_limit_invalid".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct ExplicitAgentLoopCancellation {
    requested: Arc<AtomicBool>,
}

impl std::fmt::Debug for ExplicitAgentLoopCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExplicitAgentLoopCancellation")
            .field("requested", &self.is_requested())
            .finish()
    }
}

impl ExplicitAgentLoopCancellation {
    fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

pub struct ExplicitAgentLoopGuard {
    project_id: ProjectId,
    conversation_id: ConversationId,
    policy: ExplicitAgentLoopPolicy,
    cancellation: ExplicitAgentLoopCancellation,
    started: AtomicBool,
}

impl std::fmt::Debug for ExplicitAgentLoopGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExplicitAgentLoopGuard")
            .field("project_id", &self.project_id)
            .field("conversation_id", &self.conversation_id)
            .field("policy", &self.policy)
            .field("cancel_requested", &self.cancellation.is_requested())
            .field("started", &self.started.load(Ordering::Acquire))
            .finish()
    }
}

impl ExplicitAgentLoopGuard {
    fn new(
        project_id: ProjectId,
        conversation_id: ConversationId,
        policy: ExplicitAgentLoopPolicy,
    ) -> Result<Self> {
        Ok(Self {
            project_id,
            conversation_id,
            policy: policy.validate()?,
            cancellation: ExplicitAgentLoopCancellation::new(),
            started: AtomicBool::new(false),
        })
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn conversation_id(&self) -> ConversationId {
        self.conversation_id
    }

    pub const fn policy(&self) -> ExplicitAgentLoopPolicy {
        self.policy
    }

    pub fn cancellation(&self) -> ExplicitAgentLoopCancellation {
        self.cancellation.clone()
    }

    fn begin_once(&self) -> Result<()> {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                VibeCoderError::InvalidRequest("explicit_agent_loop_guard_already_used".into())
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitAgentLoopStopReason {
    Completed,
    Cancelled,
    TurnBudgetExhausted,
    ToolBudgetExhausted,
    RepeatedWorkspaceState,
}

pub struct PersistedExplicitAgentLoopOutcome {
    stop_reason: ExplicitAgentLoopStopReason,
    turns_completed: u8,
    total_tool_calls: usize,
    total_successful_mutations: usize,
    baseline_tree_sha256: String,
    final_tree_sha256: String,
    workspace_committed: bool,
    rollback_performed: bool,
    checkpoint_cleanup_complete: bool,
    assistant_text: String,
}

impl std::fmt::Debug for PersistedExplicitAgentLoopOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersistedExplicitAgentLoopOutcome")
            .field("stop_reason", &self.stop_reason)
            .field("turns_completed", &self.turns_completed)
            .field("total_tool_calls", &self.total_tool_calls)
            .field("total_successful_mutations", &self.total_successful_mutations)
            .field("workspace_committed", &self.workspace_committed)
            .field("rollback_performed", &self.rollback_performed)
            .field("checkpoint_cleanup_complete", &self.checkpoint_cleanup_complete)
            .field(
                "assistant_text",
                &format_args!("[REDACTED; {} byte(s)]", self.assistant_text.len()),
            )
            .finish()
    }
}

impl PersistedExplicitAgentLoopOutcome {
    pub const fn stop_reason(&self) -> ExplicitAgentLoopStopReason {
        self.stop_reason
    }
    pub const fn turns_completed(&self) -> u8 {
        self.turns_completed
    }
    pub const fn total_tool_calls(&self) -> usize {
        self.total_tool_calls
    }
    pub const fn total_successful_mutations(&self) -> usize {
        self.total_successful_mutations
    }
    pub fn baseline_tree_sha256(&self) -> &str {
        &self.baseline_tree_sha256
    }
    pub fn final_tree_sha256(&self) -> &str {
        &self.final_tree_sha256
    }
    pub const fn workspace_committed(&self) -> bool {
        self.workspace_committed
    }
    pub const fn rollback_performed(&self) -> bool {
        self.rollback_performed
    }
    pub const fn checkpoint_cleanup_complete(&self) -> bool {
        self.checkpoint_cleanup_complete
    }
    pub fn assistant_text(&self) -> &str {
        &self.assistant_text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplicitAgentLoopDecision {
    Complete,
    Continue,
}

pub struct ConversationModelTurnOutcome {
    model: ModelRef,
    observed_model_id: Option<String>,
    finish_reason: Option<String>,
    usage: Option<TokenUsage>,
    assistant_text: String,
    context_messages_sent: usize,
    context_bytes_sent: usize,
}

impl std::fmt::Debug for ConversationModelTurnOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConversationModelTurnOutcome")
            .field("model", &self.model)
            .field("observed_model_id", &self.observed_model_id)
            .field("finish_reason", &self.finish_reason)
            .field("usage", &self.usage)
            .field(
                "assistant_text",
                &format_args!("[REDACTED; {} byte(s)]", self.assistant_text.len()),
            )
            .field("context_messages_sent", &self.context_messages_sent)
            .field("context_bytes_sent", &self.context_bytes_sent)
            .finish()
    }
}

impl ConversationModelTurnOutcome {
    pub fn model(&self) -> &ModelRef {
        &self.model
    }
    pub fn observed_model_id(&self) -> Option<&str> {
        self.observed_model_id.as_deref()
    }
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }
    pub const fn usage(&self) -> Option<TokenUsage> {
        self.usage
    }
    pub fn assistant_text(&self) -> &str {
        &self.assistant_text
    }
    pub const fn context_messages_sent(&self) -> usize {
        self.context_messages_sent
    }
    pub const fn context_bytes_sent(&self) -> usize {
        self.context_bytes_sent
    }
}

pub struct PersistedAgentActionTurnOutcome {
    backend: BackendTaskOutcome,
    observed_tool_calls: usize,
    successful_file_tool_calls: usize,
    successful_mutation_tool_calls: usize,
    pre_action_tree_sha256: String,
    post_action_tree_sha256: String,
    checkpoint_cleanup_complete: bool,
}

impl std::fmt::Debug for PersistedAgentActionTurnOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersistedAgentActionTurnOutcome")
            .field("backend", &self.backend)
            .field("observed_tool_calls", &self.observed_tool_calls)
            .field("successful_file_tool_calls", &self.successful_file_tool_calls)
            .field(
                "successful_mutation_tool_calls",
                &self.successful_mutation_tool_calls,
            )
            .field("workspace_change_proven", &true)
            .field("checkpoint_cleanup_complete", &self.checkpoint_cleanup_complete)
            .finish()
    }
}

impl PersistedAgentActionTurnOutcome {
    pub fn backend(&self) -> &BackendTaskOutcome {
        &self.backend
    }

    pub const fn observed_tool_calls(&self) -> usize {
        self.observed_tool_calls
    }

    pub const fn successful_file_tool_calls(&self) -> usize {
        self.successful_file_tool_calls
    }

    pub const fn successful_mutation_tool_calls(&self) -> usize {
        self.successful_mutation_tool_calls
    }

    pub fn pre_action_tree_sha256(&self) -> &str {
        &self.pre_action_tree_sha256
    }

    pub fn post_action_tree_sha256(&self) -> &str {
        &self.post_action_tree_sha256
    }

    pub const fn workspace_change_proven(&self) -> bool {
        true
    }

    pub const fn checkpoint_cleanup_complete(&self) -> bool {
        self.checkpoint_cleanup_complete
    }

    pub fn into_backend(self) -> BackendTaskOutcome {
        self.backend
    }
}

#[derive(Default)]
struct AgentActionObservation {
    calls: HashMap<String, ObservedAgentToolCall>,
    protocol_failure: bool,
}

struct ObservedAgentToolCall {
    tool: String,
    finished: bool,
    ok: bool,
}

impl AgentActionObservation {
    fn observe(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolStarted { tool, call_id } => {
                if !agent_action_tool_identity_is_safe(tool, call_id)
                    || !AGENT_ACTION_FILE_TOOLS.contains(&tool.as_str())
                    || self.calls.len() >= MAX_AGENT_ACTION_TOOL_CALLS
                    || self
                        .calls
                        .insert(
                            call_id.clone(),
                            ObservedAgentToolCall {
                                tool: tool.clone(),
                                finished: false,
                                ok: false,
                            },
                        )
                        .is_some()
                {
                    self.protocol_failure = true;
                }
            }
            AgentEvent::ToolFinished {
                tool,
                call_id,
                ok,
                ..
            } => match self.calls.get_mut(call_id) {
                Some(call) if call.tool == *tool && !call.finished => {
                    call.finished = true;
                    call.ok = *ok;
                }
                _ => self.protocol_failure = true,
            },
            _ => {}
        }
    }
}

pub struct BuildRepairTurnOutcome {
    checkpoint: CheckpointMetadata,
    failure: BuildFailureEvidence,
    turn: TurnResult,
}

impl std::fmt::Debug for BuildRepairTurnOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BuildRepairTurnOutcome")
            .field("checkpoint", &self.checkpoint)
            .field("failure", &self.failure)
            .field(
                "turn_text",
                &format_args!("[REDACTED; {} byte(s)]", self.turn.text.len()),
            )
            .field("turn_cancelled", &self.turn.cancelled)
            .field("tool_call_count", &self.turn.tool_calls.len())
            .field("usage_present", &self.turn.usage.is_some())
            .finish()
    }
}

impl BuildRepairTurnOutcome {
    pub fn checkpoint(&self) -> &CheckpointMetadata {
        &self.checkpoint
    }
    pub fn failure(&self) -> &BuildFailureEvidence {
        &self.failure
    }
    pub fn turn(&self) -> &TurnResult {
        &self.turn
    }
    pub fn into_parts(self) -> (CheckpointMetadata, BuildFailureEvidence, TurnResult) {
        (self.checkpoint, self.failure, self.turn)
    }
}

pub struct GuardedBuildRepairTurnOutcome {
    attempt: u8,
    repair: BuildRepairTurnOutcome,
    stop_reason: Option<BuildRepairLoopStopReason>,
}

impl std::fmt::Debug for GuardedBuildRepairTurnOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GuardedBuildRepairTurnOutcome")
            .field("attempt", &self.attempt)
            .field("repair", &self.repair)
            .field("stop_reason", &self.stop_reason)
            .finish()
    }
}

impl GuardedBuildRepairTurnOutcome {
    pub const fn attempt(&self) -> u8 {
        self.attempt
    }
    pub fn repair(&self) -> &BuildRepairTurnOutcome {
        &self.repair
    }
    pub const fn stop_reason(&self) -> Option<BuildRepairLoopStopReason> {
        self.stop_reason
    }
    pub fn into_repair(self) -> BuildRepairTurnOutcome {
        self.repair
    }
}

#[allow(
    clippy::large_enum_variant,
    reason = "keep the public decision payload direct and avoid a breaking boxed API"
)]
pub enum GuardedBuildRepairDecision {
    RepairTurn(GuardedBuildRepairTurnOutcome),
    Stop(BuildRepairLoopStopReason),
}

impl std::fmt::Debug for GuardedBuildRepairDecision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepairTurn(outcome) => {
                formatter.debug_tuple("RepairTurn").field(outcome).finish()
            }
            Self::Stop(reason) => formatter.debug_tuple("Stop").field(reason).finish(),
        }
    }
}

pub struct VibeCoderCore<A, G, W> {
    agent: A,
    gateway: G,
    workspace: W,
    command_policy: CommandPolicyEngine,
    process_runtime: Option<Arc<dyn ProcessRuntime>>,
    project_state_store: Option<Arc<dyn ProjectStateStore>>,
    conversation_store: Option<Arc<dyn ConversationStore>>,
    checkpoint_store: Option<Arc<dyn CheckpointStore>>,
    project_lifecycle_gate: ProjectLifecycleGate,
}

#[derive(Default)]
struct ProjectLifecycleGate {
    active: Mutex<HashSet<ProjectId>>,
}

struct ProjectLifecyclePermit<'a> {
    gate: &'a ProjectLifecycleGate,
    project_id: ProjectId,
}

impl ProjectLifecycleGate {
    fn try_acquire(&self, project_id: ProjectId) -> Result<ProjectLifecyclePermit<'_>> {
        let mut active = self.active.lock().map_err(|_| {
            VibeCoderError::InvalidRequest("project_lifecycle_gate_poisoned".into())
        })?;
        if !active.insert(project_id) {
            return Err(VibeCoderError::InvalidRequest(
                "project_lifecycle_busy".into(),
            ));
        }
        Ok(ProjectLifecyclePermit {
            gate: self,
            project_id,
        })
    }
}

impl Drop for ProjectLifecyclePermit<'_> {
    fn drop(&mut self) {
        match self.gate.active.lock() {
            Ok(mut active) => {
                active.remove(&self.project_id);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&self.project_id);
            }
        }
    }
}

impl<A, G, W> VibeCoderCore<A, G, W>
where
    A: AgentRuntime,
    G: ModelGateway,
    W: WorkspaceRuntime,
{
    /// Backwards-compatible fail-closed constructor. Command requests are denied until the local
    /// runtime supplies an explicit policy with `new_with_command_policy`.
    pub fn new(agent: A, gateway: G, workspace: W) -> Self {
        Self::new_with_command_policy(agent, gateway, workspace, CommandPolicyConfig::deny_all())
    }

    pub fn new_with_command_policy(
        agent: A,
        gateway: G,
        workspace: W,
        command_policy: CommandPolicyConfig,
    ) -> Self {
        Self {
            agent,
            gateway,
            workspace,
            command_policy: CommandPolicyEngine::new(command_policy),
            process_runtime: None,
            project_state_store: None,
            conversation_store: None,
            checkpoint_store: None,
            project_lifecycle_gate: ProjectLifecycleGate::default(),
        }
    }

    /// Attach the phone-local process runtime after runtime provisioning has supplied its trusted
    /// tool registry. Existing constructors remain fail-closed and cannot execute commands.
    pub fn with_process_runtime(mut self, process_runtime: Arc<dyn ProcessRuntime>) -> Self {
        self.process_runtime = Some(process_runtime);
        self
    }

    /// Attach the app-private persistence store. Constructors remain usable without persistence,
    /// but persisted project/session APIs fail closed until a store is supplied.
    pub fn with_project_state_store(mut self, store: Arc<dyn ProjectStateStore>) -> Self {
        self.project_state_store = Some(store);
        self
    }

    /// Attach the independent Part-34 conversation registry. Keeping this capability separate from
    /// legacy project state preserves backwards compatibility while allowing multiple chats per
    /// project, each with its own corroborated agent session.
    pub fn with_conversation_store(mut self, store: Arc<dyn ConversationStore>) -> Self {
        self.conversation_store = Some(store);
        self
    }

    pub fn persistence_capabilities(&self) -> Option<PersistenceCapabilities> {
        self.project_state_store
            .as_ref()
            .map(|store| store.capabilities())
    }

    /// Attach the app-private checkpoint store. Constructors remain fail-closed until explicitly
    /// wired by the Android-local runtime layer.
    pub fn with_checkpoint_store(mut self, store: Arc<dyn CheckpointStore>) -> Self {
        self.checkpoint_store = Some(store);
        self
    }

    pub fn checkpoint_capabilities(&self) -> Option<CheckpointCapabilities> {
        self.checkpoint_store
            .as_ref()
            .map(|store| store.capabilities())
    }

    pub fn agent_capabilities(&self) -> RuntimeCapabilities {
        self.agent.capabilities()
    }

    /// Submit one structured shell-free command request to the local approval policy. Approval
    /// still does not spawn; Part 15 execution requires a separate execution-time core call.
    pub async fn request_project_command(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        command: CommandSpec,
    ) -> Result<CommandRequestOutcome> {
        self.workspace.verify_project(project).await?;
        self.agent
            .verify_session_project_binding(project, session_id)
            .await?;
        self.command_policy
            .request_command(session_id, project.id, command)
    }

    /// Resolve exactly one pending command approval. Denial only needs the exact broker scope; an
    /// allow-once decision additionally re-verifies workspace and current agent session binding
    /// before any execution envelope can be issued.
    pub async fn decide_project_command(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        approval: &CommandApprovalRequest,
        decision: CommandApprovalDecision,
    ) -> Result<CommandDecisionOutcome> {
        if decision == CommandApprovalDecision::AllowOnce {
            self.workspace.verify_project(project).await?;
            self.agent
                .verify_session_project_binding(project, session_id)
                .await?;
        }
        self.command_policy
            .decide(session_id, project.id, approval, decision)
    }

    pub fn revoke_pending_project_commands(&self, session_id: &SessionId) -> Result<usize> {
        self.command_policy.revoke_pending_for_session(session_id)
    }

    pub fn process_capabilities(&self) -> Option<ProcessRuntimeCapabilities> {
        self.process_runtime
            .as_ref()
            .map(|runtime| runtime.capabilities())
    }

    /// Start one previously approved command. Execution-time workspace and Jcode session binding
    /// are rechecked here because an allow-once decision is not a durable filesystem/session proof.
    pub async fn start_authorized_project_command(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningProcess> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.start_authorized_project_command_with_lifecycle_held(
            project, session_id, envelope, options,
        )
        .await
    }

    async fn start_authorized_project_command_with_lifecycle_held(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningProcess> {
        if envelope.project_id() != project.id || envelope.session_id() != session_id {
            return Err(VibeCoderError::Process(
                "process_authorization_scope_mismatch".into(),
            ));
        }
        self.workspace.verify_project(project).await?;
        self.agent
            .verify_session_project_binding(project, session_id)
            .await?;
        let runtime = self
            .process_runtime
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "process runtime",
                capability: "local execution",
            })?;
        self.command_policy.validate_execution_envelope(&envelope)?;
        runtime.start(project, envelope, options)
    }

    /// Create a fresh, move-only build identity before process start so a caller can represent a
    /// real queued state. This is metadata only and grants no command/process authority.
    pub async fn prepare_build_job(
        &self,
        project: &ProjectRef,
        target: BuildTargetKind,
    ) -> Result<BuildJobDescriptor> {
        self.workspace.verify_project(project).await?;
        Ok(BuildJobDescriptor::new(project.id, target))
    }

    /// Start one authorized command as the exact prepared build job. The descriptor is consumed so
    /// one build id cannot be accidentally replayed onto multiple local processes through this API.
    /// Part 18 still does not choose the command, detect a toolchain, or infer artifacts.
    pub async fn start_authorized_build_job(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        descriptor: BuildJobDescriptor,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningBuildJob> {
        if descriptor.project_id() != project.id {
            return Err(VibeCoderError::Build("build_project_scope_mismatch".into()));
        }
        let running = self
            .start_authorized_project_command(project, session_id, envelope, options)
            .await?;
        Ok(RunningBuildJob::from_running_process(descriptor, running))
    }

    /// Cancellation only removes authority, so it does not require the original session to remain
    /// attached. The process runtime still requires the id of a currently active process.
    pub fn cancel_project_process(&self, process_id: ProcessId) -> Result<()> {
        let runtime = self
            .process_runtime
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "process runtime",
                capability: "local execution",
            })?;
        runtime.cancel(process_id)
    }

    /// Create one immutable project checkpoint only while the controlled agent/process layers are
    /// quiescent. The checkpoint adapter independently detects project mutation during copying.
    pub async fn create_project_checkpoint(
        &self,
        project: &ProjectRef,
        reason: CheckpointReason,
    ) -> Result<CheckpointMetadata> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.checkpoint_store()?
            .create_checkpoint(project, reason)
            .await
    }

    pub async fn list_project_checkpoints(
        &self,
        project_id: ProjectId,
        max_results: usize,
    ) -> Result<Vec<CheckpointMetadata>> {
        self.workspace.open_project(project_id).await?;
        self.checkpoint_store()?
            .list_checkpoints(project_id, max_results)
            .await
    }

    pub async fn remove_project_checkpoint(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<()> {
        self.workspace.open_project(project_id).await?;
        self.ensure_no_active_project_process(project_id)?;
        self.checkpoint_store()?
            .remove_checkpoint(project_id, checkpoint_id)
            .await
    }

    /// Restore the complete project tree from one immutable checkpoint. Pending command approvals
    /// for the project are revoked before the atomic directory exchange. If a persisted Jcode
    /// session exists, it is forcibly reattached/corroborated against the replaced workspace after
    /// commit; a refresh failure does not undo an already integrity-verified filesystem rollback.
    pub async fn rollback_project_checkpoint(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
    ) -> Result<(ProjectRef, RollbackResult)> {
        self.rollback_project_checkpoint_internal(project, checkpoint_id, None)
            .await
    }

    /// Same atomic rollback machinery as the public checkpoint API, but permits exactly one
    /// already-persisted conversation turn to remain pending while its own failure path restores
    /// the workspace. This capability is private to Core: callers cannot use a pending turn as a
    /// general rollback bypass.
    async fn rollback_project_checkpoint_for_pending_conversation(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
        conversation_id: ConversationId,
        session_id: &SessionId,
    ) -> Result<(ProjectRef, RollbackResult)> {
        self.rollback_project_checkpoint_internal(
            project,
            checkpoint_id,
            Some((conversation_id, session_id)),
        )
        .await
    }

    async fn rollback_project_checkpoint_internal(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
        permitted_pending_conversation: Option<(ConversationId, &SessionId)>,
    ) -> Result<(ProjectRef, RollbackResult)> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.command_policy
            .invalidate_project_authorizations(project.id)?;

        let persisted_session = if let Some(store) = self.project_state_store.as_ref() {
            match store.load_project_state(project.id).await? {
                Some(state) if state.session_creation_pending => {
                    return Err(VibeCoderError::Checkpoint(
                        "checkpoint_rollback_session_creation_pending".into(),
                    ));
                }
                Some(state) => state
                    .agent_session
                    .map(|session| {
                        if session.runtime_id != self.agent.runtime_id() {
                            return Err(VibeCoderError::Checkpoint(
                                "checkpoint_rollback_session_runtime_mismatch".into(),
                            ));
                        }
                        Ok(session.session_id)
                    })
                    .transpose()?,
                None => None,
            }
        } else {
            None
        };

        // Part 34 conversations own independent Jcode sessions. Public rollback rejects every
        // in-flight chat. The private failure-recovery path above may nominate exactly one pending
        // conversation, and only when its persisted session id still matches the turn owner.
        let persisted_conversation_sessions = if let Some(store) = self.conversation_store.as_ref() {
            let ids = store
                .list_conversation_ids(project.id, MAX_PERSISTED_CONVERSATIONS_PER_PROJECT)
                .await?;
            let mut sessions = Vec::with_capacity(ids.len());
            let mut unique = HashSet::with_capacity(ids.len());
            let mut permitted_pending_seen = false;
            for conversation_id in ids {
                let conversation = store
                    .load_conversation(project.id, conversation_id)
                    .await?
                    .ok_or_else(|| {
                        VibeCoderError::Checkpoint(
                            "checkpoint_rollback_conversation_disappeared".into(),
                        )
                    })?;
                if conversation.session_creation_pending {
                    return Err(VibeCoderError::Checkpoint(
                        "checkpoint_rollback_conversation_session_creation_pending".into(),
                    ));
                }
                let session = conversation.agent_session.ok_or_else(|| {
                    VibeCoderError::Checkpoint(
                        "checkpoint_rollback_conversation_session_missing".into(),
                    )
                })?;
                if session.runtime_id != self.agent.runtime_id() {
                    return Err(VibeCoderError::Checkpoint(
                        "checkpoint_rollback_conversation_runtime_mismatch".into(),
                    ));
                }
                if conversation.turn_pending {
                    let permitted = permitted_pending_conversation.is_some_and(
                        |(allowed_conversation_id, allowed_session_id)| {
                            conversation.conversation_id == allowed_conversation_id
                                && &session.session_id == allowed_session_id
                        },
                    );
                    if !permitted || permitted_pending_seen {
                        return Err(VibeCoderError::Checkpoint(
                            "checkpoint_rollback_conversation_turn_pending".into(),
                        ));
                    }
                    permitted_pending_seen = true;
                }
                if !unique.insert(session.session_id.0.clone()) {
                    return Err(VibeCoderError::Checkpoint(
                        "checkpoint_rollback_conversation_session_duplicate".into(),
                    ));
                }
                sessions.push(session.session_id);
            }
            if permitted_pending_conversation.is_some() && !permitted_pending_seen {
                return Err(VibeCoderError::Checkpoint(
                    "checkpoint_rollback_permitted_pending_conversation_missing".into(),
                ));
            }
            sessions
        } else {
            if permitted_pending_conversation.is_some() {
                return Err(VibeCoderError::Checkpoint(
                    "checkpoint_rollback_pending_conversation_store_missing".into(),
                ));
            }
            Vec::new()
        };

        let result = self
            .checkpoint_store()?
            .rollback_project(project, checkpoint_id)
            .await?;
        // Revoke approvals that may have been issued while rollback held the project lifecycle
        // permit. They were authorized against the pre-refresh workspace/session identity.
        self.command_policy
            .invalidate_project_authorizations(project.id)?;
        let reopened = self.workspace.open_project(project.id).await?;
        self.workspace.verify_project(&reopened).await?;
        if let Some(session_id) = persisted_session {
            self.agent
                .refresh_session_after_workspace_replacement(&reopened, &session_id)
                .await
                .map_err(|_| {
                    VibeCoderError::Checkpoint(
                        "checkpoint_rollback_committed_agent_refresh_failed".into(),
                    )
                })?;
        }
        for session_id in persisted_conversation_sessions {
            self.agent
                .refresh_session_after_workspace_replacement(&reopened, &session_id)
                .await
                .map_err(|_| {
                    VibeCoderError::Checkpoint(
                        "checkpoint_rollback_committed_conversation_refresh_failed".into(),
                    )
                })?;
        }
        Ok((reopened, result))
    }

    pub async fn gateway_health(
        &self,
        gateway_credential: GatewayCredential<'_>,
    ) -> Result<GatewayHealth> {
        self.gateway.health(gateway_credential).await
    }

    pub async fn list_gateway_models(
        &self,
        gateway_credential: GatewayCredential<'_>,
    ) -> Result<Vec<ModelRef>> {
        self.gateway.list_models(gateway_credential).await
    }

    pub async fn gateway_execution_profile(
        &self,
        gateway_credential: GatewayCredential<'_>,
    ) -> Result<GatewayExecutionProfile> {
        self.gateway.execution_profile(gateway_credential).await
    }

    /// Resolve a persisted secret reference only for the duration of one health request.
    pub async fn gateway_health_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
    ) -> Result<GatewayHealth> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.gateway_health(gateway_credential_from_secret(secret.as_ref())?)
            .await
    }

    /// Resolve a persisted secret reference only for the duration of one catalog request.
    pub async fn list_gateway_models_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
    ) -> Result<Vec<ModelRef>> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.list_gateway_models(gateway_credential_from_secret(secret.as_ref())?)
            .await
    }

    /// Resolve one explicit primary+fallback route policy against a fresh credential-scoped
    /// gateway catalog. The resolved policy contains only exact models observed in that catalog.
    pub async fn resolve_model_route_policy(
        &self,
        gateway_credential: GatewayCredential<'_>,
        policy: &ModelRoutePolicyConfig,
    ) -> Result<ResolvedModelRoutePolicy> {
        let catalog = self.list_gateway_models(gateway_credential).await?;
        ResolvedModelRoutePolicy::resolve(policy, &catalog)
    }

    pub async fn resolve_model_route_policy_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
        policy: &ModelRoutePolicyConfig,
    ) -> Result<ResolvedModelRoutePolicy> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.resolve_model_route_policy(gateway_credential_from_secret(secret.as_ref())?, policy)
            .await
    }

    /// Execute one complete prompt -> agent events/tools -> result task under a single project
    /// lifecycle permit. The running gateway profile, fresh gateway catalog, fresh Jcode catalog,
    /// and fresh active Jcode model/provider must all agree before inference begins.
    pub async fn run_backend_task(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        gateway_credential: GatewayCredential<'_>,
        policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<BackendTaskOutcome> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.agent
            .verify_session_project_binding(project, session_id)
            .await?;

        let capabilities = self.agent.ensure_ready().await?;
        if !capabilities.sessions || !capabilities.streaming_events || !capabilities.file_tools {
            return Err(VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "backend_task_execution",
            });
        }

        let profile = self.gateway.execution_profile(gateway_credential).await?;
        require_deterministic_gateway_profile(&profile)?;
        let agent_gateway_bridge = self.agent.model_gateway_bridge_identity();
        if let Some(bridge) = agent_gateway_bridge.as_ref() {
            require_agent_gateway_bridge_matches_profile(bridge, &profile)?;
        }
        let gateway_catalog = self.gateway.list_models(gateway_credential).await?;
        let resolved_policy = ResolvedModelRoutePolicy::resolve(policy, &gateway_catalog)?;
        let mut task = BackendTaskStateMachine::prepare(
            project.id,
            session_id.clone(),
            prompt,
            resolved_policy,
        )?;
        let downstream_events = Arc::new(Mutex::new(on_event));

        loop {
            let selected_model = task.selected_model().clone();
            let agent_catalog = self.agent.list_models(session_id).await?;
            let catalog_corroborated = if let Some(bridge) = agent_gateway_bridge.as_ref() {
                task.corroborate_bridged_agent_catalog(
                    &agent_catalog,
                    &bridge.transport_provider,
                )?
            } else {
                task.corroborate_agent_catalog(&agent_catalog)?
            };
            if !catalog_corroborated {
                match task.decide_failure(RouteFailureClass::ModelUnavailable)? {
                    BackendTaskFailureDecision::RetryConfiguredFallback => continue,
                    BackendTaskFailureDecision::Stop(_) => {
                        return Err(VibeCoderError::Routing(
                            "agent_model_unavailable_without_safe_fallback".into(),
                        ));
                    }
                }
            }

            let active_model = match self
                .agent
                .corroborate_model_identity(session_id, &selected_model)
                .await
            {
                Ok(active_model) => active_model,
                Err(error) => {
                    let _stop = task.decide_failure(classify_agent_failure(&error))?;
                    return Err(error);
                }
            };
            task.corroborate_active_model(&active_model)?;
            let observer = task.begin_inference()?;
            let downstream_for_attempt = Arc::clone(&downstream_events);
            let event_handler: EventHandler = Box::new(move |event| {
                observer.observe(&event);
                if let Ok(mut downstream) = downstream_for_attempt.lock()
                    && let Some(handler) = downstream.as_mut()
                {
                    handler(event);
                }
            });

            // Pre-existing approvals are not authority for this new model turn, and approvals
            // minted while a turn runs must not survive its completion/failure boundary.
            self.command_policy
                .invalidate_project_authorizations(project.id)?;
            let turn_result = self
                .agent
                .run_turn(
                    session_id,
                    prompt,
                    RunTurnOptions {
                        model: Some(selected_model),
                    },
                    Some(event_handler),
                )
                .await;
            self.command_policy
                .invalidate_project_authorizations(project.id)?;

            match turn_result {
                Ok(turn) => return task.complete(turn),
                Err(error) => match task.decide_failure(classify_agent_failure(&error))? {
                    BackendTaskFailureDecision::RetryConfiguredFallback => continue,
                    BackendTaskFailureDecision::Stop(_) => return Err(error),
                },
            }
        }
    }

    pub async fn preflight_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
    ) -> Result<()> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.preflight(gateway_credential_from_secret(secret.as_ref())?)
            .await
    }

    pub async fn preflight(&self, gateway_credential: GatewayCredential<'_>) -> Result<()> {
        let health = self.gateway_health(gateway_credential).await?;
        if !health.ready {
            return Err(VibeCoderError::Gateway(
                health
                    .detail
                    .unwrap_or_else(|| "model_gateway_not_ready".into()),
            ));
        }

        let agent = self.agent.ensure_ready().await?;
        if !agent.sessions {
            return Err(VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "sessions",
            });
        }
        if !agent.file_tools {
            return Err(VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "file_tools",
            });
        }

        let workspace = self.workspace.capabilities();
        if !workspace.managed_project_roots {
            return Err(VibeCoderError::MissingCapability {
                component: "workspace runtime",
                capability: "managed_project_roots",
            });
        }
        if !workspace.canonical_path_containment {
            return Err(VibeCoderError::MissingCapability {
                component: "workspace runtime",
                capability: "canonical_path_containment",
            });
        }
        if !workspace.read_write_files {
            return Err(VibeCoderError::MissingCapability {
                component: "workspace runtime",
                capability: "read_write_files",
            });
        }
        if !workspace.text_edit {
            return Err(VibeCoderError::MissingCapability {
                component: "workspace runtime",
                capability: "text_edit",
            });
        }
        if !workspace.project_search {
            return Err(VibeCoderError::MissingCapability {
                component: "workspace runtime",
                capability: "project_search",
            });
        }
        Ok(())
    }

    /// Create a fresh managed project and its revision-zero app-private registry state. If the
    /// state commit fails, the still-empty project directory is removed so no invisible project is
    /// left behind.
    pub async fn create_persisted_project(&self) -> Result<ProjectRef> {
        let store = self.project_state_store()?;
        let project = self
            .workspace
            .create_project(WorkspaceSpec::fresh())
            .await?;
        let state = PersistedProjectState::new(project.id);
        if let Err(error) = store.create_project_state(&state).await {
            if self.workspace.remove_project(&project).await.is_err() {
                return Err(VibeCoderError::Persistence(
                    "project_create_state_rollback_failed".into(),
                ));
            }
            return Err(error);
        }
        Ok(project)
    }

    /// Load registry state by id, then derive the physical project root from the workspace runtime.
    /// A persisted root path is never trusted because none is stored.
    pub async fn open_persisted_project(
        &self,
        id: ProjectId,
    ) -> Result<(ProjectRef, PersistedProjectState)> {
        let store = self.project_state_store()?;
        let state = store
            .load_project_state(id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("project_state_not_found".into()))?;
        let project = self.workspace.open_project(id).await?;
        Ok((project, state))
    }

    pub async fn list_persisted_project_ids(&self, max_projects: usize) -> Result<Vec<ProjectId>> {
        self.project_state_store()?
            .list_project_ids(max_projects)
            .await
    }

    /// Delete project files first, then registry metadata. If metadata deletion fails, a stale
    /// registry entry may remain but cannot reopen a missing workspace, which is safer than hiding
    /// an undeleted project directory.
    pub async fn remove_persisted_project(&self, project: &ProjectRef) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        let store = self.project_state_store()?;
        let state = store
            .load_project_state(project.id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("project_state_not_found".into()))?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.command_policy
            .invalidate_project_authorizations(project.id)?;
        if let Some(session) = &state.agent_session {
            self.command_policy
                .revoke_pending_for_session(&session.session_id)?;
        }
        self.workspace.remove_project(project).await?;
        if let Some(conversations) = self.conversation_store.as_ref() {
            conversations.remove_project_conversations(project.id).await?;
        }
        store.remove_project_state(project.id).await
    }

    /// Create one independent persisted chat and one Jcode session for it. The crash marker is
    /// committed before runtime session creation. A session that was created but could not be
    /// durably associated remains fail-closed instead of being silently reused.
    pub async fn create_persisted_conversation(
        &self,
        project: &ProjectRef,
    ) -> Result<ConversationId> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        let store = self.conversation_store()?;
        self.workspace.verify_project(project).await?;
        self.project_state_store()?
            .load_project_state(project.id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("project_state_not_found".into()))?;

        let conversation_id = ConversationId::new();
        let mut conversation = PersistedConversation::pending_creation(conversation_id, project.id);
        store.create_conversation(&conversation).await?;

        let session_id = match self
            .agent
            .create_session(project, CreateSessionOptions::default())
            .await
        {
            Ok(session_id) => session_id,
            Err(error) => {
                if store
                    .remove_conversation(project.id, conversation_id)
                    .await
                    .is_err()
                {
                    return Err(VibeCoderError::Persistence(
                        "conversation_create_rollback_failed".into(),
                    ));
                }
                return Err(error);
            }
        };

        let expected = conversation.revision;
        conversation.session_creation_pending = false;
        conversation.agent_session = Some(PersistedAgentSession {
            runtime_id: self.agent.runtime_id().into(),
            session_id,
            preferred_model: None,
        });
        store
            .update_conversation(expected, &conversation)
            .await
            .map_err(|_| {
                VibeCoderError::Persistence(
                    "conversation_session_persistence_incomplete_after_create".into(),
                )
            })?;
        Ok(conversation_id)
    }

    pub async fn list_persisted_conversation_ids(
        &self,
        project_id: ProjectId,
        max_conversations: usize,
    ) -> Result<Vec<ConversationId>> {
        self.workspace.open_project(project_id).await?;
        self.conversation_store()?
            .list_conversation_ids(project_id, max_conversations)
            .await
    }

    pub async fn load_persisted_conversation(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
    ) -> Result<PersistedConversation> {
        self.workspace.open_project(project_id).await?;
        self.conversation_store()?
            .load_conversation(project_id, conversation_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))
    }

    /// Rebuild live authority for one persisted chat. Persisted runtime/session ids are only hints;
    /// the current agent adapter must re-corroborate the binding before the session can run.
    pub async fn resume_persisted_conversation(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
    ) -> Result<(ProjectRef, SessionId)> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
        let project = self.workspace.open_project(project_id).await?;
        let conversation = self
            .conversation_store()?
            .load_conversation(project_id, conversation_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
        if conversation.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "conversation_session_creation_incomplete".into(),
            ));
        }
        if conversation.turn_pending {
            return Err(VibeCoderError::Persistence(
                "conversation_turn_recovery_required".into(),
            ));
        }
        let session = conversation.agent_session.ok_or_else(|| {
            VibeCoderError::Persistence("conversation_session_not_persisted".into())
        })?;
        if session.runtime_id != self.agent.runtime_id() {
            return Err(VibeCoderError::Persistence(
                "conversation_session_runtime_mismatch".into(),
            ));
        }
        self.agent.resume_session(&project, &session.session_id).await?;
        Ok((project, session.session_id))
    }

    /// Execute exactly one user turn for a persisted conversation. This is deliberately not an
    /// autonomous loop: one call means one answer/action turn. Repeated work must be explicitly
    /// requested by a separate bounded loop controller.
    pub async fn run_persisted_conversation_turn(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        gateway_credential: GatewayCredential<'_>,
        policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<BackendTaskOutcome> {
        if prompt.trim().is_empty() {
            return Err(VibeCoderError::InvalidRequest(
                "conversation_prompt_empty".into(),
            ));
        }
        let store = self.conversation_store()?;

        // Resume the session and durably mark the turn pending under the same project gate
        // used by rollback/build/process lifecycle operations. The gate is deliberately released
        // only after the pending marker is committed. `run_backend_task` then reacquires it; any
        // rollback that wins that small handoff window sees `turn_pending` and fails closed.
        let (project, session_id, mut conversation) = {
            let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
            let project = self.workspace.open_project(project_id).await?;
            let mut conversation = store
                .load_conversation(project_id, conversation_id)
                .await?
                .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
            if conversation.session_creation_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_creation_incomplete".into(),
                ));
            }
            if conversation.turn_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_turn_recovery_required".into(),
                ));
            }
            let session = conversation.agent_session.as_ref().ok_or_else(|| {
                VibeCoderError::Persistence("conversation_session_not_persisted".into())
            })?;
            if session.runtime_id != self.agent.runtime_id() {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_runtime_mismatch".into(),
                ));
            }
            let session_id = session.session_id.clone();
            self.agent.resume_session(&project, &session_id).await?;

            conversation.append_message(ConversationRole::User, prompt.to_owned())?;
            conversation.turn_pending = true;
            let expected = conversation.revision;
            conversation = store.update_conversation(expected, &conversation).await?;
            (project, session_id, conversation)
        };

        match self
            .run_backend_task(
                &project,
                &session_id,
                gateway_credential,
                policy,
                prompt,
                on_event,
            )
            .await
        {
            Ok(outcome) => {
                let response = outcome.turn().text.clone();
                let expected = conversation.revision;
                conversation.turn_pending = false;
                if !response.is_empty() {
                    conversation.append_message(ConversationRole::Assistant, response)?;
                }
                store.update_conversation(expected, &conversation).await?;
                Ok(outcome)
            }
            Err(error) => {
                let expected = conversation.revision;
                conversation.turn_pending = false;
                if store.update_conversation(expected, &conversation).await.is_err() {
                    return Err(VibeCoderError::Persistence(
                        "conversation_turn_failure_cleanup_failed".into(),
                    ));
                }
                Err(error)
            }
        }
    }

    /// Execute one persisted coding-action turn through the attested model-gateway -> Jcode bridge.
    ///
    /// This is still one outer user turn, not an autonomous loop. The inner Jcode turn may perform
    /// several bounded file-tool continuations, but success requires a live, internally consistent
    /// tool transcript and at least one successful file mutation before the assistant response is
    /// committed. Command tools remain forbidden in this slice.
    pub async fn run_persisted_agent_action_turn(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        gateway_credential: GatewayCredential<'_>,
        policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<PersistedAgentActionTurnOutcome> {
        validate_conversation_model_prompt(prompt)?;

        let capabilities = self.agent.ensure_ready().await?;
        if !capabilities.file_tools {
            return Err(VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "file_tools",
            });
        }
        if capabilities.command_tools {
            return Err(VibeCoderError::InvalidRequest(
                "agent_action_command_tools_must_remain_disabled".into(),
            ));
        }
        let bridge = self.agent.model_gateway_bridge_identity().ok_or(
            VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "model_gateway_bridge",
            },
        )?;
        if !bridge.exact_model_id_passthrough {
            return Err(VibeCoderError::Gateway(
                "agent_action_exact_model_bridge_required".into(),
            ));
        }

        let store = self.conversation_store()?;
        let (project, session_id, conversation, checkpoint) = {
            let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
            let project = self.workspace.open_project(project_id).await?;
            self.workspace.verify_project(&project).await?;
            let mut conversation = store
                .load_conversation(project_id, conversation_id)
                .await?
                .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
            if conversation.session_creation_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_creation_incomplete".into(),
                ));
            }
            if conversation.turn_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_turn_recovery_required".into(),
                ));
            }
            let session = conversation.agent_session.as_ref().ok_or_else(|| {
                VibeCoderError::Persistence("conversation_session_not_persisted".into())
            })?;
            if session.runtime_id != self.agent.runtime_id() {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_runtime_mismatch".into(),
                ));
            }
            ensure_conversation_agent_action_capacity(&conversation, prompt)?;
            let session_id = session.session_id.clone();
            self.agent.resume_session(&project, &session_id).await?;
            self.ensure_no_active_project_process(project.id)?;
            self.agent.ensure_workspace_quiescent(&project).await?;
            let checkpoint = self
                .checkpoint_store()?
                .create_checkpoint(&project, CheckpointReason::BeforeAgentChange)
                .await?;

            // User intent and the crash marker become durable before Jcode/model activity starts.
            conversation.append_message(ConversationRole::User, prompt.to_owned())?;
            conversation.turn_pending = true;
            let expected = conversation.revision;
            conversation = match store.update_conversation(expected, &conversation).await {
                Ok(conversation) => conversation,
                Err(error) => {
                    if let Ok(checkpoints) = self.checkpoint_store() {
                        let _ = checkpoints
                            .remove_checkpoint(project.id, checkpoint.checkpoint_id)
                            .await;
                    }
                    return Err(error);
                }
            };
            (project, session_id, conversation, checkpoint)
        };

        let observation = Arc::new(Mutex::new(AgentActionObservation::default()));
        let downstream = Arc::new(Mutex::new(on_event));
        let observation_for_events = Arc::clone(&observation);
        let downstream_for_events = Arc::clone(&downstream);
        let action_event_handler: EventHandler = Box::new(move |event| {
            if let Ok(mut state) = observation_for_events.lock() {
                state.observe(&event);
            }
            if let Ok(mut downstream) = downstream_for_events.lock()
                && let Some(handler) = downstream.as_mut()
            {
                // Presentation/event consumers are not authority for the action turn and cannot
                // be allowed to disable the internal acceptance observer by unwinding through it.
                let _ = catch_unwind(AssertUnwindSafe(|| handler(event)));
            }
        });

        let action = self
            .run_backend_task(
                &project,
                &session_id,
                gateway_credential,
                policy,
                prompt,
                Some(action_event_handler),
            )
            .await;

        match action {
            Ok(outcome) => {
                let validation = match observation.lock() {
                    Ok(state) => validate_agent_action_turn(outcome.turn(), &state),
                    Err(_) => Err(VibeCoderError::Agent(
                        "agent_action_observation_poisoned".into(),
                    )),
                };
                let (observed_tool_calls, successful_file_tool_calls, successful_mutations) =
                    match validation {
                        Ok(value) => value,
                        Err(error) => {
                            self.recover_failed_persisted_agent_action(
                                store,
                                &project,
                                &conversation,
                                checkpoint.checkpoint_id,
                                None,
                            )
                            .await?;
                            return Err(error);
                        }
                    };

                let post_action_checkpoint = match self
                    .create_project_checkpoint(&project, CheckpointReason::AgentChangeVerification)
                    .await
                {
                    Ok(checkpoint) => checkpoint,
                    Err(error) => {
                        self.recover_failed_persisted_agent_action(
                            store,
                            &project,
                            &conversation,
                            checkpoint.checkpoint_id,
                            None,
                        )
                        .await?;
                        return Err(error);
                    }
                };
                if post_action_checkpoint.tree_sha256 == checkpoint.tree_sha256 {
                    self.recover_failed_persisted_agent_action(
                        store,
                        &project,
                        &conversation,
                        checkpoint.checkpoint_id,
                        Some(post_action_checkpoint.checkpoint_id),
                    )
                    .await?;
                    return Err(VibeCoderError::Agent(
                        "agent_action_workspace_unchanged".into(),
                    ));
                }

                let assistant_text = outcome.turn().text.clone();
                let mut completed = conversation.clone();
                completed.turn_pending = false;
                if let Err(error) = completed.append_message(ConversationRole::Assistant, assistant_text)
                {
                    self.recover_failed_persisted_agent_action(
                        store,
                        &project,
                        &conversation,
                        checkpoint.checkpoint_id,
                        Some(post_action_checkpoint.checkpoint_id),
                    )
                    .await?;
                    return Err(error);
                }
                if let Err(error) = store
                    .update_conversation(conversation.revision, &completed)
                    .await
                {
                    self.recover_failed_persisted_agent_action(
                        store,
                        &project,
                        &conversation,
                        checkpoint.checkpoint_id,
                        Some(post_action_checkpoint.checkpoint_id),
                    )
                    .await?;
                    return Err(error);
                }
                let post_checkpoint_removed = self
                    .remove_project_checkpoint(project.id, post_action_checkpoint.checkpoint_id)
                    .await
                    .is_ok();
                let pre_checkpoint_removed = self
                    .remove_project_checkpoint(project.id, checkpoint.checkpoint_id)
                    .await
                    .is_ok();
                let checkpoint_cleanup_complete =
                    post_checkpoint_removed && pre_checkpoint_removed;
                Ok(PersistedAgentActionTurnOutcome {
                    backend: outcome,
                    observed_tool_calls,
                    successful_file_tool_calls,
                    successful_mutation_tool_calls: successful_mutations,
                    pre_action_tree_sha256: checkpoint.tree_sha256,
                    post_action_tree_sha256: post_action_checkpoint.tree_sha256,
                    checkpoint_cleanup_complete,
                })
            }
            Err(error) => {
                self.recover_failed_persisted_agent_action(
                    store,
                    &project,
                    &conversation,
                    checkpoint.checkpoint_id,
                    None,
                )
                .await?;
                Err(error)
            }
        }
    }

    pub async fn run_persisted_agent_action_turn_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
        project_id: ProjectId,
        conversation_id: ConversationId,
        policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<PersistedAgentActionTurnOutcome> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.run_persisted_agent_action_turn(
            project_id,
            conversation_id,
            gateway_credential_from_secret(secret.as_ref())?,
            policy,
            prompt,
            on_event,
        )
        .await
    }

    /// Execute exactly one persisted conversation turn directly through the attested model gateway.
    /// This Part-34.6 path deliberately does not invoke Jcode tools, retry, fallback, or loop. The
    /// existing `run_persisted_conversation_turn` remains the agent/Jcode path for Part 34.7.
    pub async fn run_persisted_model_conversation_turn(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        gateway_credential: GatewayCredential<'_>,
        requested_model_id: &str,
        max_output_tokens: u32,
        prompt: &str,
    ) -> Result<ConversationModelTurnOutcome> {
        self.run_persisted_model_conversation_turn_inner(
            project_id,
            conversation_id,
            gateway_credential,
            requested_model_id,
            max_output_tokens,
            prompt,
            None,
        )
        .await
    }

    /// Android/UI-owned cancellable variant of the one-shot direct model turn.
    /// Cancellation is cooperative but authoritative: the in-flight HTTP future is dropped,
    /// the durable `turn_pending` marker is cleared by this same turn owner, and a racing
    /// successful response is rejected if cancellation won before durable assistant commit.
    pub async fn run_persisted_model_conversation_turn_cancellable(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        gateway_credential: GatewayCredential<'_>,
        requested_model_id: &str,
        max_output_tokens: u32,
        prompt: &str,
        cancellation: &ConversationModelTurnCancellation,
    ) -> Result<ConversationModelTurnOutcome> {
        self.run_persisted_model_conversation_turn_inner(
            project_id,
            conversation_id,
            gateway_credential,
            requested_model_id,
            max_output_tokens,
            prompt,
            Some(cancellation),
        )
        .await
    }

    async fn run_persisted_model_conversation_turn_inner(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        gateway_credential: GatewayCredential<'_>,
        requested_model_id: &str,
        max_output_tokens: u32,
        prompt: &str,
        cancellation: Option<&ConversationModelTurnCancellation>,
    ) -> Result<ConversationModelTurnOutcome> {
        validate_conversation_model_id(requested_model_id)?;
        validate_conversation_model_prompt(prompt)?;
        if max_output_tokens == 0 || max_output_tokens > MAX_CONVERSATION_MODEL_OUTPUT_TOKENS {
            return Err(VibeCoderError::InvalidRequest(
                "conversation_model_output_tokens_invalid".into(),
            ));
        }

        let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
        let store = self.conversation_store()?;
        let project = self.workspace.open_project(project_id).await?;
        self.workspace.verify_project(&project).await?;
        let mut conversation = store
            .load_conversation(project_id, conversation_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
        if conversation.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "conversation_session_creation_incomplete".into(),
            ));
        }
        if conversation.turn_pending {
            return Err(VibeCoderError::Persistence(
                "conversation_turn_recovery_required".into(),
            ));
        }
        let session = conversation.agent_session.as_ref().ok_or_else(|| {
            VibeCoderError::Persistence("conversation_session_not_persisted".into())
        })?;
        if session.runtime_id != self.agent.runtime_id() {
            return Err(VibeCoderError::Persistence(
                "conversation_session_runtime_mismatch".into(),
            ));
        }
        ensure_conversation_model_turn_capacity(&conversation, prompt)?;

        // For the UI-owned cancellable path, a racing Stop after Send still preserves the user's
        // durable message. Cancellation takes authority immediately after this commit and before
        // any model network work is polled.

        // Durable user-message + crash marker comes before network inference. If the process dies
        // after this commit, recovery sees an explicit pending turn rather than fabricating success.
        conversation.append_message(ConversationRole::User, prompt.to_owned())?;
        conversation.turn_pending = true;
        let expected = conversation.revision;
        conversation = store.update_conversation(expected, &conversation).await?;

        let inference_future = async {
            let profile = self.gateway.execution_profile(gateway_credential).await?;
            require_deterministic_gateway_profile(&profile)?;
            let catalog = self.gateway.list_models(gateway_credential).await?;
            let model = select_exact_gateway_model(&catalog, requested_model_id)?;
            let (messages, context_bytes_sent) =
                build_gateway_conversation_context(&conversation)?;
            let context_messages_sent = messages.len();
            let request = GatewayChatRequest {
                model: model.clone(),
                messages,
                max_output_tokens,
            };

            // Exactly one gateway inference call. No retry, fallback, agent tools, or autonomous loop
            // belongs in the direct model controller path.
            let response = self
                .gateway
                .chat_completion(gateway_credential, &request)
                .await?;
            validate_conversation_model_response(&model, &response)?;
            Ok::<_, VibeCoderError>((model, response, context_messages_sent, context_bytes_sent))
        };

        let mut inference = match cancellation {
            Some(cancellation) => {
                await_conversation_model_inference_or_cancel(inference_future, cancellation).await
            }
            None => inference_future.await,
        };

        // Cancel wins until the assistant message is durably committed. This closes the race where
        // the HTTP response and UI Stop arrive almost simultaneously.
        if cancellation.is_some_and(ConversationModelTurnCancellation::is_requested) {
            inference = Err(VibeCoderError::Cancelled);
        }

        match inference {
            Ok((model, response, context_messages_sent, context_bytes_sent)) => {
                let assistant_text = response.text.clone();
                let expected = conversation.revision;
                conversation.turn_pending = false;
                conversation.append_message(ConversationRole::Assistant, assistant_text.clone())?;
                store.update_conversation(expected, &conversation).await?;
                Ok(ConversationModelTurnOutcome {
                    model,
                    observed_model_id: response.observed_model_id,
                    finish_reason: response.finish_reason,
                    usage: response.usage,
                    assistant_text,
                    context_messages_sent,
                    context_bytes_sent,
                })
            }
            Err(error) => {
                let expected = conversation.revision;
                conversation.turn_pending = false;
                if store.update_conversation(expected, &conversation).await.is_err() {
                    return Err(VibeCoderError::Persistence(
                        "conversation_model_turn_failure_cleanup_failed".into(),
                    ));
                }
                Err(error)
            }
        }
    }

    pub async fn run_persisted_model_conversation_turn_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
        project_id: ProjectId,
        conversation_id: ConversationId,
        requested_model_id: &str,
        max_output_tokens: u32,
        prompt: &str,
    ) -> Result<ConversationModelTurnOutcome> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.run_persisted_model_conversation_turn(
            project_id,
            conversation_id,
            gateway_credential_from_secret(secret.as_ref())?,
            requested_model_id,
            max_output_tokens,
            prompt,
        )
        .await
    }

    /// Cooperative cancellation for the currently running turn in one persisted chat. The durable
    /// pending marker is cleared by the turn owner after the agent returns `Cancelled`.
    pub async fn cancel_persisted_conversation_turn(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
    ) -> Result<()> {
        let conversation = self
            .conversation_store()?
            .load_conversation(project_id, conversation_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
        let session = conversation.agent_session.ok_or_else(|| {
            VibeCoderError::Persistence("conversation_session_not_persisted".into())
        })?;
        if session.runtime_id != self.agent.runtime_id() {
            return Err(VibeCoderError::Persistence(
                "conversation_session_runtime_mismatch".into(),
            ));
        }
        self.agent.cancel(&session.session_id).await
    }

    /// Create a new agent session with a persisted crash marker. The marker is committed before
    /// asking the runtime to create a session, so a crash cannot make an ambiguous half-created
    /// session look cleanly absent on restart.
    pub async fn start_persisted_project_session(&self, project: &ProjectRef) -> Result<SessionId> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        let store = self.project_state_store()?;
        self.workspace.verify_project(project).await?;
        let mut state = store
            .load_project_state(project.id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("project_state_not_found".into()))?;
        if state.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "project_session_creation_incomplete".into(),
            ));
        }
        if state.agent_session.is_some() {
            return Err(VibeCoderError::Persistence(
                "project_session_already_persisted".into(),
            ));
        }

        let pending_revision = state.revision;
        state.session_creation_pending = true;
        state = store.update_project_state(pending_revision, &state).await?;

        let session_id = match self
            .agent
            .create_session(project, CreateSessionOptions::default())
            .await
        {
            Ok(session_id) => session_id,
            Err(error) => {
                let expected = state.revision;
                state.session_creation_pending = false;
                if store.update_project_state(expected, &state).await.is_err() {
                    return Err(VibeCoderError::Persistence(
                        "project_session_pending_clear_failed".into(),
                    ));
                }
                return Err(error);
            }
        };

        let expected = state.revision;
        state.session_creation_pending = false;
        state.agent_session = Some(PersistedAgentSession {
            runtime_id: self.agent.runtime_id().into(),
            session_id: session_id.clone(),
            preferred_model: None,
        });
        store
            .update_project_state(expected, &state)
            .await
            .map_err(|_| {
                VibeCoderError::Persistence(
                    "project_session_persistence_incomplete_after_create".into(),
                )
            })?;
        Ok(session_id)
    }

    /// Reopen the workspace from ProjectId and re-corroborate the persisted session against the
    /// current agent runtime. Persisted ids are hints only; this call rebuilds live authority.
    pub async fn resume_persisted_project_session(
        &self,
        project_id: ProjectId,
    ) -> Result<(ProjectRef, SessionId)> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
        let (project, state) = self.open_persisted_project(project_id).await?;
        if state.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "project_session_creation_incomplete".into(),
            ));
        }
        let session = state
            .agent_session
            .ok_or_else(|| VibeCoderError::Persistence("project_session_not_persisted".into()))?;
        if session.runtime_id != self.agent.runtime_id() {
            return Err(VibeCoderError::Persistence(
                "project_session_runtime_mismatch".into(),
            ));
        }
        self.agent
            .resume_session(&project, &session.session_id)
            .await?;
        Ok((project, session.session_id))
    }

    /// Persist an exact desired model identity, then ask the verified session runtime to apply it.
    /// The persisted value is a preference, not proof that a runtime currently has that model
    /// active; `set_model` still performs fresh runtime/catalog verification.
    pub async fn set_persisted_session_model(
        &self,
        project_id: ProjectId,
        model: &ModelRef,
    ) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project_id)?;
        let store = self.project_state_store()?;
        let (project, mut state) = self.open_persisted_project(project_id).await?;
        if state.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "project_session_creation_incomplete".into(),
            ));
        }
        let session_id = {
            let session = state.agent_session.as_ref().ok_or_else(|| {
                VibeCoderError::Persistence("project_session_not_persisted".into())
            })?;
            if session.runtime_id != self.agent.runtime_id() {
                return Err(VibeCoderError::Persistence(
                    "project_session_runtime_mismatch".into(),
                ));
            }
            session.session_id.clone()
        };
        self.agent.resume_session(&project, &session_id).await?;
        let target = ModelRouteTargetConfig {
            model_id: model.id.clone(),
            provider: model.provider.clone(),
        };
        target.validate()?;
        state
            .agent_session
            .as_mut()
            .ok_or_else(|| VibeCoderError::Persistence("project_session_not_persisted".into()))?
            .preferred_model = Some(target);
        let expected = state.revision;
        store.update_project_state(expected, &state).await?;
        self.agent.set_model(&session_id, model).await
    }

    pub async fn set_persisted_route_policy(
        &self,
        project_id: ProjectId,
        route_policy: Option<ModelRoutePolicyConfig>,
    ) -> Result<PersistedProjectState> {
        let store = self.project_state_store()?;
        self.workspace.open_project(project_id).await?;
        if let Some(policy) = &route_policy {
            policy.validate()?;
        }
        let mut state = store
            .load_project_state(project_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("project_state_not_found".into()))?;
        if state.session_creation_pending {
            return Err(VibeCoderError::Persistence(
                "project_session_creation_incomplete".into(),
            ));
        }
        let expected = state.revision;
        state.route_policy = route_policy;
        store.update_project_state(expected, &state).await
    }

    /// Create a fresh project under the workspace runtime's managed app-private root.
    pub async fn create_project(&self) -> Result<ProjectRef> {
        self.workspace.create_project(WorkspaceSpec::fresh()).await
    }

    /// Re-open a managed project by id. Persisted/caller-supplied root paths are not accepted.
    pub async fn open_project(&self, id: ProjectId) -> Result<ProjectRef> {
        self.workspace.open_project(id).await
    }

    pub async fn remove_project(&self, project: &ProjectRef) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.command_policy
            .invalidate_project_authorizations(project.id)?;
        self.workspace.remove_project(project).await
    }

    pub async fn resolve_project_path(
        &self,
        project: &ProjectRef,
        relative: &Path,
    ) -> Result<PathBuf> {
        self.workspace.resolve_project_path(project, relative).await
    }

    /// Create a project-relative directory tree using the workspace runtime's operation-time
    /// containment checks.
    pub async fn create_project_dir_all(
        &self,
        project: &ProjectRef,
        relative: &Path,
    ) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.create_dir_all(project, relative).await
    }

    /// Read one bounded regular project file through the safe workspace primitive.
    pub async fn read_project_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        self.workspace.read_file(project, relative, max_bytes).await
    }

    /// Atomically replace/create one regular file within an existing project directory.
    pub async fn atomic_write_project_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        contents: &[u8],
    ) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace
            .atomic_write_file(project, relative, contents)
            .await
    }

    /// Replace exactly one expected UTF-8 text fragment. Ambiguous/missing matches or a target
    /// changed during the operation fail closed without committing the replacement.
    pub async fn edit_project_text_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        expected: &str,
        replacement: &str,
    ) -> Result<TextEditResult> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace
            .edit_text_file(project, relative, expected, replacement)
            .await
    }

    /// Apply multiple exact UTF-8 hunks as one all-or-nothing atomic patch.
    pub async fn apply_project_text_patch(
        &self,
        project: &ProjectRef,
        relative: &Path,
        hunks: &[TextPatchHunk],
    ) -> Result<TextPatchResult> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace
            .apply_text_patch(project, relative, hunks)
            .await
    }

    /// Deterministically discover bounded regular files without exposing absolute app-private paths.
    pub async fn list_project_files(
        &self,
        project: &ProjectRef,
        max_entries: usize,
    ) -> Result<ProjectFileList> {
        self.workspace
            .list_project_files(project, max_entries)
            .await
    }

    /// Literal UTF-8 search over safely discovered project files.
    pub async fn search_project_text(
        &self,
        project: &ProjectRef,
        needle: &str,
        max_matches: usize,
    ) -> Result<ProjectTextSearchResult> {
        self.workspace
            .search_project_text(project, needle, max_matches)
            .await
    }

    /// Inspect website metadata through the safe workspace boundary and return a deterministic,
    /// read-only logical toolchain/build intent. Part 19 does not execute package scripts.
    pub async fn inspect_website_toolchain(
        &self,
        project: &ProjectRef,
    ) -> Result<WebsiteToolchainReport> {
        inspect_website_project(&self.workspace, project).await
    }

    /// Prepare one website build pipeline from a fresh Part-19 inspection. This grants no process
    /// authority; each install/build stage must still pass through the Part-14 allow-once broker.
    pub async fn prepare_website_build_pipeline(
        &self,
        project: &ProjectRef,
        policy: WebsiteBuildPolicy,
    ) -> Result<WebsiteBuildPipeline> {
        self.workspace.verify_project(project).await?;
        let report = inspect_website_project(&self.workspace, project).await?;
        WebsiteBuildPipeline::new(project.id, report, policy)
    }

    /// Submit the pipeline's exact current command for explicit allow-once approval. The complete
    /// toolchain report is freshly re-inspected first so package.json/lockfile drift invalidates the
    /// prepared pipeline before an approval can be issued.
    pub async fn request_website_build_stage_command(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        pipeline: &WebsiteBuildPipeline,
    ) -> Result<CommandRequestOutcome> {
        if pipeline.project_id() != project.id {
            return Err(VibeCoderError::Build(
                "web_build_project_scope_mismatch".into(),
            ));
        }
        let current = inspect_website_project(&self.workspace, project).await?;
        pipeline.verify_toolchain_unchanged(&current)?;
        let command = pipeline.current_command()?;
        self.request_project_command(project, session_id, command)
            .await
    }

    /// Start exactly the command bound to the current website pipeline stage. Both the toolchain
    /// report and the authorized command are rechecked immediately before delegating to the existing
    /// build/process boundary. The consumed pipeline becomes a running-stage handle.
    pub async fn start_authorized_website_build_stage(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        pipeline: WebsiteBuildPipeline,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningWebsiteBuildStage> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        if pipeline.project_id() != project.id {
            return Err(VibeCoderError::Build(
                "web_build_project_scope_mismatch".into(),
            ));
        }
        self.workspace.verify_project(project).await?;
        self.agent.ensure_workspace_quiescent(project).await?;
        let current = inspect_website_project(&self.workspace, project).await?;
        pipeline.verify_toolchain_unchanged(&current)?;
        pipeline.command_matches_current_stage(envelope.command())?;
        let descriptor = BuildJobDescriptor::new(project.id, BuildTargetKind::Website);
        let running_process = self
            .start_authorized_project_command_with_lifecycle_held(
                project, session_id, envelope, options,
            )
            .await?;
        let running = RunningBuildJob::from_running_process(descriptor, running_process);
        pipeline.into_running(running)
    }

    /// Capture one terminal failed build, create a rollback point, and run exactly one agent repair
    /// turn. Retry/rebuild policy is intentionally deferred to Part 22. The project lifecycle permit
    /// remains held across checkpoint + repair so controlled process starts and direct Core file
    /// mutations cannot overlap the repair turn.
    pub async fn run_first_build_repair_turn(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        failed_build: &BuildResult,
        model: Option<ModelRef>,
        on_event: Option<EventHandler>,
    ) -> Result<BuildRepairTurnOutcome> {
        let plan = BuildRepairPlan::from_failed_build(failed_build)?;
        if plan.evidence().project_id() != project.id {
            return Err(VibeCoderError::Build(
                "build_repair_project_scope_mismatch".into(),
            ));
        }

        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.ensure_no_active_project_process(project.id)?;
        self.agent.ensure_workspace_quiescent(project).await?;
        self.agent
            .verify_session_project_binding(project, session_id)
            .await?;

        // Approvals from the failed build are stale before any repair mutation begins.
        self.command_policy
            .invalidate_project_authorizations(project.id)?;
        let checkpoint = self
            .checkpoint_store()?
            .create_checkpoint(project, CheckpointReason::BeforeBuildRepair)
            .await?;

        let prompt = plan.prompt().to_owned();
        let failure = plan.into_evidence();
        let turn_result = self
            .agent
            .run_turn(session_id, &prompt, RunTurnOptions { model }, on_event)
            .await;

        // Any approval minted while the repair turn was active must not survive the turn boundary.
        // Revoke even when the agent turn failed/cancelled before returning that result.
        self.command_policy
            .invalidate_project_authorizations(project.id)?;
        let turn = turn_result?;
        Ok(BuildRepairTurnOutcome {
            checkpoint,
            failure,
            turn,
        })
    }

    /// Create a bounded Part-22 repair/rebuild guard. The guard itself carries no file, process,
    /// command, checkpoint, or model authority.
    pub fn new_build_repair_loop(
        &self,
        project_id: ProjectId,
        target: BuildTargetKind,
        policy: BuildRepairLoopPolicy,
    ) -> Result<BuildRepairLoopGuard> {
        BuildRepairLoopGuard::new(project_id, target, policy)
    }

    /// Apply Part-22 retry/repeated-error/cancellation guards before delegating to the existing
    /// one-turn Part-21 repair boundary. An identical repeated failure or exhausted budget stops
    /// before creating another checkpoint or agent turn.
    pub async fn run_guarded_build_repair_turn(
        &self,
        guard: &mut BuildRepairLoopGuard,
        project: &ProjectRef,
        session_id: &SessionId,
        failed_build: &BuildResult,
        model: Option<ModelRef>,
        on_event: Option<EventHandler>,
    ) -> Result<GuardedBuildRepairDecision> {
        if guard.project_id() != project.id || guard.target() != failed_build.target() {
            return Err(VibeCoderError::Build(
                "build_loop_project_scope_mismatch".into(),
            ));
        }

        let permit = match guard.authorize_repair(failed_build)? {
            RepairAuthorization::Stop(reason) => {
                return Ok(GuardedBuildRepairDecision::Stop(reason));
            }
            RepairAuthorization::Repair(permit) => permit,
        };
        let attempt = permit.attempt();
        let expected_fingerprint = permit.fingerprint_sha256().to_owned();
        let repair = self
            .run_first_build_repair_turn(project, session_id, failed_build, model, on_event)
            .await?;
        guard.finish_repair(permit, repair.turn(), repair.failure().fingerprint_sha256())?;
        if repair.failure().fingerprint_sha256() != expected_fingerprint {
            return Err(VibeCoderError::Build(
                "build_loop_repair_evidence_changed".into(),
            ));
        }
        Ok(GuardedBuildRepairDecision::RepairTurn(
            GuardedBuildRepairTurnOutcome {
                attempt,
                stop_reason: guard.stop_reason(),
                repair,
            },
        ))
    }

    /// Record one completed Part-20 website stage without confusing dependency-install success
    /// for whole-build success. Failed stages remain repair-eligible and are deliberately left in
    /// `AwaitingBuildResult` for `run_guarded_build_repair_turn`.
    pub fn record_guarded_website_build_completion(
        &self,
        guard: &mut BuildRepairLoopGuard,
        completion: &WebsiteBuildStageCompletion,
    ) -> Result<Option<BuildRepairLoopStopReason>> {
        if guard.project_id() != completion.pipeline().project_id()
            || guard.target() != BuildTargetKind::Website
        {
            return Err(VibeCoderError::Build(
                "build_loop_project_scope_mismatch".into(),
            ));
        }
        match completion.pipeline().state() {
            WebsiteBuildPipelineState::AwaitingApproval(_) => Ok(None),
            WebsiteBuildPipelineState::Failed => Ok(None),
            WebsiteBuildPipelineState::Succeeded
            | WebsiteBuildPipelineState::Cancelled
            | WebsiteBuildPipelineState::TimedOut => {
                Ok(Some(guard.finish_nonfailed_build(completion.result())?))
            }
            WebsiteBuildPipelineState::NoBuildRequired | WebsiteBuildPipelineState::Running(_) => {
                Err(VibeCoderError::Build(
                    "build_loop_website_completion_state_invalid".into(),
                ))
            }
        }
    }

    /// Prepare the next website rebuild only after a completed guarded repair turn. This does not
    /// execute or auto-approve anything; the fresh pipeline still requires Part-14 allow-once
    /// approval for every Part-20 stage.
    pub async fn prepare_guarded_website_rebuild(
        &self,
        guard: &mut BuildRepairLoopGuard,
        project: &ProjectRef,
        policy: WebsiteBuildPolicy,
    ) -> Result<WebsiteBuildPipeline> {
        if guard.project_id() != project.id || guard.target() != BuildTargetKind::Website {
            return Err(VibeCoderError::Build(
                "build_loop_project_scope_mismatch".into(),
            ));
        }
        let permit = guard.rebuild_permit()?;
        let pipeline = self.prepare_website_build_pipeline(project, policy).await?;
        guard.mark_rebuild_prepared(permit)?;
        Ok(pipeline)
    }

    /// Cooperative loop cancellation. This invalidates outstanding command approvals immediately
    /// and prevents any later guarded repair/rebuild boundary from proceeding. Active process
    /// cancellation continues to use `cancel_project_process`; an active repair turn can be
    /// interrupted with `cancel_active_build_repair_turn`.
    pub fn request_build_repair_loop_cancel(&self, guard: &BuildRepairLoopGuard) -> Result<()> {
        guard.cancellation().request();
        self.command_policy
            .invalidate_project_authorizations(guard.project_id())
            .map(|_| ())
    }

    /// Cancel a currently running guarded website rebuild. The loop cancellation flag is set before
    /// process cancellation so even a concurrently finishing build cannot authorize another retry.
    pub fn cancel_active_guarded_website_rebuild(
        &self,
        guard: &BuildRepairLoopGuard,
        running: &RunningWebsiteBuildStage,
    ) -> Result<()> {
        if guard.project_id() != running.project_id() || guard.target() != BuildTargetKind::Website
        {
            return Err(VibeCoderError::Build(
                "build_loop_project_scope_mismatch".into(),
            ));
        }
        self.request_build_repair_loop_cancel(guard)?;
        self.cancel_project_process(running.process_id())
    }

    /// Request cancellation for a currently running repair turn and mark the whole loop cancelled.
    /// The session/project binding is corroborated before invoking the agent runtime's cancellation.
    pub async fn cancel_active_build_repair_turn(
        &self,
        guard: &BuildRepairLoopGuard,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        if guard.project_id() != project.id {
            return Err(VibeCoderError::Build(
                "build_loop_project_scope_mismatch".into(),
            ));
        }
        self.request_build_repair_loop_cancel(guard)?;
        self.workspace.verify_project(project).await?;
        self.agent
            .verify_session_project_binding(project, session_id)
            .await?;
        self.agent.cancel(session_id).await
    }

    pub async fn start_project_session(&self, project: &ProjectRef) -> Result<SessionId> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.agent
            .create_session(project, CreateSessionOptions::default())
            .await
    }

    pub async fn resume_project_session(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        let _lifecycle = self.project_lifecycle_gate.try_acquire(project.id)?;
        self.workspace.verify_project(project).await?;
        self.agent.resume_session(project, session_id).await
    }

    /// Return the switchable model catalog for one already verified agent session.
    pub async fn list_session_models(&self, session_id: &SessionId) -> Result<Vec<ModelRef>> {
        self.agent.list_models(session_id).await
    }

    /// Persistently select the model used by this agent session. Runtime-specific safety checks
    /// (catalog freshness, provider corroboration, active-turn exclusion) stay in the adapter.
    pub async fn set_session_model(&self, session_id: &SessionId, model: &ModelRef) -> Result<()> {
        self.agent.set_model(session_id, model).await
    }

    fn checkpoint_store(&self) -> Result<&Arc<dyn CheckpointStore>> {
        self.checkpoint_store
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "checkpoint store",
                capability: "project_checkpoint_rollback",
            })
    }

    fn ensure_no_active_project_process(&self, project_id: ProjectId) -> Result<()> {
        if let Some(runtime) = self.process_runtime.as_ref()
            && runtime.active_for_project(project_id)? != 0
        {
            return Err(VibeCoderError::Process("project_process_active".into()));
        }
        Ok(())
    }

    /// Create a one-shot explicit loop guard. Merely calling a normal action-turn API can never
    /// enter this mode; a caller must construct and pass this separate guard after explicit user
    /// intent. The guard is scope-bound and cannot be reused for a second loop invocation.
    pub fn new_explicit_agent_loop(
        &self,
        project_id: ProjectId,
        conversation_id: ConversationId,
        policy: ExplicitAgentLoopPolicy,
    ) -> Result<ExplicitAgentLoopGuard> {
        ExplicitAgentLoopGuard::new(project_id, conversation_id, policy)
    }

    /// Prevent another iteration from starting. To interrupt an already-running Jcode turn, use
    /// `cancel_active_explicit_agent_loop_turn`, which sets this same cancellation flag first.
    pub fn request_explicit_agent_loop_cancel(&self, guard: &ExplicitAgentLoopGuard) -> Result<()> {
        guard.cancellation.request();
        self.command_policy
            .invalidate_project_authorizations(guard.project_id)
            .map(|_| ())
    }

    /// Cancel the active inner Jcode turn for one explicit loop and permanently arm the loop's
    /// cooperative cancellation flag so a racing completion cannot authorize another iteration.
    pub async fn cancel_active_explicit_agent_loop_turn(
        &self,
        guard: &ExplicitAgentLoopGuard,
    ) -> Result<()> {
        self.request_explicit_agent_loop_cancel(guard)?;
        let project = self.workspace.open_project(guard.project_id).await?;
        self.workspace.verify_project(&project).await?;
        let conversation = self
            .conversation_store()?
            .load_conversation(guard.project_id, guard.conversation_id)
            .await?
            .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
        if !conversation.turn_pending {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_no_active_turn".into(),
            ));
        }
        let session = conversation.agent_session.ok_or_else(|| {
            VibeCoderError::Persistence("conversation_session_not_persisted".into())
        })?;
        if session.runtime_id != self.agent.runtime_id() {
            return Err(VibeCoderError::Persistence(
                "conversation_session_runtime_mismatch".into(),
            ));
        }
        self.agent
            .verify_session_project_binding(&project, &session.session_id)
            .await?;
        self.agent.cancel(&session.session_id).await
    }

    /// Execute a user-opted-in bounded multi-turn coding loop.
    ///
    /// The original user message is persisted once and `turn_pending` remains armed across every
    /// inner iteration. Intermediate model prose is not committed to chat. Each iteration must
    /// use the live file-tool transcript; `continue` additionally requires a real project-tree
    /// mutation. A terminal non-success stop restores the whole workspace to the immutable
    /// pre-loop checkpoint before clearing the pending marker.
    pub async fn run_persisted_explicit_agent_loop(
        &self,
        guard: &ExplicitAgentLoopGuard,
        gateway_credential: GatewayCredential<'_>,
        route_policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<PersistedExplicitAgentLoopOutcome> {
        validate_conversation_model_prompt(prompt)?;
        guard.begin_once()?;
        if guard.cancellation.is_requested() {
            return Err(VibeCoderError::Cancelled);
        }

        let capabilities = self.agent.ensure_ready().await?;
        if !capabilities.file_tools {
            return Err(VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "file_tools",
            });
        }
        if capabilities.command_tools {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_command_tools_must_remain_disabled".into(),
            ));
        }
        let bridge = self.agent.model_gateway_bridge_identity().ok_or(
            VibeCoderError::MissingCapability {
                component: "agent runtime",
                capability: "model_gateway_bridge",
            },
        )?;
        if !bridge.exact_model_id_passthrough {
            return Err(VibeCoderError::Gateway(
                "explicit_agent_loop_exact_model_bridge_required".into(),
            ));
        }

        let store = self.conversation_store()?;
        let (project, session_id, conversation, baseline_checkpoint) = {
            let _lifecycle = self.project_lifecycle_gate.try_acquire(guard.project_id)?;
            let project = self.workspace.open_project(guard.project_id).await?;
            self.workspace.verify_project(&project).await?;
            let mut conversation = store
                .load_conversation(guard.project_id, guard.conversation_id)
                .await?
                .ok_or_else(|| VibeCoderError::Persistence("conversation_not_found".into()))?;
            if conversation.session_creation_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_creation_incomplete".into(),
                ));
            }
            if conversation.turn_pending {
                return Err(VibeCoderError::Persistence(
                    "conversation_turn_recovery_required".into(),
                ));
            }
            let session = conversation.agent_session.as_ref().ok_or_else(|| {
                VibeCoderError::Persistence("conversation_session_not_persisted".into())
            })?;
            if session.runtime_id != self.agent.runtime_id() {
                return Err(VibeCoderError::Persistence(
                    "conversation_session_runtime_mismatch".into(),
                ));
            }
            ensure_conversation_agent_action_capacity(&conversation, prompt)?;
            let session_id = session.session_id.clone();
            self.agent.resume_session(&project, &session_id).await?;
            self.ensure_no_active_project_process(project.id)?;
            self.agent.ensure_workspace_quiescent(&project).await?;
            let checkpoint = self
                .checkpoint_store()?
                .create_checkpoint(&project, CheckpointReason::BeforeAgentChange)
                .await?;

            // Explicit loop intent is one durable user turn, not N synthetic user messages.
            conversation.append_message(ConversationRole::User, prompt.to_owned())?;
            conversation.turn_pending = true;
            let expected = conversation.revision;
            let conversation = match store.update_conversation(expected, &conversation).await {
                Ok(conversation) => conversation,
                Err(error) => {
                    if let Ok(checkpoints) = self.checkpoint_store() {
                        let _ = checkpoints
                            .remove_checkpoint(project.id, checkpoint.checkpoint_id)
                            .await;
                    }
                    return Err(error);
                }
            };
            (project, session_id, conversation, checkpoint)
        };

        let downstream = Arc::new(Mutex::new(on_event));
        let mut previous_tree_sha256 = baseline_checkpoint.tree_sha256.clone();
        let mut workspace_occurrences = HashMap::<String, u8>::new();
        workspace_occurrences.insert(previous_tree_sha256.clone(), 1);
        let mut turns_completed = 0u8;
        let mut total_tool_calls = 0usize;
        let mut total_successful_mutations = 0usize;

        for iteration in 1..=guard.policy.max_turns {
            if guard.cancellation.is_requested() {
                return self
                    .finish_persisted_explicit_agent_loop_non_success(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                        ExplicitAgentLoopStopReason::Cancelled,
                        turns_completed,
                        total_tool_calls,
                        total_successful_mutations,
                        &baseline_checkpoint.tree_sha256,
                    )
                    .await;
            }

            let iteration_prompt = match build_explicit_agent_loop_iteration_prompt(
                prompt,
                iteration,
                guard.policy.max_turns,
            ) {
                Ok(prompt) => prompt,
                Err(error) => {
                    self.recover_failed_persisted_explicit_agent_loop(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                    )
                    .await?;
                    return Err(error);
                }
            };
            let observation = Arc::new(Mutex::new(AgentActionObservation::default()));
            let observation_for_events = Arc::clone(&observation);
            let downstream_for_events = Arc::clone(&downstream);
            let iteration_event_handler: EventHandler = Box::new(move |event| {
                if let Ok(mut state) = observation_for_events.lock() {
                    state.observe(&event);
                }
                if let Ok(mut downstream) = downstream_for_events.lock()
                    && let Some(handler) = downstream.as_mut()
                {
                    let _ = catch_unwind(AssertUnwindSafe(|| handler(event)));
                }
            });

            let action = self
                .run_backend_task(
                    &project,
                    &session_id,
                    gateway_credential,
                    route_policy,
                    &iteration_prompt,
                    Some(iteration_event_handler),
                )
                .await;
            let outcome = match action {
                Ok(outcome) => outcome,
                Err(VibeCoderError::Cancelled) if guard.cancellation.is_requested() => {
                    return self
                        .finish_persisted_explicit_agent_loop_non_success(
                            store,
                            &project,
                            &conversation,
                            &session_id,
                            baseline_checkpoint.checkpoint_id,
                            None,
                            ExplicitAgentLoopStopReason::Cancelled,
                            turns_completed,
                            total_tool_calls,
                            total_successful_mutations,
                            &baseline_checkpoint.tree_sha256,
                        )
                        .await;
                }
                Err(error) => {
                    self.recover_failed_persisted_explicit_agent_loop(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                    )
                    .await?;
                    return Err(error);
                }
            };
            turns_completed = turns_completed.saturating_add(1);
            if guard.cancellation.is_requested() {
                return self
                    .finish_persisted_explicit_agent_loop_non_success(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                        ExplicitAgentLoopStopReason::Cancelled,
                        turns_completed,
                        total_tool_calls,
                        total_successful_mutations,
                        &baseline_checkpoint.tree_sha256,
                    )
                    .await;
            }

            let (decision, assistant_body) = match parse_explicit_agent_loop_response(&outcome.turn().text) {
                Ok(value) => value,
                Err(error) => {
                    self.recover_failed_persisted_explicit_agent_loop(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                    )
                    .await?;
                    return Err(error);
                }
            };
            let iteration_validation = match observation.lock() {
                Ok(state) => validate_agent_loop_iteration_turn(outcome.turn(), &state),
                Err(_) => Err(VibeCoderError::Agent(
                    "explicit_agent_loop_observation_poisoned".into(),
                )),
            };
            let (observed_tool_calls, _successful_file_tools, successful_mutations) =
                match iteration_validation {
                    Ok(value) => value,
                    Err(error) => {
                        self.recover_failed_persisted_explicit_agent_loop(
                            store,
                            &project,
                            &conversation,
                            &session_id,
                            baseline_checkpoint.checkpoint_id,
                            None,
                        )
                        .await?;
                        return Err(error);
                    }
                };
            total_tool_calls = total_tool_calls.saturating_add(observed_tool_calls);
            total_successful_mutations =
                total_successful_mutations.saturating_add(successful_mutations);


            let verification_checkpoint = match self
                .create_project_checkpoint(&project, CheckpointReason::AgentChangeVerification)
                .await
            {
                Ok(checkpoint) => checkpoint,
                Err(error) => {
                    self.recover_failed_persisted_explicit_agent_loop(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        None,
                    )
                    .await?;
                    return Err(error);
                }
            };
            let current_tree_sha256 = verification_checkpoint.tree_sha256.clone();

            if total_tool_calls > guard.policy.max_total_tool_calls {
                return self
                    .finish_persisted_explicit_agent_loop_non_success(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        Some(verification_checkpoint.checkpoint_id),
                        ExplicitAgentLoopStopReason::ToolBudgetExhausted,
                        turns_completed,
                        total_tool_calls,
                        total_successful_mutations,
                        &baseline_checkpoint.tree_sha256,
                    )
                    .await;
            }
            if guard.cancellation.is_requested() {
                return self
                    .finish_persisted_explicit_agent_loop_non_success(
                        store,
                        &project,
                        &conversation,
                        &session_id,
                        baseline_checkpoint.checkpoint_id,
                        Some(verification_checkpoint.checkpoint_id),
                        ExplicitAgentLoopStopReason::Cancelled,
                        turns_completed,
                        total_tool_calls,
                        total_successful_mutations,
                        &baseline_checkpoint.tree_sha256,
                    )
                    .await;
            }

            match decision {
                ExplicitAgentLoopDecision::Continue => {
                    if successful_mutations == 0 || current_tree_sha256 == previous_tree_sha256 {
                        self.recover_failed_persisted_explicit_agent_loop(
                            store,
                            &project,
                            &conversation,
                            &session_id,
                            baseline_checkpoint.checkpoint_id,
                            Some(verification_checkpoint.checkpoint_id),
                        )
                        .await?;
                        return Err(VibeCoderError::Agent(
                            "explicit_agent_loop_continue_without_workspace_progress".into(),
                        ));
                    }

                    let occurrence = workspace_occurrences
                        .entry(current_tree_sha256.clone())
                        .or_insert(0);
                    *occurrence = occurrence.saturating_add(1);
                    if *occurrence >= guard.policy.max_same_workspace_occurrences {
                        return self
                            .finish_persisted_explicit_agent_loop_non_success(
                                store,
                                &project,
                                &conversation,
                                &session_id,
                                baseline_checkpoint.checkpoint_id,
                                Some(verification_checkpoint.checkpoint_id),
                                ExplicitAgentLoopStopReason::RepeatedWorkspaceState,
                                turns_completed,
                                total_tool_calls,
                                total_successful_mutations,
                                &baseline_checkpoint.tree_sha256,
                            )
                            .await;
                    }
                    previous_tree_sha256 = current_tree_sha256;
                    let _ = self
                        .remove_project_checkpoint(project.id, verification_checkpoint.checkpoint_id)
                        .await;

                    if iteration == guard.policy.max_turns {
                        return self
                            .finish_persisted_explicit_agent_loop_non_success(
                                store,
                                &project,
                                &conversation,
                                &session_id,
                                baseline_checkpoint.checkpoint_id,
                                None,
                                ExplicitAgentLoopStopReason::TurnBudgetExhausted,
                                turns_completed,
                                total_tool_calls,
                                total_successful_mutations,
                                &baseline_checkpoint.tree_sha256,
                            )
                            .await;
                    }
                }
                ExplicitAgentLoopDecision::Complete => {
                    let mut completed = conversation.clone();
                    completed.turn_pending = false;
                    if let Err(error) = completed
                        .append_message(ConversationRole::Assistant, assistant_body.clone())
                    {
                        self.recover_failed_persisted_explicit_agent_loop(
                            store,
                            &project,
                            &conversation,
                            &session_id,
                            baseline_checkpoint.checkpoint_id,
                            Some(verification_checkpoint.checkpoint_id),
                        )
                        .await?;
                        return Err(error);
                    }
                    if let Err(error) = store
                        .update_conversation(conversation.revision, &completed)
                        .await
                    {
                        self.recover_failed_persisted_explicit_agent_loop(
                            store,
                            &project,
                            &conversation,
                            &session_id,
                            baseline_checkpoint.checkpoint_id,
                            Some(verification_checkpoint.checkpoint_id),
                        )
                        .await?;
                        return Err(error);
                    }
                    let verify_removed = self
                        .remove_project_checkpoint(project.id, verification_checkpoint.checkpoint_id)
                        .await
                        .is_ok();
                    let baseline_removed = self
                        .remove_project_checkpoint(project.id, baseline_checkpoint.checkpoint_id)
                        .await
                        .is_ok();
                    return Ok(PersistedExplicitAgentLoopOutcome {
                        stop_reason: ExplicitAgentLoopStopReason::Completed,
                        turns_completed,
                        total_tool_calls,
                        total_successful_mutations,
                        baseline_tree_sha256: baseline_checkpoint.tree_sha256.clone(),
                        final_tree_sha256: current_tree_sha256,
                        workspace_committed: true,
                        rollback_performed: false,
                        checkpoint_cleanup_complete: verify_removed && baseline_removed,
                        assistant_text: assistant_body,
                    });
                }
            }
        }

        Err(VibeCoderError::Agent(
            "explicit_agent_loop_unreachable_terminal_state".into(),
        ))
    }

    pub async fn run_persisted_explicit_agent_loop_resolved<R: SecretResolver>(
        &self,
        resolver: &R,
        credential_ref: Option<&SecretReference>,
        guard: &ExplicitAgentLoopGuard,
        route_policy: &ModelRoutePolicyConfig,
        prompt: &str,
        on_event: Option<EventHandler>,
    ) -> Result<PersistedExplicitAgentLoopOutcome> {
        let secret = resolve_optional_secret(resolver, credential_ref).await?;
        self.run_persisted_explicit_agent_loop(
            guard,
            gateway_credential_from_secret(secret.as_ref())?,
            route_policy,
            prompt,
            on_event,
        )
        .await
    }

    async fn recover_failed_persisted_explicit_agent_loop(
        &self,
        store: &Arc<dyn ConversationStore>,
        project: &ProjectRef,
        conversation: &PersistedConversation,
        session_id: &SessionId,
        baseline_checkpoint: CheckpointId,
        verification_checkpoint: Option<CheckpointId>,
    ) -> Result<()> {
        // The pending marker stays armed until the pre-loop workspace has been restored and all
        // persisted Jcode session bindings have been refreshed against that replacement tree.
        self.rollback_project_checkpoint_for_pending_conversation(
            project,
            baseline_checkpoint,
            conversation.conversation_id,
            session_id,
        )
        .await
        .map_err(|_| {
            VibeCoderError::Checkpoint(
                "explicit_agent_loop_rollback_failed_pending_preserved".into(),
            )
        })?;

        let mut cleared = conversation.clone();
        cleared.turn_pending = false;
        store
            .update_conversation(conversation.revision, &cleared)
            .await
            .map_err(|_| {
                VibeCoderError::Persistence(
                    "explicit_agent_loop_failure_cleanup_failed_pending_preserved".into(),
                )
            })?;

        if let Some(checkpoint_id) = verification_checkpoint {
            let _ = self
                .remove_project_checkpoint(project.id, checkpoint_id)
                .await;
        }
        let _ = self
            .remove_project_checkpoint(project.id, baseline_checkpoint)
            .await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_persisted_explicit_agent_loop_non_success(
        &self,
        store: &Arc<dyn ConversationStore>,
        project: &ProjectRef,
        conversation: &PersistedConversation,
        session_id: &SessionId,
        baseline_checkpoint: CheckpointId,
        verification_checkpoint: Option<CheckpointId>,
        stop_reason: ExplicitAgentLoopStopReason,
        turns_completed: u8,
        total_tool_calls: usize,
        total_successful_mutations: usize,
        baseline_tree_sha256: &str,
    ) -> Result<PersistedExplicitAgentLoopOutcome> {
        if stop_reason == ExplicitAgentLoopStopReason::Completed {
            return Err(VibeCoderError::InvalidRequest(
                "explicit_agent_loop_completed_cannot_use_rollback_finish".into(),
            ));
        }
        self.rollback_project_checkpoint_for_pending_conversation(
            project,
            baseline_checkpoint,
            conversation.conversation_id,
            session_id,
        )
        .await
        .map_err(|_| {
            VibeCoderError::Checkpoint(
                "explicit_agent_loop_terminal_rollback_failed_pending_preserved".into(),
            )
        })?;

        let assistant_text = explicit_agent_loop_stop_message(stop_reason).to_owned();
        let mut completed = conversation.clone();
        completed.turn_pending = false;
        completed.append_message(ConversationRole::Assistant, assistant_text.clone())?;
        store
            .update_conversation(conversation.revision, &completed)
            .await
            .map_err(|_| {
                VibeCoderError::Persistence(
                    "explicit_agent_loop_terminal_persistence_failed_pending_preserved".into(),
                )
            })?;

        let verification_removed = match verification_checkpoint {
            Some(checkpoint_id) => self
                .remove_project_checkpoint(project.id, checkpoint_id)
                .await
                .is_ok(),
            None => true,
        };
        let baseline_removed = self
            .remove_project_checkpoint(project.id, baseline_checkpoint)
            .await
            .is_ok();
        Ok(PersistedExplicitAgentLoopOutcome {
            stop_reason,
            turns_completed,
            total_tool_calls,
            total_successful_mutations,
            baseline_tree_sha256: baseline_tree_sha256.to_owned(),
            final_tree_sha256: baseline_tree_sha256.to_owned(),
            workspace_committed: false,
            rollback_performed: true,
            checkpoint_cleanup_complete: verification_removed && baseline_removed,
            assistant_text,
        })
    }

    fn project_state_store(&self) -> Result<&Arc<dyn ProjectStateStore>> {
        self.project_state_store
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "project state store",
                capability: "project_session_persistence",
            })
    }

    fn conversation_store(&self) -> Result<&Arc<dyn ConversationStore>> {
        self.conversation_store
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "conversation store",
                capability: "multi_chat_persistence",
            })
    }

    async fn recover_failed_persisted_agent_action(
        &self,
        store: &Arc<dyn ConversationStore>,
        project: &ProjectRef,
        conversation: &PersistedConversation,
        pre_action_checkpoint: CheckpointId,
        post_action_checkpoint: Option<CheckpointId>,
    ) -> Result<()> {
        // Keep the durable crash/recovery marker armed until the workspace has actually rolled
        // back. If rollback fails, another turn remains blocked without needing a second CAS.
        let pending_session_id = conversation
            .agent_session
            .as_ref()
            .ok_or_else(|| {
                VibeCoderError::Persistence(
                    "conversation_agent_action_recovery_session_missing".into(),
                )
            })?
            .session_id
            .clone();
        self.rollback_project_checkpoint_for_pending_conversation(
            project,
            pre_action_checkpoint,
            conversation.conversation_id,
            &pending_session_id,
        )
        .await
        .map_err(|_| {
            VibeCoderError::Checkpoint(
                "conversation_agent_action_rollback_failed_pending_preserved".into(),
            )
        })?;

        let mut cleared = conversation.clone();
        cleared.turn_pending = false;
        store
            .update_conversation(conversation.revision, &cleared)
            .await
            .map_err(|_| {
                VibeCoderError::Persistence(
                    "conversation_agent_action_failure_cleanup_failed_pending_preserved".into(),
                )
            })?;

        if let Some(checkpoint_id) = post_action_checkpoint {
            let _ = self
                .remove_project_checkpoint(project.id, checkpoint_id)
                .await;
        }
        let _ = self
            .remove_project_checkpoint(project.id, pre_action_checkpoint)
            .await;
        Ok(())
    }
}

fn validate_conversation_model_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_CONVERSATION_MODEL_ID_BYTES
        || value.trim() != value
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(VibeCoderError::InvalidRequest(
            "conversation_model_id_invalid".into(),
        ));
    }
    Ok(())
}

fn validate_conversation_model_prompt(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > MAX_CONVERSATION_MODEL_MESSAGE_BYTES
        || value.contains('\0')
    {
        return Err(VibeCoderError::InvalidRequest(
            "conversation_model_prompt_invalid".into(),
        ));
    }
    Ok(())
}

fn ensure_conversation_model_turn_capacity(
    conversation: &PersistedConversation,
    prompt: &str,
) -> Result<()> {
    if conversation.messages.len() > MAX_CONVERSATION_MESSAGES.saturating_sub(2) {
        return Err(VibeCoderError::Persistence(
            "conversation_model_turn_message_capacity_exhausted".into(),
        ));
    }
    let existing_bytes = conversation
        .messages
        .iter()
        .try_fold(0usize, |total, message| total.checked_add(message.text.len()))
        .ok_or_else(|| VibeCoderError::Persistence(
            "conversation_model_turn_text_capacity_exhausted".into(),
        ))?;
    let reserved = existing_bytes
        .checked_add(prompt.len())
        .and_then(|value| value.checked_add(MAX_CONVERSATION_MESSAGE_BYTES))
        .ok_or_else(|| VibeCoderError::Persistence(
            "conversation_model_turn_text_capacity_exhausted".into(),
        ))?;
    if reserved > MAX_CONVERSATION_TEXT_BYTES {
        return Err(VibeCoderError::Persistence(
            "conversation_model_turn_text_capacity_exhausted".into(),
        ));
    }
    Ok(())
}

fn ensure_conversation_agent_action_capacity(
    conversation: &PersistedConversation,
    prompt: &str,
) -> Result<()> {
    if conversation.messages.len() > MAX_CONVERSATION_MESSAGES.saturating_sub(2) {
        return Err(VibeCoderError::Persistence(
            "conversation_agent_action_message_capacity_exhausted".into(),
        ));
    }
    let existing_bytes = conversation
        .messages
        .iter()
        .try_fold(0usize, |total, message| total.checked_add(message.text.len()))
        .ok_or_else(|| {
            VibeCoderError::Persistence(
                "conversation_agent_action_text_capacity_exhausted".into(),
            )
        })?;
    let reserved = existing_bytes
        .checked_add(prompt.len())
        .and_then(|value| value.checked_add(MAX_CONVERSATION_MESSAGE_BYTES))
        .ok_or_else(|| {
            VibeCoderError::Persistence(
                "conversation_agent_action_text_capacity_exhausted".into(),
            )
        })?;
    if reserved > MAX_CONVERSATION_TEXT_BYTES {
        return Err(VibeCoderError::Persistence(
            "conversation_agent_action_text_capacity_exhausted".into(),
        ));
    }
    Ok(())
}

fn agent_action_tool_identity_is_safe(tool: &str, call_id: &str) -> bool {
    !tool.is_empty()
        && tool.trim() == tool
        && tool.len() <= 64
        && tool.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !call_id.is_empty()
        && call_id.trim() == call_id
        && call_id.len() <= MAX_AGENT_ACTION_CALL_ID_BYTES
        && !call_id.chars().any(char::is_control)
}

fn validate_agent_action_turn(
    turn: &TurnResult,
    observation: &AgentActionObservation,
) -> Result<(usize, usize, usize)> {
    let result = validate_agent_file_tool_turn(turn, observation)?;
    if result.2 == 0 {
        return Err(VibeCoderError::Agent(
            "agent_action_no_successful_mutation".into(),
        ));
    }
    Ok(result)
}

fn validate_agent_loop_iteration_turn(
    turn: &TurnResult,
    observation: &AgentActionObservation,
) -> Result<(usize, usize, usize)> {
    validate_agent_file_tool_turn(turn, observation)
}

fn validate_agent_file_tool_turn(
    turn: &TurnResult,
    observation: &AgentActionObservation,
) -> Result<(usize, usize, usize)> {
    if observation.protocol_failure {
        return Err(VibeCoderError::Agent(
            "agent_action_tool_event_protocol_invalid".into(),
        ));
    }
    if turn.text.trim().is_empty()
        || turn.text.len() > MAX_CONVERSATION_MESSAGE_BYTES
        || turn.text.contains('\0')
    {
        return Err(VibeCoderError::Agent(
            "agent_action_final_response_invalid".into(),
        ));
    }
    if turn.tool_calls.is_empty() || turn.tool_calls.len() > MAX_AGENT_ACTION_TOOL_CALLS {
        return Err(VibeCoderError::Agent(
            "agent_action_tool_count_invalid".into(),
        ));
    }
    if observation.calls.len() != turn.tool_calls.len() {
        return Err(VibeCoderError::Agent(
            "agent_action_tool_transcript_count_mismatch".into(),
        ));
    }

    let mut turn_call_ids = HashSet::new();
    let mut successful_file_tool_calls = 0usize;
    let mut successful_mutation_tool_calls = 0usize;
    for call in &turn.tool_calls {
        if !agent_action_tool_identity_is_safe(&call.tool, &call.call_id)
            || !AGENT_ACTION_FILE_TOOLS.contains(&call.tool.as_str())
            || !turn_call_ids.insert(call.call_id.as_str())
        {
            return Err(VibeCoderError::Agent(
                "agent_action_tool_result_identity_invalid".into(),
            ));
        }
        let observed = observation.calls.get(&call.call_id).ok_or_else(|| {
            VibeCoderError::Agent("agent_action_tool_result_unobserved".into())
        })?;
        let result_ok = call.error.is_none();
        if observed.tool != call.tool || !observed.finished || observed.ok != result_ok {
            return Err(VibeCoderError::Agent(
                "agent_action_tool_result_event_mismatch".into(),
            ));
        }
        if result_ok {
            successful_file_tool_calls += 1;
            if AGENT_ACTION_MUTATION_TOOLS.contains(&call.tool.as_str()) {
                successful_mutation_tool_calls += 1;
            }
        }
    }
    if successful_file_tool_calls == 0 {
        return Err(VibeCoderError::Agent(
            "agent_action_no_successful_file_tool".into(),
        ));
    }
    Ok((
        observation.calls.len(),
        successful_file_tool_calls,
        successful_mutation_tool_calls,
    ))
}

fn build_explicit_agent_loop_iteration_prompt(
    original_prompt: &str,
    iteration: u8,
    max_turns: u8,
) -> Result<String> {
    if iteration == 0 || iteration > max_turns || max_turns == 0 {
        return Err(VibeCoderError::InvalidRequest(
            "explicit_agent_loop_iteration_invalid".into(),
        ));
    }
    let prompt = format!(
        "{original_prompt}\n\n[VibeCoder explicit loop control]\n\
This loop was explicitly requested by the user. Iteration {iteration} of at most {max_turns}. \
Inspect the current workspace with the allowed file tools before deciding. Continue working only \
if the original task is not yet complete. Do not use shell, browser, MCP, or external tools.\n\
Your final non-empty line MUST be exactly one of:\n\
{EXPLICIT_AGENT_LOOP_COMPLETE_MARKER}\n\
{EXPLICIT_AGENT_LOOP_CONTINUE_MARKER}\n\
Use `complete` only when the original task is satisfied based on the evidence available through \
the allowed file tools. Use `continue` only when another file-mutation iteration is genuinely \
required. Put the user-facing explanation before the marker."
    );
    if prompt.len() > 1024 * 1024 || prompt.contains('\0') {
        return Err(VibeCoderError::InvalidRequest(
            "explicit_agent_loop_iteration_prompt_too_large".into(),
        ));
    }
    Ok(prompt)
}

fn parse_explicit_agent_loop_response(
    text: &str,
) -> Result<(ExplicitAgentLoopDecision, String)> {
    if text.is_empty() || text.len() > MAX_CONVERSATION_MESSAGE_BYTES || text.contains('\0') {
        return Err(VibeCoderError::Agent(
            "explicit_agent_loop_response_invalid".into(),
        ));
    }
    let trimmed = text.trim_end();
    let (body, decision) = if let Some(body) = trimmed.strip_suffix(EXPLICIT_AGENT_LOOP_COMPLETE_MARKER)
    {
        (body, ExplicitAgentLoopDecision::Complete)
    } else if let Some(body) = trimmed.strip_suffix(EXPLICIT_AGENT_LOOP_CONTINUE_MARKER) {
        (body, ExplicitAgentLoopDecision::Continue)
    } else {
        return Err(VibeCoderError::Agent(
            "explicit_agent_loop_status_marker_missing".into(),
        ));
    };
    let body = body
        .trim_end_matches(|character: char| matches!(character, '\r' | '\n' | ' ' | '\t'))
        .to_owned();
    if body.trim().is_empty()
        || body.contains(EXPLICIT_AGENT_LOOP_COMPLETE_MARKER)
        || body.contains(EXPLICIT_AGENT_LOOP_CONTINUE_MARKER)
        || body.len() > MAX_CONVERSATION_MESSAGE_BYTES
    {
        return Err(VibeCoderError::Agent(
            "explicit_agent_loop_response_body_invalid".into(),
        ));
    }
    Ok((decision, body))
}

fn explicit_agent_loop_stop_message(reason: ExplicitAgentLoopStopReason) -> &'static str {
    match reason {
        ExplicitAgentLoopStopReason::Completed => "Explicit loop completed.",
        ExplicitAgentLoopStopReason::Cancelled => {
            "Explicit loop cancelled. Workspace restored to the pre-loop checkpoint."
        }
        ExplicitAgentLoopStopReason::TurnBudgetExhausted => {
            "Explicit loop stopped at its turn limit. Workspace restored to the pre-loop checkpoint."
        }
        ExplicitAgentLoopStopReason::ToolBudgetExhausted => {
            "Explicit loop stopped at its tool-call limit. Workspace restored to the pre-loop checkpoint."
        }
        ExplicitAgentLoopStopReason::RepeatedWorkspaceState => {
            "Explicit loop stopped after repeating a workspace state. Workspace restored to the pre-loop checkpoint."
        }
    }
}

fn select_exact_gateway_model(catalog: &[ModelRef], requested_model_id: &str) -> Result<ModelRef> {
    let mut matches = catalog
        .iter()
        .filter(|candidate| candidate.id == requested_model_id);
    let model = matches
        .next()
        .ok_or_else(|| VibeCoderError::Gateway("conversation_model_not_in_catalog".into()))?;
    if matches.next().is_some() {
        return Err(VibeCoderError::Gateway(
            "conversation_model_catalog_ambiguous".into(),
        ));
    }
    Ok(model.clone())
}

fn build_gateway_conversation_context(
    conversation: &PersistedConversation,
) -> Result<(Vec<GatewayChatMessage>, usize)> {
    let mut reversed = Vec::new();
    let mut total_bytes = 0usize;
    for message in conversation.messages.iter().rev() {
        if reversed.len() == MAX_CONVERSATION_MODEL_MESSAGES
            || message.text.len() > MAX_CONVERSATION_MODEL_MESSAGE_BYTES
        {
            break;
        }
        let next_total = total_bytes
            .checked_add(message.text.len())
            .ok_or_else(|| VibeCoderError::InvalidRequest(
                "conversation_model_context_too_large".into(),
            ))?;
        if next_total > MAX_CONVERSATION_MODEL_CONTEXT_BYTES {
            break;
        }
        total_bytes = next_total;
        reversed.push(GatewayChatMessage {
            role: match message.role {
                ConversationRole::User => GatewayChatRole::User,
                ConversationRole::Assistant => GatewayChatRole::Assistant,
            },
            content: message.text.clone(),
        });
    }
    if reversed.is_empty() {
        return Err(VibeCoderError::InvalidRequest(
            "conversation_model_context_empty".into(),
        ));
    }
    reversed.reverse();
    if matches!(reversed.first().map(|message| message.role), Some(GatewayChatRole::Assistant)) {
        let removed = reversed.remove(0);
        total_bytes = total_bytes.saturating_sub(removed.content.len());
    }
    if reversed.is_empty()
        || !matches!(reversed.last().map(|message| message.role), Some(GatewayChatRole::User))
    {
        return Err(VibeCoderError::Persistence(
            "conversation_model_context_latest_user_missing".into(),
        ));
    }
    Ok((reversed, total_bytes))
}

async fn await_conversation_model_inference_or_cancel<F, T>(
    inference: F,
    cancellation: &ConversationModelTurnCancellation,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let mut inference = Box::pin(inference);
    loop {
        if cancellation.is_requested() {
            return Err(VibeCoderError::Cancelled);
        }
        match tokio::time::timeout(Duration::from_millis(50), inference.as_mut()).await {
            Ok(result) => return result,
            Err(_) => continue,
        }
    }
}

fn validate_conversation_model_response(
    model: &ModelRef,
    response: &GatewayChatResponse,
) -> Result<()> {
    if response.requested_model_id != model.id {
        return Err(VibeCoderError::Gateway(
            "conversation_model_response_request_mismatch".into(),
        ));
    }
    if response
        .observed_model_id
        .as_deref()
        .is_some_and(|observed| observed != model.id)
    {
        return Err(VibeCoderError::Gateway(
            "conversation_model_response_identity_mismatch".into(),
        ));
    }
    if response.text.is_empty()
        || response.text.len() > MAX_CONVERSATION_MESSAGE_BYTES
        || response.text.contains('\0')
    {
        return Err(VibeCoderError::Gateway(
            "conversation_model_response_text_invalid".into(),
        ));
    }
    Ok(())
}

fn require_deterministic_gateway_profile(profile: &GatewayExecutionProfile) -> Result<()> {
    if profile.gateway_id.is_empty()
        || profile.upstream_version.is_empty()
        || profile.profile_id.is_empty()
        || profile.profile_sha256.len() != 64
        || !profile
            .profile_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !profile.permits_exact_model_execution()
    {
        return Err(VibeCoderError::Gateway(
            "deterministic_gateway_runtime_profile_required".into(),
        ));
    }
    Ok(())
}

fn require_agent_gateway_bridge_matches_profile(
    bridge: &ModelGatewayBridgeIdentity,
    profile: &GatewayExecutionProfile,
) -> Result<()> {
    if bridge.gateway_id != profile.gateway_id
        || bridge.transport_provider.is_empty()
        || bridge.transport_provider.trim() != bridge.transport_provider
        || bridge.transport_provider.chars().any(char::is_control)
        || !bridge.exact_model_id_passthrough
    {
        return Err(VibeCoderError::Gateway(
            "agent_model_gateway_bridge_profile_mismatch".into(),
        ));
    }
    Ok(())
}

async fn resolve_optional_secret<R: SecretResolver>(
    resolver: &R,
    reference: Option<&SecretReference>,
) -> Result<Option<SecretValue>> {
    match reference {
        Some(reference) => resolver.resolve(reference).await.map(Some),
        None => Ok(None),
    }
}

fn gateway_credential_from_secret(secret: Option<&SecretValue>) -> Result<GatewayCredential<'_>> {
    match secret {
        Some(secret) => Ok(GatewayCredential::Secret(secret.expose_str()?)),
        None => Ok(GatewayCredential::Anonymous),
    }
}
#[cfg(test)]
mod part17_lifecycle_tests {
    use super::*;

    #[test]
    fn project_lifecycle_gate_rejects_overlap_and_releases_on_drop() {
        let gate = ProjectLifecycleGate::default();
        let project = ProjectId::new();
        let permit = gate
            .try_acquire(project)
            .expect("first project lifecycle permit");
        assert!(gate.try_acquire(project).is_err());
        drop(permit);
        assert!(gate.try_acquire(project).is_ok());
    }

    #[test]
    fn project_lifecycle_gate_is_project_scoped() {
        let gate = ProjectLifecycleGate::default();
        let first = ProjectId::new();
        let second = ProjectId::new();
        let _first = gate
            .try_acquire(first)
            .expect("first project lifecycle permit");
        assert!(gate.try_acquire(second).is_ok());
    }
}

#[cfg(test)]
mod part34_6_controller_tests {
    use super::*;

    #[test]
    fn model_context_keeps_recent_contiguous_history_and_ends_with_user() {
        let mut conversation = PersistedConversation::pending_creation(
            ConversationId::new(),
            ProjectId::new(),
        );
        conversation
            .append_message(ConversationRole::Assistant, "orphan assistant".into())
            .unwrap();
        conversation
            .append_message(ConversationRole::User, "first user".into())
            .unwrap();
        conversation
            .append_message(ConversationRole::Assistant, "first answer".into())
            .unwrap();
        conversation
            .append_message(ConversationRole::User, "latest user".into())
            .unwrap();
        let (messages, bytes) = build_gateway_conversation_context(&conversation).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, GatewayChatRole::User);
        assert_eq!(messages.last().unwrap().role, GatewayChatRole::User);
        assert_eq!(messages.last().unwrap().content, "latest user");
        assert_eq!(
            bytes,
            "first user".len() + "first answer".len() + "latest user".len()
        );
    }

    #[test]
    fn model_response_identity_is_exact_and_persistable() {
        let model = ModelRef {
            id: "provider/model".into(),
            display_name: None,
            provider: Some("provider".into()),
        };
        let good = GatewayChatResponse {
            requested_model_id: model.id.clone(),
            observed_model_id: Some(model.id.clone()),
            text: "hello".into(),
            finish_reason: Some("stop".into()),
            usage: None,
        };
        validate_conversation_model_response(&model, &good).unwrap();
        let mut bad = good;
        bad.observed_model_id = Some("provider/other".into());
        assert!(validate_conversation_model_response(&model, &bad).is_err());
    }

    #[test]
    fn model_turn_outcome_debug_redacts_assistant_text() {
        let outcome = ConversationModelTurnOutcome {
            model: ModelRef {
                id: "provider/model".into(),
                display_name: None,
                provider: Some("provider".into()),
            },
            observed_model_id: Some("provider/model".into()),
            finish_reason: Some("stop".into()),
            usage: None,
            assistant_text: "sensitive assistant answer".into(),
            context_messages_sent: 1,
            context_bytes_sent: 4,
        };
        let debug = format!("{outcome:?}");
        assert!(!debug.contains("sensitive assistant answer"));
        assert!(debug.contains("REDACTED"));
    }
}

#[cfg(test)]
mod part34_8_agent_action_tests {
    use super::*;
    use vibecoder_domain::ToolCallResult;

    fn observed_success(tool: &str, call_id: &str) -> AgentActionObservation {
        let mut observation = AgentActionObservation::default();
        observation.observe(&AgentEvent::ToolStarted {
            tool: tool.into(),
            call_id: call_id.into(),
        });
        observation.observe(&AgentEvent::ToolFinished {
            tool: tool.into(),
            call_id: call_id.into(),
            ok: true,
            output: "ok".into(),
            error: None,
        });
        observation
    }

    #[test]
    fn action_acceptance_requires_successful_mutation() {
        let observation = observed_success("edit", "call-1");
        let turn = TurnResult {
            text: "Updated the file.".into(),
            cancelled: false,
            tool_calls: vec![ToolCallResult {
                call_id: "call-1".into(),
                tool: "edit".into(),
                output: "ok".into(),
                error: None,
            }],
            usage: None,
        };
        assert_eq!(validate_agent_action_turn(&turn, &observation).unwrap(), (1, 1, 1));
    }

    #[test]
    fn read_only_turn_is_not_an_action_acceptance_success() {
        let observation = observed_success("read", "call-1");
        let turn = TurnResult {
            text: "Read the file.".into(),
            cancelled: false,
            tool_calls: vec![ToolCallResult {
                call_id: "call-1".into(),
                tool: "read".into(),
                output: "contents".into(),
                error: None,
            }],
            usage: None,
        };
        assert!(validate_agent_action_turn(&turn, &observation).is_err());
    }

    #[test]
    fn transcript_mismatch_fails_closed() {
        let observation = observed_success("edit", "call-1");
        let turn = TurnResult {
            text: "Updated the file.".into(),
            cancelled: false,
            tool_calls: vec![ToolCallResult {
                call_id: "call-2".into(),
                tool: "edit".into(),
                output: "ok".into(),
                error: None,
            }],
            usage: None,
        };
        assert!(validate_agent_action_turn(&turn, &observation).is_err());
    }
}

#[cfg(test)]
mod part34_9_explicit_agent_loop_tests {
    use super::*;

    #[test]
    fn explicit_loop_policy_is_bounded() {
        assert!(ExplicitAgentLoopPolicy::default().validate().is_ok());
        assert!(
            ExplicitAgentLoopPolicy {
                max_turns: MAX_EXPLICIT_AGENT_LOOP_TURNS + 1,
                ..ExplicitAgentLoopPolicy::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ExplicitAgentLoopPolicy {
                max_total_tool_calls: MAX_EXPLICIT_AGENT_LOOP_TOTAL_TOOL_CALLS + 1,
                ..ExplicitAgentLoopPolicy::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn explicit_loop_response_requires_one_terminal_marker() {
        let (decision, body) = parse_explicit_agent_loop_response(
            "I inspected the project and the requested change is complete.\nVIBECODER_LOOP_STATUS=complete",
        )
        .unwrap();
        assert_eq!(decision, ExplicitAgentLoopDecision::Complete);
        assert_eq!(body, "I inspected the project and the requested change is complete.");

        assert!(
            parse_explicit_agent_loop_response("Done without a machine marker.").is_err()
        );
        assert!(
            parse_explicit_agent_loop_response(
                "VIBECODER_LOOP_STATUS=continue\nextra\nVIBECODER_LOOP_STATUS=complete"
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_loop_iteration_prompt_is_machine_bounded() {
        let prompt = build_explicit_agent_loop_iteration_prompt("Fix the file.", 1, 4).unwrap();
        assert!(prompt.contains("explicitly requested by the user"));
        assert!(prompt.contains(EXPLICIT_AGENT_LOOP_COMPLETE_MARKER));
        assert!(prompt.contains(EXPLICIT_AGENT_LOOP_CONTINUE_MARKER));
        assert!(prompt.contains("Do not use shell, browser, MCP, or external tools."));
    }

    #[test]
    fn explicit_loop_guard_is_one_shot_and_cancellable() {
        let guard = ExplicitAgentLoopGuard::new(
            ProjectId::new(),
            ConversationId::new(),
            ExplicitAgentLoopPolicy::default(),
        )
        .unwrap();
        assert!(guard.begin_once().is_ok());
        assert!(guard.begin_once().is_err());
        let cancellation = guard.cancellation();
        assert!(!cancellation.is_requested());
        cancellation.request();
        assert!(guard.cancellation().is_requested());
    }
}

