//! Provider-neutral lifecycle contract for one locally executed, explicitly authorized command.
//!
//! Part 15 keeps authorization and execution separate. A process runtime can only start a command
//! after consuming the non-forgeable `CommandExecutionEnvelope` produced by the command-policy
//! broker. Process handles are ephemeral and deliberately non-serializable.

use futures_channel::oneshot;
use std::fmt;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;
use uuid::Uuid;
use vibecoder_command_policy::CommandExecutionEnvelope;
use vibecoder_domain::{ProjectId, ProjectRef, Result, VibeCoderError};

pub const DEFAULT_TIMEOUT_MS: u64 = 10 * 60 * 1000;
pub const MIN_TIMEOUT_MS: u64 = 1_000;
pub const MAX_TIMEOUT_MS: u64 = 30 * 60 * 1000;
pub const DEFAULT_STDOUT_LIMIT_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_STDERR_LIMIT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_STREAM_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_EVENT_DRAIN: usize = 256;
/// After cancellation/timeout the Unix local runtime keeps the process-group leader unreaped for
/// this grace interval before SIGKILL escalation. This intentionally adds up to 250 ms to a
/// cancellation result even when the leader exits immediately after SIGTERM, preventing process
/// group ID reuse before descendants are escalated.
pub const PROCESS_TERMINATION_GRACE_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(Uuid);

impl ProcessId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ProcessId {
    fn default() -> Self {
        Self::new()
    }
}

/// Bounded execution settings. Cancellation/timeout completion also includes the fixed
/// `PROCESS_TERMINATION_GRACE_MS` process-group safety window on Unix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExecutionOptions {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl ProcessExecutionOptions {
    pub fn validate(self) -> Result<Self> {
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&self.timeout_ms) {
            return Err(process_error("process_timeout_out_of_range"));
        }
        if self.max_stdout_bytes > MAX_STREAM_CAPTURE_BYTES
            || self.max_stderr_bytes > MAX_STREAM_CAPTURE_BYTES
        {
            return Err(process_error("process_capture_limit_out_of_range"));
        }
        Ok(self)
    }

    pub fn timeout(self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

impl Default for ProcessExecutionOptions {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_stdout_bytes: DEFAULT_STDOUT_LIMIT_BYTES,
            max_stderr_bytes: DEFAULT_STDERR_LIMIT_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessRuntimeCapabilities {
    pub local_execution: bool,
    pub cancellation: bool,
    pub timeout: bool,
    pub bounded_output_capture: bool,
    pub process_group_termination: bool,
    pub strong_process_isolation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessTermination {
    Exited,
    Signaled,
    TimedOut,
    Cancelled,
}

#[derive(PartialEq, Eq)]
pub enum ProcessEvent {
    Started {
        process_id: ProcessId,
    },
    Stdout {
        bytes: Vec<u8>,
    },
    Stderr {
        bytes: Vec<u8>,
    },
    EventQueueOverflow,
    Finished {
        process_id: ProcessId,
        termination: ProcessTermination,
        exit_code: Option<i32>,
    },
}

impl fmt::Debug for ProcessEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Started { process_id } => formatter
                .debug_struct("Started")
                .field("process_id", process_id)
                .finish(),
            Self::Stdout { bytes } => formatter
                .debug_struct("Stdout")
                .field(
                    "bytes",
                    &format_args!("[REDACTED; {} byte(s)]", bytes.len()),
                )
                .finish(),
            Self::Stderr { bytes } => formatter
                .debug_struct("Stderr")
                .field(
                    "bytes",
                    &format_args!("[REDACTED; {} byte(s)]", bytes.len()),
                )
                .finish(),
            Self::EventQueueOverflow => formatter.write_str("EventQueueOverflow"),
            Self::Finished {
                process_id,
                termination,
                exit_code,
            } => formatter
                .debug_struct("Finished")
                .field("process_id", process_id)
                .field("termination", termination)
                .field("exit_code", exit_code)
                .finish(),
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct ProcessResult {
    pub process_id: ProcessId,
    pub termination: ProcessTermination,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub event_queue_overflowed: bool,
    pub duration_ms: u64,
}

impl ProcessResult {
    pub fn success(&self) -> bool {
        self.termination == ProcessTermination::Exited && self.exit_code == Some(0)
    }
}

impl fmt::Debug for ProcessResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessResult")
            .field("process_id", &self.process_id)
            .field("termination", &self.termination)
            .field("exit_code", &self.exit_code)
            .field(
                "stdout",
                &format_args!("[REDACTED; {} byte(s)]", self.stdout.len()),
            )
            .field(
                "stderr",
                &format_args!("[REDACTED; {} byte(s)]", self.stderr.len()),
            )
            .field("stdout_truncated", &self.stdout_truncated)
            .field("stderr_truncated", &self.stderr_truncated)
            .field("event_queue_overflowed", &self.event_queue_overflowed)
            .field("duration_ms", &self.duration_ms)
            .finish()
    }
}

/// One non-persistable process observation handle. Dropping the handle does not cancel the process;
/// cancellation is explicit through `ProcessRuntime::cancel` so UI lifecycle changes cannot kill a
/// build by accident.
pub struct RunningProcess {
    process_id: ProcessId,
    events: Mutex<Receiver<ProcessEvent>>,
    completion: oneshot::Receiver<Result<ProcessResult>>,
}

impl fmt::Debug for RunningProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunningProcess")
            .field("process_id", &self.process_id)
            .finish_non_exhaustive()
    }
}

impl RunningProcess {
    #[doc(hidden)]
    pub fn from_channels(
        process_id: ProcessId,
        events: Receiver<ProcessEvent>,
        completion: oneshot::Receiver<Result<ProcessResult>>,
    ) -> Self {
        Self {
            process_id,
            events: Mutex::new(events),
            completion,
        }
    }

    pub const fn process_id(&self) -> ProcessId {
        self.process_id
    }

    /// Drain at most `max_events` currently buffered events without blocking.
    pub fn drain_events(&self, max_events: usize) -> Result<Vec<ProcessEvent>> {
        if max_events == 0 || max_events > MAX_EVENT_DRAIN {
            return Err(process_error("process_event_drain_limit"));
        }
        let receiver = self
            .events
            .lock()
            .map_err(|_| process_error("process_event_queue_poisoned"))?;
        let mut events = Vec::new();
        while events.len() < max_events {
            match receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        Ok(events)
    }

    pub async fn wait(self) -> Result<ProcessResult> {
        self.completion
            .await
            .map_err(|_| process_error("process_completion_channel_closed"))?
    }
}

pub trait ProcessRuntime: Send + Sync {
    fn capabilities(&self) -> ProcessRuntimeCapabilities;

    /// Consume one allow-once envelope and start exactly one local process.
    fn start(
        &self,
        project: &ProjectRef,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningProcess>;

    /// Number of active local processes currently scoped to this project. Snapshot/rollback uses
    /// this as a fail-closed quiescence guard before replacing project-directory identity.
    fn active_for_project(&self, project_id: ProjectId) -> Result<usize>;

    /// Request cancellation. The runtime must terminate the process group rather than only the
    /// direct child where the target platform supports that safely.
    fn cancel(&self, process_id: ProcessId) -> Result<()>;
}

fn process_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Process(code.into())
}
