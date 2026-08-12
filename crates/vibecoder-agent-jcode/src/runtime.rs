use crate::error::map_operation_error;
use crate::model::{
    ModelCapabilityRegistry, discover_models, select_model_from_catalog, validate_model_ref,
    verify_active_model, wait_for_fresh_model_probe,
};
use crate::permission::PermissionRegistry;
use crate::session::{
    SessionBinding, SessionRegistry, canonical_project_root, corroborate_new_session_project,
    session_metadata, validate_jcode_session_id, verify_attached_session_id,
    verify_session_project,
};
use crate::turn::{ActiveTurnLease, TurnRegistry, map_turn_result, run_options};
use crate::{
    JcodeConnectionConfig, JcodeConnectionManager, JcodeConnectionSnapshot, JcodeConnectionState,
};
use async_trait::async_trait;
use futures_channel::oneshot;
use std::path::Path;
use std::sync::Arc;
use vibecoder_agent_contract::{AgentRuntime, CreateSessionOptions, EventHandler, RunTurnOptions};
use vibecoder_domain::{
    ModelRef, PermissionDecision, ProjectRef, Result, RuntimeCapabilities, SessionId, TurnResult,
    VibeCoderError,
};

/// VibeCoder's provider-neutral runtime adapter backed by Jcode's public harness SDK.
///
/// Parts 2-6 implement transport/session lifecycle, turn streaming, permission mediation, and
/// verified session-scoped model discovery/selection.
pub struct JcodeAgentRuntime {
    connection: JcodeConnectionManager,
    sessions: SessionRegistry,
    turns: Arc<TurnRegistry>,
    permissions: Arc<PermissionRegistry>,
    models: ModelCapabilityRegistry,
}

impl JcodeAgentRuntime {
    pub fn new(config: JcodeConnectionConfig) -> Result<Self> {
        Ok(Self {
            connection: JcodeConnectionManager::new(config)?,
            sessions: SessionRegistry::new(),
            turns: Arc::new(TurnRegistry::new()),
            permissions: Arc::new(PermissionRegistry::new()),
            models: ModelCapabilityRegistry::new(),
        })
    }

