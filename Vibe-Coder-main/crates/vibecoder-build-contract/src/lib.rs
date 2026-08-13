//! Provider-neutral build-job lifecycle and normalized result model.
//!
//! Part 18 deliberately does not discover toolchains or decide how a website/Android project is
//! built. It wraps one already-authorized Part 15 process with build identity/lifecycle semantics
//! and a stable result shape. Raw stdout/stderr remains bounded, non-persisted process evidence;
//! Part 21 parses bounded repair evidence from it without persisting raw process output.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use vibecoder_domain::{ProjectId, Result, VibeCoderError};
use vibecoder_process_contract::{
    MAX_EVENT_DRAIN, ProcessEvent, ProcessId, ProcessResult, ProcessTermination, RunningProcess,
};

pub const MAX_BUILD_DIAGNOSTICS: usize = 512;
pub const MAX_BUILD_ARTIFACTS: usize = 64;
pub const MAX_DIAGNOSTIC_CODE_BYTES: usize = 256;
pub const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_ARTIFACT_RELATIVE_PATH_BYTES: usize = 4096;
pub const MAX_ARTIFACT_PATH_COMPONENT_BYTES: usize = 255;
const INTERNAL_TEMP_PREFIX: &str = ".vibecoder-tmp-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildId(Uuid);

