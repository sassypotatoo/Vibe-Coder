//! Android/Unix phone-local process executor, hardened in Part 26 for Android W^X.
//!
//! The executor accepts only a consumed allow-once command envelope. It does not perform PATH
//! lookup, does not inherit the ambient environment, keeps stdin closed, bounds captured output,
//! places each child in its own process group, and enforces cancellation/timeout from a supervisor
//! thread. Runtime executables come from a separate package-installed code root; writable app-private
//! runtime data is never treated as executable code. Strong kernel sandboxing is deliberately not
//! claimed here.

use futures_channel::oneshot;
use std::collections::{HashMap, hash_map::Entry};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use vibecoder_command_policy::{
    AuthorizedCommand, CommandEnvironmentPolicy, CommandExecutionEnvelope, CommandProgram,
};
use vibecoder_domain::{ProjectId, ProjectRef, Result, VibeCoderError};
use vibecoder_process_contract::{
    ProcessEvent, ProcessExecutionOptions, ProcessId, ProcessResult, ProcessRuntime,
    PROCESS_TERMINATION_GRACE_MS,
    ProcessRuntimeCapabilities, ProcessTermination, RunningProcess,
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

const PRODUCT_ROOT_NAME: &str = "vibecoder";
const PROJECTS_ROOT_NAME: &str = "projects";
const RUNTIME_ROOT_NAME: &str = "runtime";
const PROCESS_HOME_NAME: &str = "process-home";
const PROCESS_TMP_NAME: &str = "process-tmp";
const INTERNAL_TEMP_PREFIX: &str = ".vibecoder-tmp-";
const MAX_RELATIVE_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENT_BYTES: usize = 255;
const MAX_RUNTIME_TOOL_ID_BYTES: usize = 64;
const MAX_RUNTIME_TOOLS: usize = 64;
const MAX_RUNTIME_FIXED_ARGS: usize = 32;
const MAX_RUNTIME_FIXED_ARG_BYTES: usize = 4096;
const MAX_ACTIVE_PROCESSES: usize = 4;
const MAX_ACTIVE_PER_PROJECT: usize = 2;
const EVENT_QUEUE_CAPACITY: usize = 256;
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const MAX_PIPE_CHUNKS_PER_POLL: usize = 8;
const SUPERVISOR_POLL_MS: u64 = 10;
const EXIT_PIPE_DRAIN_GRACE_MS: u64 = 250;

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeToolSpec {
    tool_id: String,
    relative_path: PathBuf,
    fixed_args: Vec<String>,
}

impl fmt::Debug for RuntimeToolSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeToolSpec")
            .field("tool_id", &self.tool_id)
            .field("relative_path", &self.relative_path)
            .field("fixed_arg_count", &self.fixed_args.len())
            .finish()
    }
}

impl RuntimeToolSpec {
    pub fn new(tool_id: impl Into<String>, relative_path: impl Into<PathBuf>) -> Result<Self> {
        Self::with_fixed_args(tool_id, relative_path, std::iter::empty::<String>())
    }

    /// Register a package-installed executable with a trusted, non-shell fixed argv prefix.
    ///
    /// Android uses this for script-based tools such as npm: the executable authority remains the
    /// packaged Node binary while the fixed first argument points at a separately verified npm CLI
    /// data asset. Project-controlled arguments are appended only after this trusted prefix.
    pub fn with_fixed_args<I, S>(
        tool_id: impl Into<String>,
        relative_path: impl Into<PathBuf>,
        fixed_args: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let tool_id = validate_runtime_tool_id(tool_id.into())?;
        let relative_path = normalize_relative_path(&relative_path.into(), false)?;
        let mut trusted_args = Vec::new();
        for value in fixed_args {
            if trusted_args.len() >= MAX_RUNTIME_FIXED_ARGS {
                return Err(process_error("process_runtime_fixed_arg_limit"));
            }
            let value = value.into();
            validate_runtime_fixed_arg(&value)?;
            trusted_args.push(value);
        }
        Ok(Self {
            tool_id,
            relative_path,
            fixed_args: trusted_args,
        })
    }