    pub fn connect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot connect/recover Jcode while a turn is active; cancel the turn first".into(),
            ));
        }
        self.connection.connect()
    }

    pub fn disconnect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot disconnect Jcode while a turn is active; cancel the turn first".into(),
            ));
        }
        let snapshot = self.connection.disconnect()?;
        self.sessions.clear_attachment()?;
        Ok(snapshot)
    }

    pub fn reconnect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot reconnect Jcode while a turn is active; cancel the turn first".into(),
            ));
        }
        let snapshot = self.connection.reconnect()?;
        self.sessions.clear_attachment()?;
        Ok(snapshot)
    }

    pub fn connection_status(&self) -> Result<JcodeConnectionSnapshot> {
        self.connection.status()
    }

    fn capabilities_from_snapshot(
        &self,
        snapshot: &JcodeConnectionSnapshot,
    ) -> RuntimeCapabilities {
        let JcodeConnectionState::Connected { identity } = &snapshot.state else {
            return RuntimeCapabilities::none();
        };
        RuntimeCapabilities {
            sessions: identity
                .capabilities
                .iter()
                .any(|value| value == "sessions"),
            streaming_events: identity
                .capabilities
                .iter()
                .any(|value| value == "streaming"),
            permissions: identity
                .capabilities
                .iter()
                .any(|value| value == "permissions"),
            model_selection: self.models.is_verified(snapshot.generation),
            file_tools: false,
            command_tools: false,
        }
    }

    fn ensure_session_transport(&self) -> Result<u64> {
        let snapshot = self.connection.connect()?;
        match snapshot.state {
            JcodeConnectionState::Connected { identity } => {
                if !identity
                    .capabilities
                    .iter()
                    .any(|value| value == "sessions")
                {
                    return Err(VibeCoderError::MissingCapability {
                        component: "Jcode harness",
                        capability: "sessions",
                    });
                }
                Ok(snapshot.generation)
            }
            JcodeConnectionState::Disconnected
            | JcodeConnectionState::Connecting
            | JcodeConnectionState::Faulted { .. } => Err(VibeCoderError::Agent(
                "Jcode session operation requires a healthy connected harness".into(),
            )),
        }
    }

    fn reset_unverified_attachment(&self) {
        // If Jcode ever reports a different session/root than requested, do not keep using that
        // attachment. Closing the SDK connection is a local fail-closed action; it does not delete
        // the persisted upstream session.
        let _ = self.connection.disconnect();
        let _ = self.sessions.clear_attachment();
    }

    fn attach_verified(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
        expected_root: &Path,
        generation: u64,
    ) -> Result<()> {
        let verification = self.connection.with_client(|client| {
            let attached = client
                .attach_session(&session_id.0)
                .map_err(|error| map_operation_error("resume_session", error))?;
            let actual_id = verify_attached_session_id(&attached, Some(session_id))?;
            let sessions = client
                .list_sessions()
                .map_err(|error| map_operation_error("resume_session_metadata", error))?;
            let metadata = session_metadata(&sessions, &actual_id)?;
            verify_session_project(metadata, expected_root)?;
            Ok(actual_id)
        });

        match verification {
            Ok(actual_id) => self.sessions.mark_attached(
                &actual_id,
                project,
                expected_root.to_path_buf(),
                generation,
            ),
            Err(error) => {
                self.reset_unverified_attachment();
                Err(error)
            }
        }
    }

    fn ensure_bound_session_attached(
        &self,
        session_id: &SessionId,
        binding: &SessionBinding,
        generation: u64,
    ) -> Result<()> {
        if self
            .sessions
            .is_attached_on_generation(session_id, generation)?
        {
            return Ok(());
        }

        let root = std::fs::canonicalize(&binding.project_root).map_err(|_| {
            VibeCoderError::InvalidRequest(
                "bound project root is no longer accessible; resume the session explicitly".into(),
            )
        })?;
        if root != binding.project_root {
            return Err(VibeCoderError::InvalidRequest(
                "bound project root identity changed; resume the session explicitly".into(),
            ));
        }

        let verification = self.connection.with_client(|client| {
            let attached = client
                .attach_session(&session_id.0)
                .map_err(|error| map_operation_error("session_reattach", error))?;
            let actual_id = verify_attached_session_id(&attached, Some(session_id))?;
            let sessions = client
                .list_sessions()
                .map_err(|error| map_operation_error("session_reattach_metadata", error))?;
            let metadata = session_metadata(&sessions, &actual_id)?;
            verify_session_project(metadata, &binding.project_root)?;
            Ok(actual_id)
        });
        match verification {
            Ok(actual_id) => {
                let synthetic_project = ProjectRef {
                    id: binding.project_id,
                    root: binding.project_root.clone(),
                };
                self.sessions.mark_attached(
                    &actual_id,
                    &synthetic_project,
                    binding.project_root.clone(),
                    generation,
                )
            }
            Err(error) => {
                self.reset_unverified_attachment();
                Err(error)
            }
        }
    }

    /// Open a fresh sidecar API connection and verify it against the target session.
    ///
    /// The manager-owned connection must remain alive: in default private mode it owns an
    /// ephemeral JCODE_HOME, so reconnecting/dropping it would delete the private runtime state.
    /// A second connection to the same live socket gets a fresh server-side BridgeState without
    /// disturbing the owner. Subscribing before attach then waiting for target-session `ModelInfo`
    /// proves the sidecar has processed its fresh model probe before its cache is authorized.
    fn open_fresh_model_client(
        &self,
        session_id: &SessionId,
        binding: &SessionBinding,
    ) -> Result<jcode_sdk::JcodeClient> {
        let root = std::fs::canonicalize(&binding.project_root).map_err(|_| {
            VibeCoderError::InvalidRequest(
                "bound project root is no longer accessible; resume the session explicitly".into(),
            )
        })?;
        if root != binding.project_root {
            return Err(VibeCoderError::InvalidRequest(
                "bound project root identity changed; resume the session explicitly".into(),
            ));
        }

        let timeout = self.connection.config().request_timeout();
        let client = self.connection.open_clean_model_client()?;
        let events = client.events(Some(&session_id.0));
        let attached = client
            .attach_session(&session_id.0)
            .map_err(|error| map_operation_error("model_session_refresh", error))?;
        let actual_id = verify_attached_session_id(&attached, Some(session_id))?;
        let sessions = client
            .list_sessions()
            .map_err(|error| map_operation_error("model_session_refresh_metadata", error))?;
        let metadata = session_metadata(&sessions, &actual_id)?;
        verify_session_project(metadata, &binding.project_root)?;
        wait_for_fresh_model_probe(&events, &actual_id, timeout)?;
        Ok(client)
    }

    fn verify_transport_generation(&self, expected: u64) -> Result<()> {
        let snapshot = self.connection.status()?;
        match snapshot.state {
            JcodeConnectionState::Connected { .. } if snapshot.generation == expected => Ok(()),
            _ => Err(VibeCoderError::Agent(
                "Jcode owner transport changed during model discovery".into(),
            )),
        }
    }
}