impl BuildId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for BuildId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTargetKind {
    Website,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl BuildState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildDiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildArtifactKind {
    WebsiteBundle,
    AndroidApk,
    AndroidAab,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildDiagnostic {
    severity: BuildDiagnosticSeverity,
    code: Option<String>,
    message: String,
    relative_path: Option<PathBuf>,
    line: Option<u32>,
    column: Option<u32>,
}

impl BuildDiagnostic {
    pub fn new(
        severity: BuildDiagnosticSeverity,
        code: Option<String>,
        message: String,
        relative_path: Option<PathBuf>,
        line: Option<u32>,
        column: Option<u32>,
    ) -> Result<Self> {
        if message.is_empty()
            || message.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES
            || message
                .chars()
                .any(|ch| char::is_control(ch) || is_bidi_control(ch))
        {
            return Err(build_error("build_diagnostic_message_invalid"));
        }
        if let Some(code) = code.as_ref()
            && (code.is_empty()
                || code.len() > MAX_DIAGNOSTIC_CODE_BYTES
                || code
                    .chars()
                    .any(|ch| char::is_control(ch) || is_bidi_control(ch)))
        {
            return Err(build_error("build_diagnostic_code_invalid"));
        }
        if let Some(path) = relative_path.as_ref() {
            validate_relative_artifact_path(path)?;
        }
        if line == Some(0) || column == Some(0) {
            return Err(build_error("build_diagnostic_location_invalid"));
        }
        Ok(Self {
            severity,
            code,
            message,
            relative_path,
            line,
            column,
        })
    }

    pub const fn severity(&self) -> BuildDiagnosticSeverity {
        self.severity
    }
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn relative_path(&self) -> Option<&Path> {
        self.relative_path.as_deref()
    }
    pub const fn line(&self) -> Option<u32> {
        self.line
    }
    pub const fn column(&self) -> Option<u32> {
        self.column
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArtifact {
    kind: BuildArtifactKind,
    relative_path: PathBuf,
    size_bytes: u64,
    /// Optional lowercase SHA-256 recorded by a later artifact-discovery layer. Part 18 does not
    /// verify project bytes itself, so presence of this value is evidence metadata, not authority.
    sha256_hex: Option<String>,
}

impl BuildArtifact {
    pub fn new(
        kind: BuildArtifactKind,
        relative_path: PathBuf,
        size_bytes: u64,
        sha256_hex: Option<String>,
    ) -> Result<Self> {
        validate_relative_artifact_path(&relative_path)?;
        if let Some(digest) = sha256_hex.as_ref()
            && (digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        {
            return Err(build_error("build_artifact_sha256_invalid"));
        }
        Ok(Self {
            kind,
            relative_path,
            size_bytes,
            sha256_hex,
        })
    }

    pub const fn kind(&self) -> BuildArtifactKind {
        self.kind
    }
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
    pub fn recorded_sha256(&self) -> Option<&str> {
        self.sha256_hex.as_deref()
    }
}

#[derive(PartialEq, Eq)]
pub struct BuildOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    live_event_queue_overflowed: bool,
}

impl BuildOutput {
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
    pub const fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }
    pub const fn live_event_queue_overflowed(&self) -> bool {
        self.live_event_queue_overflowed
    }
}

impl fmt::Debug for BuildOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildOutput")
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
            .field(
                "live_event_queue_overflowed",
                &self.live_event_queue_overflowed,
            )
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct BuildJobDescriptor {
    build_id: BuildId,
    project_id: ProjectId,
    target: BuildTargetKind,
}

impl BuildJobDescriptor {
    pub fn new(project_id: ProjectId, target: BuildTargetKind) -> Self {
        Self {
            build_id: BuildId::new(),
            project_id,
            target,
        }
    }

    pub const fn build_id(&self) -> BuildId {
        self.build_id
    }
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn target(&self) -> BuildTargetKind {
        self.target
    }
    pub const fn queued_state(&self) -> BuildState {
        BuildState::Queued
    }
    pub fn queued_event(&self) -> BuildEvent {
        BuildEvent::Queued {
            build_id: self.build_id,
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct BuildResult {
    build_id: BuildId,
    project_id: ProjectId,
    target: BuildTargetKind,
    state: BuildState,
    process_id: ProcessId,
    exit_code: Option<i32>,
    duration_ms: u64,
    output: BuildOutput,
    diagnostics: Vec<BuildDiagnostic>,
    artifacts: Vec<BuildArtifact>,
}

impl fmt::Debug for BuildResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildResult")
            .field("build_id", &self.build_id)
            .field("project_id", &self.project_id)
            .field("target", &self.target)
            .field("state", &self.state)
            .field("process_id", &self.process_id)
            .field("exit_code", &self.exit_code)
            .field("duration_ms", &self.duration_ms)
            .field("output", &self.output)
            .field("diagnostic_count", &self.diagnostics.len())
            .field("artifact_count", &self.artifacts.len())
            .finish()
    }
}

impl BuildResult {
    pub fn from_process_result(descriptor: BuildJobDescriptor, process: ProcessResult) -> Self {
        let state = match process.termination {
            ProcessTermination::Exited if process.exit_code == Some(0) => BuildState::Succeeded,
            ProcessTermination::Exited | ProcessTermination::Signaled => BuildState::Failed,
            ProcessTermination::TimedOut => BuildState::TimedOut,
            ProcessTermination::Cancelled => BuildState::Cancelled,
        };
        Self {
            build_id: descriptor.build_id,
            project_id: descriptor.project_id,
            target: descriptor.target,
            state,
            process_id: process.process_id,
            exit_code: process.exit_code,
            duration_ms: process.duration_ms,
            output: BuildOutput {
                stdout: process.stdout,
                stderr: process.stderr,
                stdout_truncated: process.stdout_truncated,
                stderr_truncated: process.stderr_truncated,
                live_event_queue_overflowed: process.event_queue_overflowed,
            },
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    pub const fn build_id(&self) -> BuildId {
        self.build_id
    }
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn target(&self) -> BuildTargetKind {
        self.target
    }
    pub const fn state(&self) -> BuildState {
        self.state
    }
    pub const fn process_id(&self) -> ProcessId {
        self.process_id
    }
    pub const fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
    pub const fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
    pub fn output(&self) -> &BuildOutput {
        &self.output
    }
    pub fn diagnostics(&self) -> &[BuildDiagnostic] {
        &self.diagnostics
    }
    pub fn artifacts(&self) -> &[BuildArtifact] {
        &self.artifacts
    }
    pub fn success(&self) -> bool {
        matches!(self.state, BuildState::Succeeded)
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<BuildDiagnostic>) -> Result<()> {
        if diagnostics.len() > MAX_BUILD_DIAGNOSTICS {
            return Err(build_error("build_diagnostic_limit"));
        }
        self.diagnostics = diagnostics;
        Ok(())
    }

    /// Attach already-normalized artifact metadata. This method validates metadata shape only; it
    /// deliberately does not claim that the referenced project file currently exists or matches
    /// the optional recorded digest. Toolchain-specific artifact discovery must prove that later.
    pub fn set_artifacts(&mut self, artifacts: Vec<BuildArtifact>) -> Result<()> {
        if artifacts.len() > MAX_BUILD_ARTIFACTS {
            return Err(build_error("build_artifact_limit"));
        }
        let mut paths = HashSet::with_capacity(artifacts.len());
        for artifact in &artifacts {
            if !paths.insert(artifact.relative_path.clone()) {
                return Err(build_error("build_artifact_duplicate_path"));
            }
        }
        self.artifacts = artifacts;
        Ok(())
    }
}

pub enum BuildEvent {
    Queued {
        build_id: BuildId,
    },
    Started {
        build_id: BuildId,
        process_id: ProcessId,
    },
    Output {
        build_id: BuildId,
        stream: BuildOutputStream,
        bytes: Vec<u8>,
    },
    EventQueueOverflow {
        build_id: BuildId,
    },
    Finished {
        build_id: BuildId,
        state: BuildState,
        exit_code: Option<i32>,
    },
}

impl fmt::Debug for BuildEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued { build_id } => formatter
                .debug_struct("Queued")
                .field("build_id", build_id)
                .finish(),
            Self::Started {
                build_id,
                process_id,
            } => formatter
                .debug_struct("Started")
                .field("build_id", build_id)
                .field("process_id", process_id)
                .finish(),
            Self::Output {
                build_id,
                stream,
                bytes,
            } => formatter
                .debug_struct("Output")
                .field("build_id", build_id)
                .field("stream", stream)
                .field(
                    "bytes",
                    &format_args!("[REDACTED; {} byte(s)]", bytes.len()),
                )
                .finish(),
            Self::EventQueueOverflow { build_id } => formatter
                .debug_struct("EventQueueOverflow")
                .field("build_id", build_id)
                .finish(),
            Self::Finished {
                build_id,
                state,
                exit_code,
            } => formatter
                .debug_struct("Finished")
                .field("build_id", build_id)
                .field("state", state)
                .field("exit_code", exit_code)
                .finish(),
        }
    }
}

pub struct RunningBuildJob {
    descriptor: BuildJobDescriptor,
    process: RunningProcess,
}

impl fmt::Debug for RunningBuildJob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunningBuildJob")
            .field("descriptor", &self.descriptor)
            .field("process_id", &self.process.process_id())
            .finish_non_exhaustive()
    }
}

impl RunningBuildJob {
    pub fn from_running_process(descriptor: BuildJobDescriptor, process: RunningProcess) -> Self {
        Self {
            descriptor,
            process,
        }
    }