    pub fn tool_id(&self) -> &str {
        &self.tool_id
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn fixed_args(&self) -> &[String] {
        &self.fixed_args
    }
}

#[derive(Debug)]
struct ActiveProcess {
    project_id: ProjectId,
    cancel_requested: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct ProcessRegistry {
    active: HashMap<ProcessId, ActiveProcess>,
}

pub struct LocalProcessRuntime {
    app_private_root: PathBuf,
    product_root: PathBuf,
    projects_root: PathBuf,
    runtime_root: PathBuf,
    packaged_executable_root: PathBuf,
    process_home: PathBuf,
    process_tmp: PathBuf,
    runtime_tools: HashMap<String, RuntimeToolSpec>,
    registry: Arc<Mutex<ProcessRegistry>>,
}

impl fmt::Debug for LocalProcessRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalProcessRuntime")
            .field("app_private_root", &"[REDACTED]")
            .field("runtime_root", &"[REDACTED]")
            .field("packaged_executable_root", &"[REDACTED]")
            .field("runtime_tool_count", &self.runtime_tools.len())
            .finish_non_exhaustive()
    }
}

impl LocalProcessRuntime {
    /// Initialize from two platform-owned roots:
    /// - `app_private_dir`: writable app data used for projects, HOME, TMPDIR and runtime state;
    /// - `packaged_executable_dir`: package-owned code exposed as ordinary filesystem files. On
    ///   Android this root is supplied by the Android host and must not be writable app home. It is
    ///   intentionally not inferred from `nativeLibraryDir`, because modern APK packaging may load
    ///   JNI libraries directly from the APK instead of extracting them.
    ///
    /// Runtime tool definitions are relative to `packaged_executable_dir` and are resolved again
    /// at every spawn. This separation is mandatory because Android API 29+ forbids direct execve
    /// of files from the writable app home directory.
    pub fn initialize(
        app_private_dir: impl AsRef<Path>,
        packaged_executable_dir: impl AsRef<Path>,
        runtime_tools: impl IntoIterator<Item = RuntimeToolSpec>,
    ) -> Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = (
                app_private_dir.as_ref(),
                packaged_executable_dir.as_ref(),
                runtime_tools.into_iter(),
            );
            return Err(process_error("process_runtime_unsupported_platform"));
        }

        #[cfg(unix)]
        {
            if !app_private_dir.as_ref().is_absolute() {
                return Err(process_error("process_app_private_root_not_absolute"));
            }
            if !packaged_executable_dir.as_ref().is_absolute() {
                return Err(process_error("process_packaged_code_root_not_absolute"));
            }
            let app_private_root = canonical_existing_directory(app_private_dir.as_ref())?;
            let packaged_executable_root =
                canonical_existing_directory(packaged_executable_dir.as_ref())?;
            ensure_execution_root_separate_from_writable_home(
                &app_private_root,
                &packaged_executable_root,
            )?;
            #[cfg(target_os = "android")]
            verify_android_packaged_code_directory(&packaged_executable_root)?;
            let product_root =
                ensure_fixed_private_directory(&app_private_root, PRODUCT_ROOT_NAME)?;
            let projects_root = ensure_fixed_private_directory(&product_root, PROJECTS_ROOT_NAME)?;
            let runtime_root = ensure_fixed_private_directory(&product_root, RUNTIME_ROOT_NAME)?;
            let process_home = ensure_fixed_private_directory(&runtime_root, PROCESS_HOME_NAME)?;
            let process_tmp = ensure_fixed_private_directory(&runtime_root, PROCESS_TMP_NAME)?;

            let mut tools = HashMap::new();
            for spec in runtime_tools {
                if tools.len() >= MAX_RUNTIME_TOOLS {
                    return Err(process_error("process_runtime_tool_limit"));
                }
                match tools.entry(spec.tool_id.clone()) {
                    Entry::Vacant(entry) => {
                        entry.insert(spec);
                    }
                    Entry::Occupied(_) => {
                        return Err(process_error("process_runtime_tool_duplicate"));
                    }
                }
            }

            Ok(Self {
                app_private_root,
                product_root,
                projects_root,
                runtime_root,
                packaged_executable_root,
                process_home,
                process_tmp,
                runtime_tools: tools,
                registry: Arc::new(Mutex::new(ProcessRegistry::default())),
            })
        }
    }

    pub fn app_private_root(&self) -> &Path {
        &self.app_private_root
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn packaged_executable_root(&self) -> &Path {
        &self.packaged_executable_root
    }

    pub fn active_count(&self) -> Result<usize> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| process_error("process_registry_poisoned"))?;
        Ok(registry.active.len())
    }

    fn reserve_process(&self, project_id: ProjectId) -> Result<(ProcessId, Arc<AtomicBool>)> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| process_error("process_registry_poisoned"))?;
        if registry.active.len() >= MAX_ACTIVE_PROCESSES {
            return Err(process_error("process_active_limit"));
        }
        if registry
            .active
            .values()
            .filter(|active| active.project_id == project_id)
            .count()
            >= MAX_ACTIVE_PER_PROJECT
        {
            return Err(process_error("process_project_active_limit"));
        }

        for _ in 0..8 {
            let process_id = ProcessId::new();
            let cancel_requested = Arc::new(AtomicBool::new(false));
            match registry.active.entry(process_id) {
                Entry::Vacant(entry) => {
                    entry.insert(ActiveProcess {
                        project_id,
                        cancel_requested: Arc::clone(&cancel_requested),
                    });
                    return Ok((process_id, cancel_requested));
                }
                Entry::Occupied(_) => continue,
            }
        }
        Err(process_error("process_id_collision"))
    }

    fn release_process(&self, process_id: ProcessId) {
        if let Ok(mut registry) = self.registry.lock() {
            registry.active.remove(&process_id);
        }
    }

    fn verify_project_root(&self, project: &ProjectRef) -> Result<PathBuf> {
        verify_no_symlink_directory(&self.app_private_root)?;
        verify_exact_child_directory(
            &self.app_private_root,
            &self.product_root,
            PRODUCT_ROOT_NAME,
        )?;
        verify_exact_child_directory(&self.product_root, &self.projects_root, PROJECTS_ROOT_NAME)?;

        let expected = self
            .projects_root
            .join(project.id.0.hyphenated().to_string());
        let canonical = canonical_existing_directory(&expected)?;
        if canonical != expected || project.root != canonical {
            return Err(process_error("process_project_reference_mismatch"));
        }
        verify_no_symlink_directory(&canonical)?;
        Ok(canonical)
    }

    fn resolve_working_directory(&self, project_root: &Path, relative: &Path) -> Result<PathBuf> {
        let normalized = normalize_relative_path(relative, true)?;
        let mut current = project_root.to_path_buf();
        for component in normalized.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            current.push(name);
            verify_no_symlink_directory(&current)?;
        }
        Ok(current)
    }

    fn resolve_runtime_tool(&self, tool_id: &str) -> Result<(PathBuf, Vec<String>)> {
        let spec = self
            .runtime_tools
            .get(tool_id)
            .ok_or_else(|| process_error("process_runtime_tool_unregistered"))?;
        verify_no_symlink_directory(&self.packaged_executable_root)?;
        #[cfg(target_os = "android")]
        verify_android_packaged_code_directory(&self.packaged_executable_root)?;
        let executable = resolve_executable_beneath(
            &self.packaged_executable_root,
            &spec.relative_path,
            "process_runtime_tool_invalid",
        )?;
        #[cfg(target_os = "android")]
        verify_android_packaged_code_file(&executable)?;
        Ok((executable, spec.fixed_args.clone()))
    }

    fn resolve_workspace_executable(
        &self,
        project_root: &Path,
        relative: &Path,
    ) -> Result<PathBuf> {
        #[cfg(target_os = "android")]
        {
            // Android API 29+ does not permit direct execve from the writable app home. Project
            // workspaces live beneath that writable root, so a workspace file cannot become code
            // authority merely because its mode has an execute bit. Scripts must run through a
            // trusted package-installed interpreter/runtime tool instead.
            let _ = (project_root, relative);
            return Err(process_error(
                "process_workspace_executable_android_wx_forbidden",
            ));
        }

        #[cfg(not(target_os = "android"))]
        {
            let normalized = normalize_relative_path(relative, false)?;
            resolve_executable_beneath(
                project_root,
                &normalized,
                "process_workspace_executable_invalid",
            )
        }
    }

    fn prepare_command(
        &self,
        project: &ProjectRef,
        authorized: &AuthorizedCommand,
    ) -> Result<Command> {
        if authorized.project_id() != project.id {
            return Err(process_error("process_authorization_project_mismatch"));
        }
        if authorized.environment_policy() != CommandEnvironmentPolicy::RuntimeManagedClean {
            return Err(process_error("process_environment_policy_invalid"));
        }

        let project_root = self.verify_project_root(project)?;
        let working_dir =
            self.resolve_working_directory(&project_root, &authorized.command().working_dir)?;
        let (executable, fixed_args) = match &authorized.command().program {
            CommandProgram::RuntimeTool { tool_id } => self.resolve_runtime_tool(tool_id)?,
            CommandProgram::WorkspaceExecutable { relative_path } => (
                self.resolve_workspace_executable(&project_root, relative_path)?,
                Vec::new(),
            ),
        };

        // Recheck all mutable execution-time objects immediately before creating the command.
        verify_no_symlink_directory(&working_dir)?;
        verify_executable_file(&executable)?;
        verify_no_symlink_directory(&self.process_home)?;
        verify_no_symlink_directory(&self.process_tmp)?;

        let mut command = Command::new(executable);
        command
            .args(&fixed_args)
            .args(&authorized.command().args)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .env("HOME", &self.process_home)
            .env("TMPDIR", &self.process_tmp);

        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }

        Ok(command)
    }
}