#[async_trait]
impl AgentRuntime for JcodeAgentRuntime {
    fn runtime_id(&self) -> &'static str {
        "jcode-harness"
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        self.connection
            .status()
            .map(|snapshot| self.capabilities_from_snapshot(&snapshot))
            .unwrap_or_else(|_| RuntimeCapabilities::none())
    }

    async fn ensure_ready(&self) -> Result<RuntimeCapabilities> {
        let snapshot = self.connect()?;
        let capabilities = self.capabilities_from_snapshot(&snapshot);
        if !capabilities.sessions {
            return Err(VibeCoderError::MissingCapability {
                component: "Jcode harness",
                capability: "sessions",
            });
        }
        Ok(capabilities)
    }

    async fn create_session(
        &self,
        project: &ProjectRef,
        options: CreateSessionOptions,
    ) -> Result<SessionId> {
        if options.model.is_some() {
            return Err(VibeCoderError::InvalidRequest(
                "Jcode model selection is session-scoped and not atomic with session creation; create the session, then select its model".into(),
            ));
        }

        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot create a Jcode session while a turn is active".into(),
            ));
        }
        let expected_root = canonical_project_root(project)?;
        let generation = self.ensure_session_transport()?;
        let working_dir = expected_root
            .to_str()
            .ok_or_else(|| {
                VibeCoderError::InvalidRequest(
                    "project root must be valid UTF-8 for the Jcode harness".into(),
                )
            })?
            .to_string();

        let verification = self.connection.with_client(|client| {
            let attached = client
                .create_session(Some(working_dir))
                .map_err(|error| map_operation_error("create_session", error))?;
            let session_id = verify_attached_session_id(&attached, None)?;
            let sessions = client
                .list_sessions()
                .map_err(|error| map_operation_error("create_session_metadata", error))?;
            corroborate_new_session_project(&sessions, &session_id, &expected_root)?;
            Ok(session_id)
        });

        match verification {
            Ok(session_id) => {
                self.sessions
                    .mark_attached(&session_id, project, expected_root, generation)?;
                Ok(session_id)
            }
            Err(error) => {
                self.reset_unverified_attachment();
                Err(error)
            }
        }
    }

    async fn resume_session(&self, project: &ProjectRef, session_id: &SessionId) -> Result<()> {
        validate_jcode_session_id(session_id)?;
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot resume a Jcode session while a turn is active".into(),
            ));
        }
        let expected_root = canonical_project_root(project)?;
        let generation = self.ensure_session_transport()?;

        if let Some(binding) = self.sessions.binding(session_id)? {
            if binding.project_id != project.id || binding.project_root != expected_root {
                return Err(VibeCoderError::InvalidRequest(
                    "session is already bound to a different project".into(),
                ));
            }
            if self
                .sessions
                .is_attached_on_generation(session_id, generation)?
            {
                return Ok(());
            }
        }

        self.attach_verified(project, session_id, &expected_root, generation)
    }

    async fn verify_session_project_binding(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        validate_jcode_session_id(session_id)?;
        let expected_root = canonical_project_root(project)?;
        let binding = self.sessions.binding(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "Jcode session has no verified project binding in this app process".into(),
            )
        })?;
        if binding.project_id != project.id || binding.project_root != expected_root {
            return Err(VibeCoderError::InvalidRequest(
                "Jcode session is bound to a different project".into(),
            ));
        }
        let snapshot = self.connection.status()?;
        let generation = match snapshot.state {
            JcodeConnectionState::Connected { .. } => snapshot.generation,
            _ => {
                return Err(VibeCoderError::Agent(
                    "Jcode session project binding cannot be authorized without an active connection".into(),
                ));
            }
        };
        if binding.connection_generation != generation
            || !self
                .sessions
                .is_attached_on_generation(session_id, generation)?
        {
            return Err(VibeCoderError::Agent(
                "Jcode session project binding is stale on the current connection generation"
                    .into(),
            ));
        }
        Ok(())
    }

    async fn ensure_workspace_quiescent(&self, project: &ProjectRef) -> Result<()> {
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "workspace checkpoint/rollback requires Jcode to have no active turn".into(),
            ));
        }
        let _ = canonical_project_root(project)?;
        Ok(())
    }

    async fn refresh_session_after_workspace_replacement(
        &self,
        project: &ProjectRef,
        session_id: &SessionId,
    ) -> Result<()> {
        validate_jcode_session_id(session_id)?;
        let expected_root = canonical_project_root(project)?;
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot refresh Jcode workspace identity while a turn is active".into(),
            ));
        }
        let generation = self.ensure_session_transport()?;
        // Force a real attach/list_sessions corroboration even when the path string is unchanged.
        // Rollback atomically replaces directory identity, so cached attachment state is not proof.
        self.sessions.clear_attachment()?;
        self.attach_verified(project, session_id, &expected_root, generation)
    }

    async fn run_turn(
        &self,
        session_id: &SessionId,
        prompt: &str,
        options: RunTurnOptions,
        on_event: Option<EventHandler>,
    ) -> Result<TurnResult> {
        validate_jcode_session_id(session_id)?;
        if prompt.trim().is_empty() {
            return Err(VibeCoderError::InvalidRequest(
                "agent turn prompt cannot be empty".into(),
            ));
        }
        if let Some(model) = options.model.as_ref() {
            validate_model_ref(model)?;
        }

        // Serialize only attachment/startup. The blocking model turn itself must not hold this gate,
        // otherwise `cancel()` could never obtain it while the turn is running.
        let gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "a Jcode turn is already active on this connection".into(),
            ));
        }
        let generation = self.ensure_session_transport()?;
        let connection = self.connection.status()?;
        let identity = match &connection.state {
            JcodeConnectionState::Connected { identity } if connection.generation == generation => {
                identity
            }
            _ => {
                return Err(VibeCoderError::Agent(
                    "Jcode transport changed while preparing the turn".into(),
                ));
            }
        };
        if !identity
            .capabilities
            .iter()
            .any(|value| value == "streaming")
        {
            return Err(VibeCoderError::MissingCapability {
                component: "Jcode harness",
                capability: "streaming",
            });
        }
        let permissions_supported = identity
            .capabilities
            .iter()
            .any(|value| value == "permissions");
        let binding = self.sessions.binding(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "session must be created or resumed before running a turn".into(),
            )
        })?;
        let model_catalog = if options.model.is_some() {
            let model_client = self.open_fresh_model_client(session_id, &binding)?;
            let catalog = discover_models(&model_client, session_id)?;
            self.verify_transport_generation(generation)?;
            self.models.mark_verified(generation)?;
            drop(model_client);
            Some(catalog)
        } else {
            None
        };
        self.ensure_bound_session_attached(session_id, &binding, generation)?;
        if let (Some(model), Some(catalog)) = (options.model.as_ref(), model_catalog.as_ref()) {
            self.connection.with_client(|client| {
                select_model_from_catalog(client, session_id, model, catalog)
            })?;
            let verification_client = self.open_fresh_model_client(session_id, &binding)?;
            let _active_model = verify_active_model(&verification_client, session_id, model)?;
            self.verify_transport_generation(generation)?;
        }

        let run_client = self.connection.clone_client_for_inflight()?;
        let cancel_client = run_client.clone();
        let safety_client = run_client.clone();
        self.turns.begin(session_id, generation)?;
        let lease = ActiveTurnLease::new(
            Arc::clone(&self.turns),
            Arc::clone(&self.permissions),
            session_id.clone(),
            generation,
            cancel_client,
        );
        drop(gate);

        let (sender, receiver) = oneshot::channel();
        let worker_session = session_id.clone();
        let worker_prompt = prompt.to_owned();
        let (worker_options, safety_state) = run_options(
            worker_session.clone(),
            on_event,
            safety_client,
            permissions_supported,
            generation,
            Arc::clone(&self.permissions),
        );
        let worker_registry = Arc::clone(&self.turns);
        let worker_generation = generation;
        let spawn = std::thread::Builder::new()
            .name("vibecoder-jcode-turn".into())
            .spawn(move || {
                let result = run_client.run(&worker_session.0, &worker_prompt, worker_options);
                let completion =
                    worker_registry.mark_worker_finished(&worker_session, worker_generation);
                let _ = sender.send((result, completion));
            });
        if spawn.is_err() {
            return Err(VibeCoderError::Agent(
                "could not start the isolated Jcode turn worker".into(),
            ));
        }

        let (sdk_result, worker_completion) = receiver.await.map_err(|_| {
            VibeCoderError::Agent("Jcode turn worker exited without returning a result".into())
        })?;
        worker_completion?;
        let cancelled = lease.complete()?;
        if safety_state.permission_protocol_failure() {
            return Err(VibeCoderError::Agent(
                "Jcode permission protocol failed closed for this turn".into(),
            ));
        }
        match sdk_result {
            Ok(turn) => Ok(map_turn_result(turn, cancelled)),
            Err(_error) if cancelled => Err(VibeCoderError::Cancelled),
            Err(error) => Err(map_operation_error("run_turn", error)),
        }
    }

    async fn cancel(&self, session_id: &SessionId) -> Result<()> {
        validate_jcode_session_id(session_id)?;
        let _gate = self.sessions.lock_gate()?;
        let _turn_control = self.turns.lock_control()?;
        let generation = self.turns.active_generation(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "there is no active turn for this session to cancel".into(),
            )
        })?;
        let connection = self.connection.status()?;
        if !matches!(&connection.state, JcodeConnectionState::Connected { .. })
            || connection.generation != generation
        {
            return Err(VibeCoderError::Agent(
                "active Jcode turn transport is no longer the original connection".into(),
            ));
        }
        if self.sessions.binding(session_id)?.is_none() {
            return Err(VibeCoderError::InvalidRequest(
                "active turn lost its session/project binding".into(),
            ));
        }
        if !self
            .sessions
            .is_attached_on_generation(session_id, generation)?
        {
            return Err(VibeCoderError::Agent(
                "active Jcode turn lost its verified session attachment".into(),
            ));
        }

        self.connection.with_client(|client| {
            client
                .cancel(&session_id.0)
                .map_err(|error| map_operation_error("cancel", error))
        })?;
        // Mark only after the upstream cancel request is acknowledged. The turn-control gate
        // prevents normal completion cleanup from racing this decision.
        self.turns.mark_cancel_acknowledged(session_id).map(drop)
    }

    async fn respond_to_permission(
        &self,
        session_id: &SessionId,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<()> {
        validate_jcode_session_id(session_id)?;

        // Verify and reserve the response while attachment state is stable, but do not hold the
        // session gate or turn-control gate across the network request. Explicit cancel must remain
        // independently deliverable if a permission response stalls or races cancellation.
        let gate = self.sessions.lock_gate()?;
        let generation = self.turns.active_generation(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "permission response requires an active turn for this session".into(),
            )
        })?;
        let connection = self.connection.status()?;
        let identity = match &connection.state {
            JcodeConnectionState::Connected { identity } if connection.generation == generation => {
                identity
            }
            _ => {
                return Err(VibeCoderError::Agent(
                    "permission request transport is no longer the connection that emitted it"
                        .into(),
                ));
            }
        };
        if !identity
            .capabilities
            .iter()
            .any(|value| value == "permissions")
        {
            return Err(VibeCoderError::MissingCapability {
                component: "Jcode harness",
                capability: "permissions",
            });
        }
        if self.sessions.binding(session_id)?.is_none()
            || !self
                .sessions
                .is_attached_on_generation(session_id, generation)?
        {
            return Err(VibeCoderError::InvalidRequest(
                "permission request lost its verified session/project binding".into(),
            ));
        }
        let response_client = self.connection.clone_client_for_inflight()?;
        let pending = self
            .permissions
            .begin_response(session_id, request_id, generation)?;
        drop(gate);

        // VibeCoder's AllowSession scope is deliberately local and exact-match. Do not map it to
        // Jcode's `AllowAlways`: the reviewed API does not specify whether that persists beyond one
        // agent session. Every approved request therefore sends only the upstream single-use Allow.
        let upstream = match &decision {
            PermissionDecision::AllowOnce | PermissionDecision::AllowSession => {
                jcode_sdk::PermissionDecision::Allow
            }
            PermissionDecision::Deny => jcode_sdk::PermissionDecision::Deny,
        };
        let result = response_client
            .respond_to_permission(&session_id.0, request_id, upstream)
            .map_err(|error| map_operation_error("respond_to_permission", error));
        match result {
            Ok(()) => self.permissions.complete_response(&pending, decision),
            Err(error) => {
                self.permissions.abort_response(&pending);
                Err(error)
            }
        }
    }

    async fn list_models(&self, session_id: &SessionId) -> Result<Vec<ModelRef>> {
        validate_jcode_session_id(session_id)?;
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot inspect the Jcode model catalog while a turn is active".into(),
            ));
        }
        let generation = self.ensure_session_transport()?;
        let binding = self.sessions.binding(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "session must be created or resumed before discovering its models".into(),
            )
        })?;
        let model_client = self.open_fresh_model_client(session_id, &binding)?;
        let catalog = discover_models(&model_client, session_id)?;
        self.verify_transport_generation(generation)?;
        self.models.mark_verified(generation)?;
        Ok(catalog)
    }

    async fn corroborate_model_identity(
        &self,
        session_id: &SessionId,
        model: &ModelRef,
    ) -> Result<ModelRef> {
        validate_jcode_session_id(session_id)?;
        validate_model_ref(model)?;
        let _gate = self.sessions.lock_gate()?;
        if self.turns.has_active()? {
            return Err(VibeCoderError::InvalidRequest(
                "cannot change the Jcode model while a turn is active".into(),
            ));
        }
        let generation = self.ensure_session_transport()?;
        let binding = self.sessions.binding(session_id)?.ok_or_else(|| {
            VibeCoderError::InvalidRequest(
                "session must be created or resumed before selecting its model".into(),
            )
        })?;
        let model_client = self.open_fresh_model_client(session_id, &binding)?;
        let catalog = discover_models(&model_client, session_id)?;
        self.verify_transport_generation(generation)?;
        self.models.mark_verified(generation)?;
        drop(model_client);
        self.ensure_bound_session_attached(session_id, &binding, generation)?;
        self.connection
            .with_client(|client| select_model_from_catalog(client, session_id, model, &catalog))?;
        let verification_client = self.open_fresh_model_client(session_id, &binding)?;
        let active = verify_active_model(&verification_client, session_id, model)?;
        self.verify_transport_generation(generation)?;
        Ok(active)
    }

    async fn set_model(&self, session_id: &SessionId, model: &ModelRef) -> Result<()> {
        self.corroborate_model_identity(session_id, model)
            .await
            .map(|_| ())
    }
}
