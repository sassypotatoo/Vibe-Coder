//! Provider-neutral command request and authorization policy.
//!
//! Part 14 deliberately does not spawn processes. It converts a bounded, structured command
//! request into an unforgeable, single-use-by-move execution envelope only after an explicit
//! allow-once decision. Part 15 consumes the envelope to implement actual process lifecycle, timeout,
//! cancellation, bounded output capture, and operation-time executable resolution.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;
use vibecoder_domain::{ProjectId, Result, SessionId, VibeCoderError};

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_TOTAL_ARGUMENT_BYTES: usize = 32 * 1024;
const MAX_RELATIVE_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENT_BYTES: usize = 255;
const MAX_RUNTIME_TOOL_ID_BYTES: usize = 64;
const MAX_ALLOWED_RUNTIME_TOOLS: usize = 64;
const MAX_SESSION_ID_BYTES: usize = 256;
const MAX_PENDING_COMMANDS: usize = 64;
const MAX_PENDING_PER_SESSION: usize = 8;
const INTERNAL_TEMP_PREFIX: &str = ".vibecoder-tmp-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandProgram {
    /// A runtime-owned tool id. Part 15 must resolve this id through a trusted runtime registry;
    /// PATH lookup is not authority.
    RuntimeTool { tool_id: String },
    /// A project-relative executable such as `gradlew`. Part 15 must open/verify it beneath the
    /// project root at execution time rather than trusting a previously resolved absolute path.
    WorkspaceExecutable { relative_path: PathBuf },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub program: CommandProgram,
    pub args: Vec<String>,
    /// Empty or `.` means the project root. Absolute/parent-traversal paths are rejected.
    pub working_dir: PathBuf,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandSpec")
            .field("program", &self.program)
            .field(
                "args",
                &format_args!("[REDACTED; {} argument(s)]", self.args.len()),
            )
            .field("working_dir", &self.working_dir)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandApprovalDecision {
    AllowOnce,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandApprovalRequest {
    pub request_id: String,
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub command: CommandSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDenied {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRequestOutcome {
    ApprovalRequired(CommandApprovalRequest),
    Denied(CommandDenied),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandDecisionOutcome {
    Authorized(CommandExecutionEnvelope),
    Denied(CommandDenied),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEnvironmentPolicy {
    /// Part 15 must construct an explicit runtime-managed environment. Ambient app/process
    /// environment is not inherited merely because an agent requested a command.
    RuntimeManagedClean,
}

/// Authorization object handed to the Part 15 executor.
///
/// It intentionally does not implement `Clone`, `Serialize`, or `Deserialize`, and all fields are
/// private. Safe Rust therefore cannot fabricate one outside this crate or duplicate it for replay.
/// The Part 15 executor must consume this value by ownership and must still re-resolve filesystem
/// and runtime-tool authority at operation time.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandExecutionEnvelope {
    request_id: String,
    session_id: SessionId,
    project_id: ProjectId,
    command: CommandSpec,
    environment_policy: CommandEnvironmentPolicy,
    project_epoch: u64,
}

/// Move-only authorized command material extracted from one allow-once envelope.
///
/// This type also keeps private fields and is only constructible by this crate. The executor can
/// inspect it through getters after consuming the envelope, but cannot fabricate a new grant.
#[derive(Debug, PartialEq, Eq)]
pub struct AuthorizedCommand {
    request_id: String,
    session_id: SessionId,
    project_id: ProjectId,
    command: CommandSpec,
    environment_policy: CommandEnvironmentPolicy,
    project_epoch: u64,
}

impl AuthorizedCommand {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn project_epoch(&self) -> u64 {
        self.project_epoch
    }

    pub fn command(&self) -> &CommandSpec {
        &self.command
    }

    pub const fn environment_policy(&self) -> CommandEnvironmentPolicy {
        self.environment_policy
    }
}

impl CommandExecutionEnvelope {
    /// Consume the single-use authorization. There is intentionally no inverse constructor.
    pub fn into_authorized_command(self) -> AuthorizedCommand {
        AuthorizedCommand {
            request_id: self.request_id,
            session_id: self.session_id,
            project_id: self.project_id,
            command: self.command,
            environment_policy: self.environment_policy,
            project_epoch: self.project_epoch,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn project_epoch(&self) -> u64 {
        self.project_epoch
    }

    pub fn command(&self) -> &CommandSpec {
        &self.command
    }

    pub const fn environment_policy(&self) -> CommandEnvironmentPolicy {
        self.environment_policy
    }

    /// Part 14 never authorizes shell-string interpretation.
    pub const fn uses_shell(&self) -> bool {
        false
    }

    /// Caller/model supplied stdin is deliberately not part of the Part 14 command contract.
    pub const fn caller_stdin_enabled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPolicyConfig {
    allowed_runtime_tools: HashSet<String>,
    allow_workspace_executables: bool,
}

impl CommandPolicyConfig {
    /// Fail-closed default. Part 15/runtime provisioning must explicitly register trusted tools.
    pub fn deny_all() -> Self {
        Self {
            allowed_runtime_tools: HashSet::new(),
            allow_workspace_executables: false,
        }
    }

    pub fn new<I, S>(allowed_runtime_tools: I, allow_workspace_executables: bool) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut tools = HashSet::new();
        for tool in allowed_runtime_tools {
            if tools.len() >= MAX_ALLOWED_RUNTIME_TOOLS {
                return Err(command_error("command_policy_runtime_tool_limit"));
            }
            let tool = validate_runtime_tool_id(tool.into())?;
            if !tools.insert(tool) {
                return Err(command_error("command_policy_duplicate_runtime_tool"));
            }
        }
        Ok(Self {
            allowed_runtime_tools: tools,
            allow_workspace_executables,
        })
    }

    pub fn allows_runtime_tool(&self, tool_id: &str) -> bool {
        self.allowed_runtime_tools.contains(tool_id)
    }

    pub const fn allows_workspace_executables(&self) -> bool {
        self.allow_workspace_executables
    }
}

impl Default for CommandPolicyConfig {
    fn default() -> Self {
        Self::deny_all()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCommand {
    session_id: SessionId,
    project_id: ProjectId,
    command: CommandSpec,
}

#[derive(Debug, Default)]
struct CommandPolicyState {
    pending: HashMap<String, PendingCommand>,
    project_epochs: HashMap<ProjectId, u64>,
}

#[derive(Debug)]
pub struct CommandPolicyEngine {
    config: CommandPolicyConfig,
    state: Mutex<CommandPolicyState>,
}

impl CommandPolicyEngine {
    pub fn new(config: CommandPolicyConfig) -> Self {
        Self {
            config,
            state: Mutex::new(CommandPolicyState::default()),
        }
    }

    pub fn config(&self) -> &CommandPolicyConfig {
        &self.config
    }

    /// Validate and enqueue one command for explicit approval. No eligible command is auto-run.
    pub fn request_command(
        &self,
        session_id: &SessionId,
        project_id: ProjectId,
        command: CommandSpec,
    ) -> Result<CommandRequestOutcome> {
        validate_session_scope(session_id)?;
        let command = validate_command_spec(command)?;
        if let Some(code) = self.policy_denial(&command) {
            return Ok(CommandRequestOutcome::Denied(CommandDenied {
                code: code.into(),
            }));
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;

        if state.pending.len() >= MAX_PENDING_COMMANDS {
            return Err(command_error("command_pending_global_limit"));
        }
        let per_session = state
            .pending
            .values()
            .filter(|pending| pending.session_id == *session_id)
            .count();
        if per_session >= MAX_PENDING_PER_SESSION {
            return Err(command_error("command_pending_session_limit"));
        }
        if state.pending.values().any(|pending| {
            pending.session_id == *session_id
                && pending.project_id == project_id
                && pending.command == command
        }) {
            return Err(command_error("command_request_duplicate_pending"));
        }

        let request_id = Uuid::new_v4().hyphenated().to_string();
        let pending = PendingCommand {
            session_id: session_id.clone(),
            project_id,
            command: command.clone(),
        };
        match state.pending.entry(request_id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(pending);
            }
            Entry::Occupied(_) => return Err(command_error("command_request_id_collision")),
        }

        Ok(CommandRequestOutcome::ApprovalRequired(
            CommandApprovalRequest {
                request_id,
                session_id: session_id.clone(),
                project_id,
                command,
            },
        ))
    }

    /// Resolve exactly one pending request. Session/project mismatches fail closed and do not
    /// consume the rightful pending request.
    pub fn decide(
        &self,
        session_id: &SessionId,
        project_id: ProjectId,
        approval: &CommandApprovalRequest,
        decision: CommandApprovalDecision,
    ) -> Result<CommandDecisionOutcome> {
        validate_session_scope(session_id)?;
        validate_session_scope(&approval.session_id)?;
        validate_request_id(&approval.request_id)?;
        if approval.session_id != *session_id || approval.project_id != project_id {
            return Err(command_error("command_approval_context_mismatch"));
        }
        let request_id = approval.request_id.as_str();
        let mut state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;
        let pending = state
            .pending
            .get(request_id)
            .ok_or_else(|| command_error("command_request_not_pending"))?;
        if pending.session_id != *session_id || pending.project_id != project_id {
            return Err(command_error("command_request_scope_mismatch"));
        }
        if decision == CommandApprovalDecision::AllowOnce && pending.command != approval.command {
            return Err(command_error("command_approval_payload_mismatch"));
        }

        let pending = state
            .pending
            .remove(request_id)
            .ok_or_else(|| command_error("command_request_not_pending"))?;
        match decision {
            CommandApprovalDecision::Deny => Ok(CommandDecisionOutcome::Denied(CommandDenied {
                code: "command_denied_by_user".into(),
            })),
            CommandApprovalDecision::AllowOnce => {
                let project_epoch = state
                    .project_epochs
                    .get(&pending.project_id)
                    .copied()
                    .unwrap_or(0);
                Ok(CommandDecisionOutcome::Authorized(
                    CommandExecutionEnvelope {
                        request_id: request_id.to_owned(),
                        session_id: pending.session_id,
                        project_id: pending.project_id,
                        command: pending.command,
                        environment_policy: CommandEnvironmentPolicy::RuntimeManagedClean,
                        project_epoch,
                    },
                ))
            }
        }
    }

    /// Drop all still-pending command requests for one session. Issued envelopes are intentionally
    /// not tracked here; Part 15 process lifecycle owns cancellation after execution begins.
    pub fn revoke_pending_for_session(&self, session_id: &SessionId) -> Result<usize> {
        validate_session_scope(session_id)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;
        let before = state.pending.len();
        state
            .pending
            .retain(|_, pending| pending.session_id != *session_id);
        Ok(before.saturating_sub(state.pending.len()))
    }

    /// Revoke pending requests and invalidate every already-issued-but-not-started envelope for
    /// this project by advancing a monotonic in-memory workspace authorization epoch.
    pub fn invalidate_project_authorizations(&self, project_id: ProjectId) -> Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;
        let before = state.pending.len();
        state
            .pending
            .retain(|_, pending| pending.project_id != project_id);
        let current = state.project_epochs.get(&project_id).copied().unwrap_or(0);
        let next = current
            .checked_add(1)
            .ok_or_else(|| command_error("command_project_epoch_overflow"))?;
        state.project_epochs.insert(project_id, next);
        Ok(before.saturating_sub(state.pending.len()))
    }

    /// Revalidate a move-only envelope immediately before local process startup. Rollback advances
    /// the project epoch, making pre-rollback approvals stale even when the caller still owns one.
    pub fn validate_execution_envelope(&self, envelope: &CommandExecutionEnvelope) -> Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;
        let current = state
            .project_epochs
            .get(&envelope.project_id)
            .copied()
            .unwrap_or(0);
        if envelope.project_epoch != current {
            return Err(command_error(
                "command_execution_envelope_stale_project_epoch",
            ));
        }
        Ok(())
    }

    pub fn pending_count(&self) -> Result<usize> {
        let state = self
            .state
            .lock()
            .map_err(|_| command_error("command_policy_state_poisoned"))?;
        Ok(state.pending.len())
    }

    fn policy_denial(&self, command: &CommandSpec) -> Option<&'static str> {
        match &command.program {
            CommandProgram::RuntimeTool { tool_id } => {
                if self.config.allows_runtime_tool(tool_id) {
                    None
                } else {
                    Some("command_runtime_tool_not_allowed")
                }
            }
            CommandProgram::WorkspaceExecutable { .. } => {
                if self.config.allows_workspace_executables() {
                    None
                } else {
                    Some("command_workspace_executable_not_allowed")
                }
            }
        }
    }
}

impl Default for CommandPolicyEngine {
    fn default() -> Self {
        Self::new(CommandPolicyConfig::deny_all())
    }
}

fn validate_command_spec(command: CommandSpec) -> Result<CommandSpec> {
    if command.args.len() > MAX_ARGUMENTS {
        return Err(command_error("command_argument_count_exceeded"));
    }
    let mut total = 0usize;
    let mut args = Vec::with_capacity(command.args.len());
    for arg in command.args {
        if arg.len() > MAX_ARGUMENT_BYTES || has_forbidden_display_char(&arg) {
            return Err(command_error("command_argument_invalid"));
        }
        total = total
            .checked_add(arg.len())
            .ok_or_else(|| command_error("command_argument_bytes_exceeded"))?;
        if total > MAX_TOTAL_ARGUMENT_BYTES {
            return Err(command_error("command_argument_bytes_exceeded"));
        }
        args.push(arg);
    }

    let working_dir = normalize_project_relative(&command.working_dir, true)?;
    let program = match command.program {
        CommandProgram::RuntimeTool { tool_id } => CommandProgram::RuntimeTool {
            tool_id: validate_runtime_tool_id(tool_id)?,
        },
        CommandProgram::WorkspaceExecutable { relative_path } => {
            CommandProgram::WorkspaceExecutable {
                relative_path: normalize_project_relative(&relative_path, false)?,
            }
        }
    };

    Ok(CommandSpec {
        program,
        args,
        working_dir,
    })
}

fn validate_runtime_tool_id(tool_id: String) -> Result<String> {
    if tool_id.is_empty()
        || tool_id.len() > MAX_RUNTIME_TOOL_ID_BYTES
        || !tool_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(command_error("command_runtime_tool_id_invalid"));
    }
    let folded = tool_id.to_ascii_lowercase();
    if matches!(
        folded.as_str(),
        "sh" | "bash"
            | "dash"
            | "zsh"
            | "fish"
            | "cmd"
            | "cmd.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
    ) {
        return Err(command_error("command_runtime_shell_tool_forbidden"));
    }
    Ok(tool_id)
}

fn normalize_project_relative(path: &Path, allow_root: bool) -> Result<PathBuf> {
    if path.is_absolute() {
        return Err(command_error("command_path_must_be_project_relative"));
    }
    let text = path
        .to_str()
        .ok_or_else(|| command_error("command_path_must_be_utf8"))?;
    if text.len() > MAX_RELATIVE_PATH_BYTES
        || text.contains('\\')
        || has_forbidden_display_char(text)
    {
        return Err(command_error("command_path_invalid"));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| command_error("command_path_must_be_utf8"))?;
                if value.is_empty()
                    || value.len() > MAX_PATH_COMPONENT_BYTES
                    || value.starts_with(INTERNAL_TEMP_PREFIX)
                {
                    return Err(command_error("command_path_component_invalid"));
                }
                normalized.push(value);
            }
            Component::ParentDir => return Err(command_error("command_path_parent_forbidden")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(command_error("command_path_must_be_project_relative"));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        if !allow_root {
            return Err(command_error("command_executable_path_empty"));
        }
        return Ok(PathBuf::from("."));
    }
    Ok(normalized)
}

fn has_forbidden_display_char(value: &str) -> bool {
    value.chars().any(|ch| {
        ch.is_control()
            || matches!(
                ch,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
    })
}

fn validate_session_scope(session_id: &SessionId) -> Result<()> {
    let value = session_id.0.as_str();
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > MAX_SESSION_ID_BYTES
        || has_forbidden_display_char(value)
    {
        return Err(command_error("command_session_id_invalid"));
    }
    Ok(())
}

fn validate_request_id(request_id: &str) -> Result<()> {
    let parsed =
        Uuid::parse_str(request_id).map_err(|_| command_error("command_request_id_invalid"))?;
    if parsed.hyphenated().to_string() != request_id {
        return Err(command_error("command_request_id_invalid"));
    }
    Ok(())
}

fn command_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Command(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionId {
        SessionId::parse("session-1").expect("session")
    }

    fn runtime_policy() -> CommandPolicyEngine {
        CommandPolicyEngine::new(
            CommandPolicyConfig::new(["gradle", "node"], true).expect("policy"),
        )
    }

    fn runtime_command() -> CommandSpec {
        CommandSpec {
            program: CommandProgram::RuntimeTool {
                tool_id: "gradle".into(),
            },
            args: vec![":app:assembleDebug".into()],
            working_dir: PathBuf::from("."),
        }
    }

    #[test]
    fn malformed_deserialized_style_session_scope_is_rejected() {
        let policy = runtime_policy();
        for invalid in [SessionId(String::new()), SessionId(" session-1 ".into())] {
            assert!(
                policy
                    .request_command(&invalid, ProjectId::new(), runtime_command())
                    .is_err()
            );
        }
    }

    #[test]
    fn runtime_shell_interpreters_cannot_be_registered() {
        for shell in [
            "sh",
            "bash",
            "dash",
            "zsh",
            "fish",
            "cmd.exe",
            "powershell",
            "pwsh",
        ] {
            assert!(CommandPolicyConfig::new([shell], false).is_err(), "{shell}");
        }
    }

    #[test]
    fn deny_all_is_fail_closed() {
        let policy = CommandPolicyEngine::default();
        let outcome = policy
            .request_command(&session(), ProjectId::new(), runtime_command())
            .expect("request");
        assert!(matches!(
            outcome,
            CommandRequestOutcome::Denied(CommandDenied { ref code })
                if code == "command_runtime_tool_not_allowed"
        ));
    }

    #[test]
    fn eligible_command_requires_explicit_allow_once() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy
            .request_command(&session(), project, runtime_command())
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(request) = request else {
            panic!("expected approval");
        };
        let result = policy
            .decide(
                &session(),
                project,
                &request,
                CommandApprovalDecision::AllowOnce,
            )
            .expect("decision");
        let CommandDecisionOutcome::Authorized(envelope) = result else {
            panic!("expected envelope");
        };
        assert_eq!(envelope.command(), &runtime_command());
        assert_eq!(
            envelope.environment_policy(),
            CommandEnvironmentPolicy::RuntimeManagedClean
        );
        assert!(!envelope.uses_shell());
        assert!(!envelope.caller_stdin_enabled());
        assert_eq!(policy.pending_count().expect("pending"), 0);
    }

    #[test]
    fn tampered_approval_payload_cannot_change_or_mask_the_command() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy
            .request_command(&session(), project, runtime_command())
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(mut request) = request else {
            panic!("expected approval");
        };
        request.command.args = vec!["safe-looking-task".into()];
        assert!(
            policy
                .decide(
                    &session(),
                    project,
                    &request,
                    CommandApprovalDecision::AllowOnce,
                )
                .is_err()
        );
        assert_eq!(policy.pending_count().expect("pending"), 1);
    }

    #[test]
    fn tampered_payload_can_still_be_denied_without_granting_authority() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy
            .request_command(&session(), project, runtime_command())
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(mut request) = request else {
            panic!("expected approval");
        };
        request.command.args = vec!["different-display".into()];
        let outcome = policy
            .decide(&session(), project, &request, CommandApprovalDecision::Deny)
            .expect("deny");
        assert!(matches!(outcome, CommandDecisionOutcome::Denied(_)));
        assert_eq!(policy.pending_count().expect("pending"), 0);
    }

    #[test]
    fn wrong_scope_cannot_consume_pending_request() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy
            .request_command(&session(), project, runtime_command())
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(request) = request else {
            panic!("expected approval");
        };
        let other = SessionId::parse("other").expect("session");
        assert!(
            policy
                .decide(
                    &other,
                    project,
                    &request,
                    CommandApprovalDecision::AllowOnce,
                )
                .is_err()
        );
        assert_eq!(policy.pending_count().expect("pending"), 1);
        let denied = policy
            .decide(&session(), project, &request, CommandApprovalDecision::Deny)
            .expect("rightful deny");
        assert!(matches!(denied, CommandDecisionOutcome::Denied(_)));
    }

    #[test]
    fn raw_shell_shape_and_escape_paths_are_unrepresentable_or_rejected() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let traversal = CommandSpec {
            program: CommandProgram::WorkspaceExecutable {
                relative_path: PathBuf::from("../gradlew"),
            },
            args: Vec::new(),
            working_dir: PathBuf::new(),
        };
        assert!(
            policy
                .request_command(&session(), project, traversal)
                .is_err()
        );

        let newline_arg = CommandSpec {
            program: CommandProgram::RuntimeTool {
                tool_id: "node".into(),
            },
            args: vec!["safe\nspoof".into()],
            working_dir: PathBuf::new(),
        };
        assert!(
            policy
                .request_command(&session(), project, newline_arg)
                .is_err()
        );
    }

    #[test]
    fn bidi_override_cannot_spoof_approval_arguments() {
        let policy = runtime_policy();
        let command = CommandSpec {
            program: CommandProgram::RuntimeTool {
                tool_id: "node".into(),
            },
            args: vec!["safe\u{202e}txt".into()],
            working_dir: PathBuf::new(),
        };
        assert!(
            policy
                .request_command(&session(), ProjectId::new(), command)
                .is_err()
        );
    }

    #[test]
    fn duplicate_pending_request_is_rejected_and_session_revoke_clears_it() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        policy
            .request_command(&session(), project, runtime_command())
            .expect("first");
        assert!(
            policy
                .request_command(&session(), project, runtime_command())
                .is_err()
        );
        assert_eq!(
            policy
                .revoke_pending_for_session(&session())
                .expect("revoke"),
            1
        );
        assert_eq!(policy.pending_count().expect("pending"), 0);
    }

    #[test]
    fn rollback_epoch_invalidates_already_issued_envelope() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy
            .request_command(&session(), project, runtime_command())
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(request) = request else {
            panic!("expected approval");
        };
        let outcome = policy
            .decide(
                &session(),
                project,
                &request,
                CommandApprovalDecision::AllowOnce,
            )
            .expect("allow");
        let CommandDecisionOutcome::Authorized(envelope) = outcome else {
            panic!("expected envelope");
        };
        policy
            .validate_execution_envelope(&envelope)
            .expect("fresh envelope");
        policy
            .invalidate_project_authorizations(project)
            .expect("invalidate");
        assert!(policy.validate_execution_envelope(&envelope).is_err());
    }

    #[test]
    fn workspace_executable_is_relative_and_normalized() {
        let policy = runtime_policy();
        let project = ProjectId::new();
        let request = policy.request_command(
            &session(),
            project,
            CommandSpec {
                program: CommandProgram::WorkspaceExecutable {
                    relative_path: PathBuf::from("./gradlew"),
                },
                args: vec!["assembleDebug".into()],
                working_dir: PathBuf::from("./app/.."),
            },
        );
        assert!(request.is_err(), "parent traversal must remain forbidden");

        let request = policy
            .request_command(
                &session(),
                project,
                CommandSpec {
                    program: CommandProgram::WorkspaceExecutable {
                        relative_path: PathBuf::from("./gradlew"),
                    },
                    args: vec!["assembleDebug".into()],
                    working_dir: PathBuf::from("./app"),
                },
            )
            .expect("request");
        let CommandRequestOutcome::ApprovalRequired(request) = request else {
            panic!("expected approval");
        };
        assert_eq!(
            request.command.program,
            CommandProgram::WorkspaceExecutable {
                relative_path: PathBuf::from("gradlew")
            }
        );
        assert_eq!(request.command.working_dir, PathBuf::from("app"));
    }
}