#[cfg(unix)]
struct SupervisorPayload {
    child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

impl ProcessRuntime for LocalProcessRuntime {
    fn capabilities(&self) -> ProcessRuntimeCapabilities {
        ProcessRuntimeCapabilities {
            local_execution: cfg!(unix),
            cancellation: cfg!(unix),
            timeout: cfg!(unix),
            bounded_output_capture: cfg!(unix),
            process_group_termination: cfg!(unix),
            strong_process_isolation: false,
        }
    }

    fn start(
        &self,
        project: &ProjectRef,
        envelope: CommandExecutionEnvelope,
        options: ProcessExecutionOptions,
    ) -> Result<RunningProcess> {
        #[cfg(not(unix))]
        {
            let _ = (project, envelope, options);
            return Err(process_error("process_runtime_unsupported_platform"));
        }

        #[cfg(unix)]
        {
            let options = options.validate()?;
            let authorized = envelope.into_authorized_command();
            let mut command = self.prepare_command(project, &authorized)?;
            let (process_id, cancel_requested) = self.reserve_process(project.id)?;

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(_) => {
                    self.release_process(process_id);
                    return Err(process_error("process_spawn_failed"));
                }
            };
            let process_started = Instant::now();

            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    terminate_process_group_immediately(&mut child);
                    self.release_process(process_id);
                    return Err(process_error("process_stdout_pipe_missing"));
                }
            };
            let stderr = match child.stderr.take() {
                Some(stderr) => stderr,
                None => {
                    terminate_process_group_immediately(&mut child);
                    self.release_process(process_id);
                    return Err(process_error("process_stderr_pipe_missing"));
                }
            };