    pub const fn build_id(&self) -> BuildId {
        self.descriptor.build_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.descriptor.project_id
    }

    pub const fn target(&self) -> BuildTargetKind {
        self.descriptor.target
    }

    pub const fn process_id(&self) -> ProcessId {
        self.process.process_id()
    }

    pub const fn state(&self) -> BuildState {
        BuildState::Running
    }

    pub fn drain_events(&self, max_events: usize) -> Result<Vec<BuildEvent>> {
        if max_events == 0 || max_events > MAX_EVENT_DRAIN {
            return Err(build_error("build_event_drain_limit"));
        }
        self.process
            .drain_events(max_events)?
            .into_iter()
            .map(|event| normalize_event(self.descriptor.build_id, event))
            .collect()
    }

    pub async fn wait(self) -> Result<BuildResult> {
        let process = self.process.wait().await?;
        Ok(BuildResult::from_process_result(self.descriptor, process))
    }
}

fn normalize_event(build_id: BuildId, event: ProcessEvent) -> Result<BuildEvent> {
    Ok(match event {
        ProcessEvent::Started { process_id } => BuildEvent::Started {
            build_id,
            process_id,
        },
        ProcessEvent::Stdout { bytes } => BuildEvent::Output {
            build_id,
            stream: BuildOutputStream::Stdout,
            bytes,
        },
        ProcessEvent::Stderr { bytes } => BuildEvent::Output {
            build_id,
            stream: BuildOutputStream::Stderr,
            bytes,
        },
        ProcessEvent::EventQueueOverflow => BuildEvent::EventQueueOverflow { build_id },
        ProcessEvent::Finished {
            process_id: _,
            termination,
            exit_code,
        } => BuildEvent::Finished {
            build_id,
            state: state_from_termination(termination, exit_code),
            exit_code,
        },
    })
}

fn state_from_termination(termination: ProcessTermination, exit_code: Option<i32>) -> BuildState {
    match termination {
        ProcessTermination::Exited if matches!(exit_code, Some(0)) => BuildState::Succeeded,
        ProcessTermination::Exited | ProcessTermination::Signaled => BuildState::Failed,
        ProcessTermination::TimedOut => BuildState::TimedOut,
        ProcessTermination::Cancelled => BuildState::Cancelled,
    }
}

fn validate_relative_artifact_path(path: &Path) -> Result<()> {
    let raw = path
        .to_str()
        .ok_or_else(|| build_error("build_relative_path_invalid"))?;
    if raw.is_empty()
        || raw.len() > MAX_ARTIFACT_RELATIVE_PATH_BYTES
        || raw.contains('\\')
        || raw
            .chars()
            .any(|ch| char::is_control(ch) || is_bidi_control(ch))
    {
        return Err(build_error("build_relative_path_invalid"));
    }
    let mut normalized = String::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(build_error("build_relative_path_invalid"));
        };
        let value = value
            .to_str()
            .ok_or_else(|| build_error("build_relative_path_invalid"))?;
        if value.is_empty()
            || value.len() > MAX_ARTIFACT_PATH_COMPONENT_BYTES
            || value.starts_with(INTERNAL_TEMP_PREFIX)
        {
            return Err(build_error("build_relative_path_invalid"));
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(value);
    }
    if normalized.is_empty() || normalized != raw {
        return Err(build_error("build_relative_path_invalid"));
    }
    Ok(())
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'
            | '\u{202b}'
            | '\u{202c}'
            | '\u{202d}'
            | '\u{202e}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
    )
}

