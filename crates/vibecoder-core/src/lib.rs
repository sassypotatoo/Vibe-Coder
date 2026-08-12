//! Application orchestration layer. Provider-specific adapters stay outside this crate.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use vibecoder_agent_contract::{AgentRuntime, CreateSessionOptions, EventHandler, RunTurnOptions};
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
    ModelRef, ProjectId, ProjectRef, Result, RuntimeCapabilities, SessionId, TurnResult,
    VibeCoderError,
};
use vibecoder_gateway_contract::{
    GatewayCredential, GatewayExecutionProfile, GatewayHealth, ModelGateway,
};
use vibecoder_persistence_contract::{
    PersistedAgentSession, PersistedProjectState, PersistenceCapabilities, ProjectStateStore,
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
            if !task.corroborate_agent_catalog(&agent_catalog)? {
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
        store.remove_project_state(project.id).await
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

    fn project_state_store(&self) -> Result<&Arc<dyn ProjectStateStore>> {
        self.project_state_store
            .as_ref()
            .ok_or(VibeCoderError::MissingCapability {
                component: "project state store",
                capability: "project_session_persistence",
            })
    }
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