            if set_nonblocking(stdout.as_raw_fd()).is_err()
                || set_nonblocking(stderr.as_raw_fd()).is_err()
            {
                terminate_process_group_immediately(&mut child);
                self.release_process(process_id);
                return Err(process_error("process_pipe_nonblocking_failed"));
            }

            let (event_tx, event_rx) = sync_channel(EVENT_QUEUE_CAPACITY);
            let (completion_tx, completion_rx) = oneshot::channel();
            let _ = event_tx.try_send(ProcessEvent::Started { process_id });

            let registry = Arc::clone(&self.registry);
            let child_pid = child.id();
            let payload = Arc::new(Mutex::new(Some(SupervisorPayload {
                child,
                stdout,
                stderr,
            })));
            let worker_payload = Arc::clone(&payload);
            let thread_name = format!(
                "vibecoder-process-{}",
                &process_id.as_uuid().simple().to_string()[..8],
            );
            let spawn_result = thread::Builder::new().name(thread_name).spawn(move || {
                let payload = worker_payload.lock().ok().and_then(|mut slot| slot.take());
                let result = match payload {
                    Some(payload) => supervise_process(
                        process_id,
                        child_pid,
                        payload.child,
                        payload.stdout,
                        payload.stderr,
                        process_started,
                        options,
                        cancel_requested,
                        &event_tx,
                    ),
                    None => Err(process_error("process_supervisor_payload_missing")),
                };
                if let Ok(mut registry) = registry.lock() {
                    registry.active.remove(&process_id);
                }
                let _ = completion_tx.send(result);
            });

