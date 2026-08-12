//! Authority-free repair/rebuild loop guards for VibeCoder Part 22.
//!
//! This crate owns retry accounting, repeated-failure detection, and cooperative cancellation
//! state. It never edits files, runs an agent turn, approves a command, or starts a process.

use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
use vibecoder_build_contract::{BuildResult, BuildState, BuildTargetKind};
use vibecoder_build_repair::{BuildFailureEvidence, BuildRepairPlan};
use vibecoder_domain::{ProjectId, Result, TurnResult, VibeCoderError};

pub const DEFAULT_MAX_REPAIR_ATTEMPTS: u8 = 3;
pub const MAX_REPAIR_ATTEMPTS: u8 = 8;
pub const DEFAULT_MAX_SAME_FAILURE_OCCURRENCES: u8 = 2;
pub const MAX_SAME_FAILURE_OCCURRENCES: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildRepairLoopPolicy {
    pub max_repair_attempts: u8,
    pub max_same_failure_occurrences: u8,
}

impl Default for BuildRepairLoopPolicy {
    fn default() -> Self {
        Self {
            max_repair_attempts: DEFAULT_MAX_REPAIR_ATTEMPTS,
            max_same_failure_occurrences: DEFAULT_MAX_SAME_FAILURE_OCCURRENCES,
        }
    }
}

impl BuildRepairLoopPolicy {
    pub fn validate(self) -> Result<Self> {
        if self.max_repair_attempts == 0 || self.max_repair_attempts > MAX_REPAIR_ATTEMPTS {
            return Err(loop_error("build_loop_retry_budget_invalid"));
        }
        if self.max_same_failure_occurrences < 2
            || self.max_same_failure_occurrences > MAX_SAME_FAILURE_OCCURRENCES
        {
            return Err(loop_error("build_loop_same_failure_limit_invalid"));
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct BuildRepairLoopCancellation {
    requested: Arc<AtomicBool>,
}

impl fmt::Debug for BuildRepairLoopCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildRepairLoopCancellation")
            .field("requested", &self.is_requested())
            .finish()
    }
}

impl BuildRepairLoopCancellation {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRepairLoopStopReason {
    Succeeded,
    Cancelled,
    TimedOut,
    RetryBudgetExhausted,
    RepeatedFailure,
    RepairTurnCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopPhase {
    AwaitingBuildResult,
    RepairRunning { attempt: u8 },
    AwaitingRebuild { attempt: u8 },
    Terminal(BuildRepairLoopStopReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BuildRepairLoopId(Uuid);

impl BuildRepairLoopId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

pub struct RepairAttemptPermit {
    loop_id: BuildRepairLoopId,
    project_id: ProjectId,
    attempt: u8,
    fingerprint_sha256: String,
    evidence: BuildFailureEvidence,
}

impl fmt::Debug for RepairAttemptPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepairAttemptPermit")
            .field("project_id", &self.project_id)
            .field("attempt", &self.attempt)
            .field("fingerprint_sha256", &self.fingerprint_sha256)
            .field("evidence", &self.evidence)
            .finish()
    }
}

impl RepairAttemptPermit {
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn attempt(&self) -> u8 {
        self.attempt
    }
    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }
    pub fn evidence(&self) -> &BuildFailureEvidence {
        &self.evidence
    }
}

pub struct RebuildAttemptPermit {
    loop_id: BuildRepairLoopId,
    project_id: ProjectId,
    target: BuildTargetKind,
    attempt: u8,
}

impl fmt::Debug for RebuildAttemptPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RebuildAttemptPermit")
            .field("project_id", &self.project_id)
            .field("target", &self.target)
            .field("attempt", &self.attempt)
            .finish()
    }
}

impl RebuildAttemptPermit {
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn target(&self) -> BuildTargetKind {
        self.target
    }
    pub const fn attempt(&self) -> u8 {
        self.attempt
    }
}

pub enum RepairAuthorization {
    Repair(RepairAttemptPermit),
    Stop(BuildRepairLoopStopReason),
}