fn build_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Build(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_result(termination: ProcessTermination, exit_code: Option<i32>) -> ProcessResult {
        ProcessResult {
            process_id: ProcessId::new(),
            termination,
            exit_code,
            stdout: b"stdout".to_vec(),
            stderr: b"stderr".to_vec(),
            stdout_truncated: false,
            stderr_truncated: false,
            event_queue_overflowed: false,
            duration_ms: 123,
        }
    }

    #[test]
    fn exit_zero_is_success_and_nonzero_is_failure() {
        let project = ProjectId::new();
        let ok = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Android),
            process_result(ProcessTermination::Exited, Some(0)),
        );
        assert_eq!(ok.state(), BuildState::Succeeded);
        let failed = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Android),
            process_result(ProcessTermination::Exited, Some(1)),
        );
        assert_eq!(failed.state(), BuildState::Failed);
    }

    #[test]
    fn timeout_and_cancel_stay_distinct() {
        let project = ProjectId::new();
        let timed_out = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Website),
            process_result(ProcessTermination::TimedOut, None),
        );
        assert_eq!(timed_out.state(), BuildState::TimedOut);
        let cancelled = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Website),
            process_result(ProcessTermination::Cancelled, None),
        );
        assert_eq!(cancelled.state(), BuildState::Cancelled);
    }

    #[test]
    fn artifact_path_and_digest_are_strict() {
        assert!(
            BuildArtifact::new(
                BuildArtifactKind::AndroidApk,
                PathBuf::from("app/build/outputs/apk/debug/app-debug.apk"),
                10,
                Some("a".repeat(64)),
            )
            .is_ok()
        );
        assert!(
            BuildArtifact::new(
                BuildArtifactKind::AndroidApk,
                PathBuf::from("../escape.apk"),
                10,
                Some("a".repeat(64)),
            )
            .is_err()
        );
        assert!(
            BuildArtifact::new(
                BuildArtifactKind::AndroidApk,
                PathBuf::from("app.apk"),
                10,
                Some("A".repeat(64)),
            )
            .is_err()
        );
    }

    #[test]
    fn artifact_path_requires_canonical_relative_spelling() {
        for bad in ["./app.apk", "out//app.apk", "out/./app.apk", "out/app.apk/"] {
            assert!(
                BuildArtifact::new(BuildArtifactKind::AndroidApk, PathBuf::from(bad), 1, None,)
                    .is_err()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_artifact_path_is_rejected() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(OsString::from_vec(vec![b'a', 0xff, b'b']));
        assert!(BuildArtifact::new(BuildArtifactKind::Other, path, 1, None).is_err());
    }

    #[test]
    fn bidi_spoofing_is_rejected_from_normalized_metadata() {
        assert!(
            BuildDiagnostic::new(
                BuildDiagnosticSeverity::Error,
                None,
                "safe\u{202e}spoof".into(),
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(
            BuildArtifact::new(
                BuildArtifactKind::Other,
                PathBuf::from("safe\u{202e}spoof.apk"),
                1,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn duplicate_artifact_paths_are_rejected() {
        let project = ProjectId::new();
        let mut result = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Android),
            process_result(ProcessTermination::Exited, Some(0)),
        );
        let first = BuildArtifact::new(
            BuildArtifactKind::AndroidApk,
            PathBuf::from("app-debug.apk"),
            10,
            None,
        )
        .unwrap();
        let second = BuildArtifact::new(
            BuildArtifactKind::AndroidApk,
            PathBuf::from("app-debug.apk"),
            10,
            None,
        )
        .unwrap();
        assert!(result.set_artifacts(vec![first, second]).is_err());
    }

    #[test]
    fn process_success_does_not_create_artifact_claim() {
        let project = ProjectId::new();
        let mut result = BuildResult::from_process_result(
            BuildJobDescriptor::new(project, BuildTargetKind::Android),
            process_result(ProcessTermination::Exited, Some(0)),
        );
        assert!(result.success());
        assert!(result.artifacts().is_empty());
        let candidate = BuildArtifact::new(
            BuildArtifactKind::AndroidApk,
            PathBuf::from("app-debug.apk"),
            10,
            None,
        )
        .unwrap();
        result.set_artifacts(vec![candidate]).unwrap();
        assert_eq!(result.artifacts()[0].recorded_sha256(), None);
    }
}