            if spawn_result.is_err() {
                // The thread never ran, so the original Arc still owns the complete child payload.
                if let Ok(mut slot) = payload.lock() {
                    if let Some(mut payload) = slot.take() {
                        terminate_process_group_immediately(&mut payload.child);
                    }
                }
                self.release_process(process_id);
                return Err(process_error("process_supervisor_start_failed"));
            }

            Ok(RunningProcess::from_channels(
                process_id,
                event_rx,
                completion_rx,
            ))
        }
    }

    fn active_for_project(&self, project_id: ProjectId) -> Result<usize> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| process_error("process_registry_poisoned"))?;
        Ok(registry
            .active
            .values()
            .filter(|active| active.project_id == project_id)
            .count())
    }

    fn cancel(&self, process_id: ProcessId) -> Result<()> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| process_error("process_registry_poisoned"))?;
        let active = registry
            .active
            .get(&process_id)
            .ok_or_else(|| process_error("process_not_active"))?;
        active.cancel_requested.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(unix)]
#[allow(
    clippy::too_many_arguments,
    reason = "the supervisor receives one ownership-bearing value per process resource"
)]
fn supervise_process(
    process_id: ProcessId,
    child_pid: u32,
    mut child: Child,
    mut stdout: ChildStdout,
    mut stderr: ChildStderr,
    started: Instant,
    options: ProcessExecutionOptions,
    cancel_requested: Arc<AtomicBool>,
    event_tx: &SyncSender<ProcessEvent>,
) -> Result<ProcessResult> {
    let deadline = started + options.timeout();
    let mut stdout_capture = Vec::new();
    let mut stderr_capture = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let mut event_queue_overflowed = false;
    let mut requested_termination: Option<(ProcessTermination, Instant)> = None;
    let mut kill_escalated = false;
    let mut exit_status: Option<ExitStatus> = None;
    let mut exit_seen_at: Option<Instant> = None;

    loop {
        if let Err(error) = drain_pipe(
            &mut stdout,
            &mut stdout_closed,
            &mut stdout_capture,
            options.max_stdout_bytes,
            &mut stdout_truncated,
            event_tx,
            true,
            &mut event_queue_overflowed,
        ) {
            terminate_process_group_immediately(&mut child);
            return Err(error);
        }
        if let Err(error) = drain_pipe(
            &mut stderr,
            &mut stderr_closed,
            &mut stderr_capture,
            options.max_stderr_bytes,
            &mut stderr_truncated,
            event_tx,
            false,
            &mut event_queue_overflowed,
        ) {
            terminate_process_group_immediately(&mut child);
            return Err(error);
        }

        let now = Instant::now();

        // Observe a natural exit before considering a concurrent cancel/timeout request. A process
        // that already completed must not be retroactively classified as cancelled.
        if exit_status.is_none() && (requested_termination.is_none() || kill_escalated) {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    exit_seen_at = Some(now);
                }
                Ok(None) => {}
                Err(_) => {
                    terminate_process_group_immediately(&mut child);
                    return Err(process_error("process_wait_failed"));
                }
            }
        }

        if exit_status.is_none() && requested_termination.is_none() {
            let requested = if cancel_requested.load(Ordering::Acquire) {
                Some(ProcessTermination::Cancelled)
            } else if now >= deadline {
                Some(ProcessTermination::TimedOut)
            } else {
                None
            };
            if let Some(kind) = requested {
                if !signal_process_group(child_pid, libc::SIGTERM) {
                    let _ = child.kill();
                }
                requested_termination = Some((kind, now));
            }
        }

        if let Some((_, requested_at)) = requested_termination {
            if !kill_escalated
                && now.duration_since(requested_at) >= Duration::from_millis(PROCESS_TERMINATION_GRACE_MS)
            {
                // Do not reap the process-group leader during the TERM grace window. Keeping its PID
                // allocated prevents group-id reuse before this escalation and lets SIGKILL cover
                // descendants that ignored TERM even if they closed inherited output pipes.
                if !signal_process_group(child_pid, libc::SIGKILL) {
                    let _ = child.kill();
                }
                kill_escalated = true;
            }
        }

        if exit_status.is_some() {
            if stdout_closed && stderr_closed {
                break;
            }
            if exit_seen_at.is_some_and(|seen| {
                now.duration_since(seen) >= Duration::from_millis(EXIT_PIPE_DRAIN_GRACE_MS)
            }) {
                break;
            }
        }

        thread::sleep(Duration::from_millis(SUPERVISOR_POLL_MS));
    }

    // One final nonblocking drain catches bytes produced immediately before process exit.
    if let Err(error) = drain_pipe(
        &mut stdout,
        &mut stdout_closed,
        &mut stdout_capture,
        options.max_stdout_bytes,
        &mut stdout_truncated,
        event_tx,
        true,
        &mut event_queue_overflowed,
    ) {
        terminate_process_group_immediately(&mut child);
        return Err(error);
    }
    if let Err(error) = drain_pipe(
        &mut stderr,
        &mut stderr_closed,
        &mut stderr_capture,
        options.max_stderr_bytes,
        &mut stderr_truncated,
        event_tx,
        false,
        &mut event_queue_overflowed,
    ) {
        terminate_process_group_immediately(&mut child);
        return Err(error);
    }

    let status = exit_status.ok_or_else(|| process_error("process_exit_status_missing"))?;
    let termination = requested_termination
        .map(|(kind, _)| kind)
        .unwrap_or_else(|| classify_exit(&status));
    let exit_code = status.code();
    if event_queue_overflowed {
        let _ = event_tx.try_send(ProcessEvent::EventQueueOverflow);
    }
    emit_event(
        event_tx,
        ProcessEvent::Finished {
            process_id,
            termination,
            exit_code,
        },
        &mut event_queue_overflowed,
    );

    Ok(ProcessResult {
        process_id,
        termination,
        exit_code,
        stdout: stdout_capture,
        stderr: stderr_capture,
        stdout_truncated,
        stderr_truncated,
        event_queue_overflowed,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

#[cfg(unix)]
#[allow(
    clippy::too_many_arguments,
    reason = "pipe capture and bounded event state are deliberately explicit mutable outputs"
)]
fn drain_pipe<R: Read>(
    reader: &mut R,
    closed: &mut bool,
    capture: &mut Vec<u8>,
    capture_limit: usize,
    capture_truncated: &mut bool,
    event_tx: &SyncSender<ProcessEvent>,
    stdout: bool,
    event_queue_overflowed: &mut bool,
) -> Result<()> {
    if *closed {
        return Ok(());
    }
    let mut buffer = [0u8; OUTPUT_CHUNK_BYTES];
    for _ in 0..MAX_PIPE_CHUNKS_PER_POLL {
        match reader.read(&mut buffer) {
            Ok(0) => {
                *closed = true;
                return Ok(());
            }
            Ok(count) => {
                let chunk = &buffer[..count];
                append_bounded(capture, chunk, capture_limit, capture_truncated);
                let event = if stdout {
                    ProcessEvent::Stdout {
                        bytes: chunk.to_vec(),
                    }
                } else {
                    ProcessEvent::Stderr {
                        bytes: chunk.to_vec(),
                    }
                };
                emit_event(event_tx, event, event_queue_overflowed);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(process_error("process_output_read_failed")),
        }
    }
    Ok(())
}

fn append_bounded(destination: &mut Vec<u8>, chunk: &[u8], limit: usize, truncated: &mut bool) {
    let remaining = limit.saturating_sub(destination.len());
    let keep = remaining.min(chunk.len());
    destination.extend_from_slice(&chunk[..keep]);
    if keep < chunk.len() {
        *truncated = true;
    }
}

fn emit_event(sender: &SyncSender<ProcessEvent>, event: ProcessEvent, overflowed: &mut bool) {
    match sender.try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => *overflowed = true,
        Err(TrySendError::Disconnected(_)) => {}
    }
}