impl fmt::Debug for RepairAuthorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repair(permit) => formatter.debug_tuple("Repair").field(permit).finish(),
            Self::Stop(reason) => formatter.debug_tuple("Stop").field(reason).finish(),
        }
    }
}

pub struct BuildRepairLoopGuard {
    loop_id: BuildRepairLoopId,
    project_id: ProjectId,
    target: BuildTargetKind,
    policy: BuildRepairLoopPolicy,
    cancellation: BuildRepairLoopCancellation,
    phase: LoopPhase,
    repair_attempts_started: u8,
    last_failure_fingerprint: Option<String>,
    same_failure_occurrences: u8,
}

impl fmt::Debug for BuildRepairLoopGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildRepairLoopGuard")
            .field("project_id", &self.project_id)
            .field("target", &self.target)
            .field("policy", &self.policy)
            .field("phase", &self.phase)
            .field("repair_attempts_started", &self.repair_attempts_started)
            .field("same_failure_occurrences", &self.same_failure_occurrences)
            .field("cancel_requested", &self.cancellation.is_requested())
            .field(
                "last_failure_fingerprint",
                &self.last_failure_fingerprint.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl BuildRepairLoopGuard {
    pub fn new(
        project_id: ProjectId,
        target: BuildTargetKind,
        policy: BuildRepairLoopPolicy,
    ) -> Result<Self> {
        Ok(Self {
            loop_id: BuildRepairLoopId::new(),
            project_id,
            target,
            policy: policy.validate()?,
            cancellation: BuildRepairLoopCancellation::new(),
            phase: LoopPhase::AwaitingBuildResult,
            repair_attempts_started: 0,
            last_failure_fingerprint: None,
            same_failure_occurrences: 0,
        })
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn target(&self) -> BuildTargetKind {
        self.target
    }
    pub const fn policy(&self) -> BuildRepairLoopPolicy {
        self.policy
    }
    pub const fn repair_attempts_started(&self) -> u8 {
        self.repair_attempts_started
    }
    pub fn cancellation(&self) -> BuildRepairLoopCancellation {
        self.cancellation.clone()
    }
    pub fn stop_reason(&self) -> Option<BuildRepairLoopStopReason> {
        match self.phase {
            LoopPhase::Terminal(reason) => Some(reason),
            _ if self.cancellation.is_requested() => Some(BuildRepairLoopStopReason::Cancelled),
            _ => None,
        }
    }

    pub fn finish_nonfailed_build(
        &mut self,
        result: &BuildResult,
    ) -> Result<BuildRepairLoopStopReason> {
        if result.project_id() != self.project_id || result.target() != self.target {
            return Err(loop_error("build_loop_result_scope_mismatch"));
        }
        if !matches!(self.phase, LoopPhase::AwaitingBuildResult) {
            return Err(loop_error("build_loop_not_awaiting_build_result"));
        }
        let reason = match result.state() {
            BuildState::Succeeded => BuildRepairLoopStopReason::Succeeded,
            BuildState::Cancelled => BuildRepairLoopStopReason::Cancelled,
            BuildState::TimedOut => BuildRepairLoopStopReason::TimedOut,
            BuildState::Failed => {
                return Err(loop_error(
                    "build_loop_failed_result_requires_repair_authorization",
                ));
            }
            BuildState::Queued | BuildState::Running => {
                return Err(loop_error("build_loop_requires_terminal_build_result"));
            }
        };
        self.phase = LoopPhase::Terminal(reason);
        Ok(reason)
    }

    pub fn authorize_repair(&mut self, result: &BuildResult) -> Result<RepairAuthorization> {
        if result.project_id() != self.project_id || result.target() != self.target {
            return Err(loop_error("build_loop_result_scope_mismatch"));
        }
        if !matches!(self.phase, LoopPhase::AwaitingBuildResult) {
            return Err(loop_error("build_loop_not_awaiting_build_result"));
        }

        let terminal = match result.state() {
            BuildState::Succeeded => Some(BuildRepairLoopStopReason::Succeeded),
            BuildState::Cancelled => Some(BuildRepairLoopStopReason::Cancelled),
            BuildState::TimedOut => Some(BuildRepairLoopStopReason::TimedOut),
            BuildState::Queued | BuildState::Running => {
                return Err(loop_error("build_loop_requires_terminal_build_result"));
            }
            BuildState::Failed => None,
        };
        if let Some(reason) = terminal {
            self.phase = LoopPhase::Terminal(reason);
            return Ok(RepairAuthorization::Stop(reason));
        }
        if self.cancellation.is_requested() {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::Cancelled);
            return Ok(RepairAuthorization::Stop(
                BuildRepairLoopStopReason::Cancelled,
            ));
        }

        let plan = BuildRepairPlan::from_failed_build(result)?;
        let evidence = plan.into_evidence();
        let fingerprint = evidence.fingerprint_sha256().to_owned();
        if self.last_failure_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            self.same_failure_occurrences = self.same_failure_occurrences.saturating_add(1);
        } else {
            self.last_failure_fingerprint = Some(fingerprint.clone());
            self.same_failure_occurrences = 1;
        }

        if self.same_failure_occurrences >= self.policy.max_same_failure_occurrences {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::RepeatedFailure);
            return Ok(RepairAuthorization::Stop(
                BuildRepairLoopStopReason::RepeatedFailure,
            ));
        }
        if self.repair_attempts_started >= self.policy.max_repair_attempts {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::RetryBudgetExhausted);
            return Ok(RepairAuthorization::Stop(
                BuildRepairLoopStopReason::RetryBudgetExhausted,
            ));
        }

        self.repair_attempts_started = self.repair_attempts_started.saturating_add(1);
        let attempt = self.repair_attempts_started;
        self.phase = LoopPhase::RepairRunning { attempt };
        Ok(RepairAuthorization::Repair(RepairAttemptPermit {
            loop_id: self.loop_id,
            project_id: self.project_id,
            attempt,
            fingerprint_sha256: fingerprint,
            evidence,
        }))
    }

    pub fn finish_repair(
        &mut self,
        permit: RepairAttemptPermit,
        turn: &TurnResult,
        observed_fingerprint: &str,
    ) -> Result<()> {
        self.verify_repair_permit(&permit)?;
        if permit.fingerprint_sha256 != observed_fingerprint {
            return Err(loop_error("build_loop_repair_fingerprint_mismatch"));
        }
        if self.cancellation.is_requested() {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::Cancelled);
            return Ok(());
        }
        if turn.cancelled {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::RepairTurnCancelled);
            return Ok(());
        }
        self.phase = LoopPhase::AwaitingRebuild {
            attempt: permit.attempt,
        };
        Ok(())
    }

    pub fn rebuild_permit(&self) -> Result<RebuildAttemptPermit> {
        if self.cancellation.is_requested() {
            return Err(VibeCoderError::Cancelled);
        }
        let LoopPhase::AwaitingRebuild { attempt } = self.phase else {
            return Err(loop_error("build_loop_not_awaiting_rebuild"));
        };
        Ok(RebuildAttemptPermit {
            loop_id: self.loop_id,
            project_id: self.project_id,
            target: self.target,
            attempt,
        })
    }

    pub fn mark_rebuild_prepared(&mut self, permit: RebuildAttemptPermit) -> Result<()> {
        if permit.loop_id != self.loop_id
            || permit.project_id != self.project_id
            || permit.target != self.target
        {
            return Err(loop_error("build_loop_rebuild_permit_scope_mismatch"));
        }
        let LoopPhase::AwaitingRebuild { attempt } = self.phase else {
            return Err(loop_error("build_loop_not_awaiting_rebuild"));
        };
        if attempt != permit.attempt {
            return Err(loop_error("build_loop_rebuild_attempt_mismatch"));
        }
        if self.cancellation.is_requested() {
            self.phase = LoopPhase::Terminal(BuildRepairLoopStopReason::Cancelled);
            return Err(VibeCoderError::Cancelled);
        }
        self.phase = LoopPhase::AwaitingBuildResult;
        Ok(())
    }

    fn verify_repair_permit(&self, permit: &RepairAttemptPermit) -> Result<()> {
        if permit.loop_id != self.loop_id || permit.project_id != self.project_id {
            return Err(loop_error("build_loop_repair_permit_scope_mismatch"));
        }
        let LoopPhase::RepairRunning { attempt } = self.phase else {
            return Err(loop_error("build_loop_repair_not_running"));
        };
        if attempt != permit.attempt {
            return Err(loop_error("build_loop_repair_attempt_mismatch"));
        }
        Ok(())
    }
}