#[cfg(unix)]
fn classify_exit(status: &ExitStatus) -> ProcessTermination {
    if status.signal().is_some() {
        ProcessTermination::Signaled
    } else {
        ProcessTermination::Exited
    }
}

#[cfg(unix)]
fn set_nonblocking(fd: std::os::unix::io::RawFd) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn signal_process_group(child_pid: u32, signal: libc::c_int) -> bool {
    if child_pid == 0 || child_pid > i32::MAX as u32 {
        return false;
    }
    let group = -(child_pid as i32);
    unsafe { libc::kill(group, signal) == 0 }
}

#[cfg(unix)]
fn terminate_process_group_immediately(child: &mut Child) {
    let _ = signal_process_group(child.id(), libc::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn ensure_execution_root_separate_from_writable_home(
    app_private_root: &Path,
    packaged_executable_root: &Path,
) -> Result<()> {
    if packaged_executable_root == app_private_root
        || packaged_executable_root.starts_with(app_private_root)
        || app_private_root.starts_with(packaged_executable_root)
    {
        return Err(process_error("process_executable_root_overlaps_writable_home"));
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn verify_android_packaged_code_directory(path: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|_| process_error("process_packaged_code_root_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(process_error("process_packaged_code_root_invalid"));
    }
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| process_error("process_packaged_code_root_invalid"))?;
    // access(W_OK) evaluates writability for the current app process rather than naively treating
    // another owner's mode bits as app authority. Package-installed native code must not be
    // writable by the VibeCoder UID.
    if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
        return Err(process_error("process_packaged_code_root_writable"));
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn verify_android_packaged_code_file(path: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|_| process_error("process_packaged_code_file_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(process_error("process_packaged_code_file_invalid"));
    }
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| process_error("process_packaged_code_file_invalid"))?;
    if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
        return Err(process_error("process_packaged_code_file_writable"));
    }
    Ok(())
}

#[cfg(unix)]
fn canonical_existing_directory(path: &Path) -> Result<PathBuf> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| process_error("process_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(process_error("process_directory_invalid"));
    }
    fs::canonicalize(path).map_err(|_| process_error("process_directory_canonicalize_failed"))
}

#[cfg(unix)]
fn ensure_fixed_private_directory(parent: &Path, child: &str) -> Result<PathBuf> {
    let path = parent.join(child);
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(_) => return Err(process_error("process_directory_create_failed")),
    }
    verify_no_symlink_directory(&path)?;
    let canonical = fs::canonicalize(&path)
        .map_err(|_| process_error("process_directory_canonicalize_failed"))?;
    if canonical != path {
        return Err(process_error("process_fixed_directory_alias_rejected"));
    }
    Ok(canonical)
}

#[cfg(unix)]
fn verify_exact_child_directory(parent: &Path, expected: &Path, child: &str) -> Result<()> {
    let candidate = parent.join(child);
    if candidate != expected {
        return Err(process_error("process_fixed_directory_mismatch"));
    }
    verify_no_symlink_directory(expected)
}

#[cfg(unix)]
fn verify_no_symlink_directory(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| process_error("process_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(process_error("process_directory_invalid"));
    }
    Ok(())
}

fn normalize_relative_path(path: &Path, allow_root: bool) -> Result<PathBuf> {
    if path.is_absolute() {
        return Err(process_error("process_path_absolute_forbidden"));
    }
    let text = path
        .to_str()
        .ok_or_else(|| process_error("process_path_non_utf8"))?;
    if text.len() > MAX_RELATIVE_PATH_BYTES
        || text.contains('\\')
        || text.chars().any(is_forbidden_display_char)
    {
        return Err(process_error("process_path_invalid"));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| process_error("process_path_non_utf8"))?;
                if value.is_empty()
                    || value.len() > MAX_PATH_COMPONENT_BYTES
                    || value.starts_with(INTERNAL_TEMP_PREFIX)
                {
                    return Err(process_error("process_path_component_invalid"));
                }
                normalized.push(value);
            }
            Component::ParentDir => return Err(process_error("process_path_parent_forbidden")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(process_error("process_path_absolute_forbidden"));
            }
        }
    }
    if normalized.as_os_str().is_empty() && !allow_root {
        return Err(process_error("process_path_empty"));
    }
    Ok(normalized)
}