fn loop_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Build(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibecoder_build_contract::BuildJobDescriptor;
    use vibecoder_process_contract::{ProcessId, ProcessResult, ProcessTermination};

    fn clean_turn() -> TurnResult {
        TurnResult {
            text: String::new(),
            cancelled: false,
            tool_calls: Vec::new(),
            usage: None,
        }
    }

    fn failed(project: ProjectId, stderr: &[u8]) -> BuildResult {
        BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Website),
            ProcessResult {
                process_id: ProcessId::new(),
                termination: ProcessTermination::Exited,
                exit_code: Some(1),
                stdout: Vec::new(),
                stderr: stderr.to_vec(),
                stdout_truncated: false,
                stderr_truncated: false,
                event_queue_overflowed: false,
                duration_ms: 1,
            },
        )
    }

    #[test]
    fn repeated_identical_failure_stops_before_second_repair_by_default() {
        let project = ProjectId::new();
        let mut guard = BuildRepairLoopGuard::new(
            project,
            BuildTargetKind::Website,
            BuildRepairLoopPolicy::default(),
        )
        .unwrap();
        let first = failed(project, b"error: same failure");
        let permit = match guard.authorize_repair(&first).unwrap() {
            RepairAuthorization::Repair(permit) => permit,
            other => panic!("unexpected {other:?}"),
        };
        let fingerprint = permit.fingerprint_sha256().to_owned();
        guard
            .finish_repair(permit, &clean_turn(), &fingerprint)
            .unwrap();
        let rebuild = guard.rebuild_permit().unwrap();
        guard.mark_rebuild_prepared(rebuild).unwrap();
        let second = failed(project, b"error: same failure");
        assert!(matches!(
            guard.authorize_repair(&second).unwrap(),
            RepairAuthorization::Stop(BuildRepairLoopStopReason::RepeatedFailure)
        ));
    }

    #[test]
    fn retry_budget_stops_distinct_failures() {
        let project = ProjectId::new();
        let mut guard = BuildRepairLoopGuard::new(
            project,
            BuildTargetKind::Website,
            BuildRepairLoopPolicy {
                max_repair_attempts: 1,
                max_same_failure_occurrences: 2,
            },
        )
        .unwrap();
        let first = failed(project, b"error: first");
        let permit = match guard.authorize_repair(&first).unwrap() {
            RepairAuthorization::Repair(permit) => permit,
            other => panic!("unexpected {other:?}"),
        };
        let fingerprint = permit.fingerprint_sha256().to_owned();
        guard
            .finish_repair(permit, &clean_turn(), &fingerprint)
            .unwrap();
        let rebuild = guard.rebuild_permit().unwrap();
        guard.mark_rebuild_prepared(rebuild).unwrap();
        let second = failed(project, b"error: different");
        assert!(matches!(
            guard.authorize_repair(&second).unwrap(),
            RepairAuthorization::Stop(BuildRepairLoopStopReason::RetryBudgetExhausted)
        ));
    }

    #[test]
    fn cancellation_stops_at_next_guard_boundary() {
        let project = ProjectId::new();
        let mut guard = BuildRepairLoopGuard::new(
            project,
            BuildTargetKind::Website,
            BuildRepairLoopPolicy::default(),
        )
        .unwrap();
        guard.cancellation().request();
        let first = failed(project, b"error: cancelled");
        assert!(matches!(
            guard.authorize_repair(&first).unwrap(),
            RepairAuthorization::Stop(BuildRepairLoopStopReason::Cancelled)
        ));
    }
}