#[cfg(unix)]
fn resolve_executable_beneath(
    root: &Path,
    relative: &Path,
    invalid_code: &'static str,
) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative, false)?;
    let mut candidate = root.to_path_buf();
    let components: Vec<_> = normalized.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(value) = component else {
            return Err(process_error(invalid_code));
        };
        candidate.push(value);
        if index + 1 < components.len() {
            verify_no_symlink_directory(&candidate)?;
        }
    }
    verify_executable_file(&candidate)?;
    Ok(candidate)
}

#[cfg(unix)]
fn verify_executable_file(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| process_error("process_executable_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(process_error("process_executable_not_regular"));
    }
    if metadata.nlink() != 1 {
        return Err(process_error("process_executable_hardlink_rejected"));
    }
    if metadata.mode() & 0o100 == 0 {
        return Err(process_error("process_executable_owner_execute_required"));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(process_error("process_executable_insecure_mode"));
    }
    Ok(())
}

fn validate_runtime_tool_id(tool_id: String) -> Result<String> {
    if tool_id.is_empty()
        || tool_id.len() > MAX_RUNTIME_TOOL_ID_BYTES
        || !tool_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(process_error("process_runtime_tool_id_invalid"));
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
        return Err(process_error("process_runtime_shell_tool_forbidden"));
    }
    Ok(tool_id)
}

fn validate_runtime_fixed_arg(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_RUNTIME_FIXED_ARG_BYTES
        || value.chars().any(is_forbidden_display_char)
    {
        return Err(process_error("process_runtime_fixed_arg_invalid"));
    }
    Ok(())
}

fn is_forbidden_display_char(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn process_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Process(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibecoder_command_policy::CommandPolicyConfig;

    #[test]
    fn runtime_shells_cannot_enter_process_registry() {
        assert!(RuntimeToolSpec::new("sh", "bin/sh").is_err());
        assert!(RuntimeToolSpec::new("bash", "bin/bash").is_err());
        let _ = CommandPolicyConfig::deny_all();
    }

    #[test]
    fn trusted_interpreter_prefix_is_bounded_and_not_shell_parsed() {
        let npm = RuntimeToolSpec::with_fixed_args(
            "npm",
            "libvibecoder_node_exec.so",
            ["/data/user/0/example/files/vibecoder/runtime/assets/node/npm/bin/npm-cli.js"],
        )
        .expect("npm launch spec");
        assert_eq!(npm.tool_id(), "npm");
        assert_eq!(npm.fixed_args().len(), 1);
        assert!(RuntimeToolSpec::with_fixed_args(
            "npm",
            "libvibecoder_node_exec.so",
            ["bad\narg"],
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn writable_app_home_cannot_also_be_packaged_executable_root() {
        let app = Path::new("/tmp/vibecoder-app-home");
        assert!(ensure_execution_root_separate_from_writable_home(app, app).is_err());
        assert!(
            ensure_execution_root_separate_from_writable_home(
                app,
                Path::new("/tmp/vibecoder-app-home/runtime"),
            )
            .is_err()
        );
        assert!(
            ensure_execution_root_separate_from_writable_home(
                app,
                Path::new("/tmp/vibecoder-package-code"),
            )
            .is_ok()
        );
    }

    #[test]
    fn relative_paths_reject_parent_and_internal_temp_namespace() {
        assert!(normalize_relative_path(Path::new("../escape"), false).is_err());
        assert!(normalize_relative_path(Path::new("src/.vibecoder-tmp-x"), false).is_err());
    }

    #[test]
    fn process_options_reject_unbounded_capture_and_timeout() {
        assert!(
            ProcessExecutionOptions {
                timeout_ms: 0,
                ..ProcessExecutionOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ProcessExecutionOptions {
                max_stdout_bytes: usize::MAX,
                ..ProcessExecutionOptions::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn append_bounded_never_exceeds_limit() {
        let mut output = Vec::new();
        let mut truncated = false;
        append_bounded(&mut output, b"abcdef", 3, &mut truncated);
        assert_eq!(output, b"abc");
        assert!(truncated);
    }
}
