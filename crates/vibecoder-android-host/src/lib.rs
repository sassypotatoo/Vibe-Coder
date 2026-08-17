//! Thin Android-local host boundary for VibeCoder Part 27.
//!
//! This crate is intentionally UI-free. It is the first artifact that is configured to produce a
//! `cdylib` for APK native-library packaging, owns the app-private/package-code root split, wires
//! package-installed runtimes into the process/Jcode adapters, and exposes fail-closed ARM64 probe
//! collection. A desktop build of this crate is useful for source tests, but it never counts as
//! Android execution attestation.

mod app_controller_ffi;
mod gateway_transport;
mod inference;
mod omniroute_ffi;
mod omniroute_service;
pub use omniroute_service::{
    OmniRouteRuntimeVerification, OmniRouteServiceHandle, OmniRouteServiceReadiness,
};

use serde::Serialize;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::future::Future;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use vibecoder_agent_jcode::{
    JcodeConnectionConfig, JcodeConnectionMode, JcodeModelGatewayBridge,
};
#[cfg(target_os = "android")]
use vibecoder_agent_jcode::{JcodeAgentRuntime, JcodeConnectionState};
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_process_contract::{ProcessExecutionOptions, ProcessId, ProcessRuntime, RunningProcess};
use vibecoder_process_local::{LocalProcessRuntime, RuntimeToolSpec};
use vibecoder_runtime_packaging::{
    AndroidArm64RuntimeInventory, AndroidRuntimeReadinessReport, RuntimeArtifactKind,
    ProbeState, RuntimeComponentEvidence, RuntimeComponentSpec, RuntimePlacement,
    evaluate_android_arm64_readiness, probe_android_native_artifact,
    probe_android_native_executable,
};

pub const ANDROID_HOST_ABI_VERSION: u32 = 1;

const ANDROID_HOST_FFI_SCHEMA: u32 = 1;
const ANDROID_HOST_FFI_MAX_JSON_BYTES: usize = 1024 * 1024;
const ANDROID_HOST_FFI_ERROR: i64 = -1;
const ANDROID_HOST_FFI_BUFFER_TOO_SMALL: i64 = -2;

#[derive(Debug, Serialize)]
struct AndroidHostProbeSnapshot {
    schema: u32,
    native_loaded: bool,
    probe_ok: bool,
    abi_version: u32,
    target: &'static str,
    native_evidence: Vec<RuntimeComponentEvidence>,
    additional_evidence: Vec<RuntimeComponentEvidence>,
    readiness: AndroidRuntimeReadinessReport,
}

/// C ABI used by the Part-28 JNI diagnostic shell.
///
/// Call once with `output = null` and `output_capacity = 0` to obtain the required UTF-8 JSON byte
/// length, then call again with exactly that much writable space. Errors are intentionally
/// collapsed to a stable negative status so configuration paths or provider data are never leaked
/// through this bootstrap boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_probe_snapshot_json(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    unsafe {
        ffi_probe_snapshot_entry(
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
            inventory_json,
            inventory_len,
            std::ptr::null(),
            0,
            output,
            output_capacity,
        )
    }
}

/// Extended diagnostic ABI used by the Android shell once APK-asset presence has been inspected by
/// `AssetManager`. Keeping the original symbol above preserves the Part-28 ABI for older shells.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_probe_snapshot_json_v2(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    additional_evidence_json: *const u8,
    additional_evidence_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    unsafe {
        ffi_probe_snapshot_entry(
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
            inventory_json,
            inventory_len,
            additional_evidence_json,
            additional_evidence_len,
            output,
            output_capacity,
        )
    }
}

unsafe fn ffi_probe_snapshot_entry(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    additional_evidence_json: *const u8,
    additional_evidence_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    let result = std::panic::catch_unwind(|| unsafe {
        ffi_probe_snapshot_bytes(
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
            inventory_json,
            inventory_len,
            additional_evidence_json,
            additional_evidence_len,
        )
    });
    let Ok(Ok(bytes)) = result else {
        return ANDROID_HOST_FFI_ERROR;
    };
    let Ok(required) = i64::try_from(bytes.len()) else {
        return ANDROID_HOST_FFI_ERROR;
    };
    if output.is_null() {
        return if output_capacity == 0 {
            required
        } else {
            ANDROID_HOST_FFI_ERROR
        };
    }
    if output_capacity < bytes.len() {
        return ANDROID_HOST_FFI_BUFFER_TOO_SMALL;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    }
    required
}

unsafe fn ffi_probe_snapshot_bytes(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    additional_evidence_json: *const u8,
    additional_evidence_len: usize,
) -> Result<Vec<u8>> {
    if app_private_dir.is_null()
        || native_library_dir.is_null()
        || packaged_executable_dir.is_null()
        || inventory_json.is_null()
        || inventory_len == 0
        || inventory_len > ANDROID_HOST_FFI_MAX_JSON_BYTES
        || additional_evidence_len > ANDROID_HOST_FFI_MAX_JSON_BYTES
        || (additional_evidence_len > 0 && additional_evidence_json.is_null())
    {
        return Err(host_error("android_host_ffi_arguments_invalid"));
    }
    let app_private = unsafe { ffi_path(app_private_dir)? };
    let native_library = unsafe { ffi_path(native_library_dir)? };
    let packaged_executable = unsafe { ffi_path(packaged_executable_dir)? };
    let inventory = unsafe { std::slice::from_raw_parts(inventory_json, inventory_len) };
    let additional_evidence = if additional_evidence_len == 0 {
        Vec::new()
    } else {
        let bytes = unsafe {
            std::slice::from_raw_parts(additional_evidence_json, additional_evidence_len)
        };
        serde_json::from_slice::<Vec<RuntimeComponentEvidence>>(bytes)
            .map_err(|_| host_error("android_host_ffi_additional_evidence_invalid"))?
    };
    let host = AndroidHostRuntime::from_inventory_json(
        AndroidHostPaths::new(app_private, native_library, packaged_executable)?,
        inventory,
    )?;

    let mut native_evidence = host.collect_packaged_native_evidence()?;
    if let Some(index) = native_evidence
        .iter()
        .position(|item| item.component_id == JCODE_COMPONENT_ID)
    {
        let native_ready = matches!(native_evidence[index].package_presence, ProbeState::Passed)
            && matches!(native_evidence[index].arm64_identity, ProbeState::Passed)
            && matches!(native_evidence[index].execution, ProbeState::Passed)
            && matches!(native_evidence[index].version, ProbeState::Passed)
            && matches!(
                native_evidence[index].page_size_16k_compatibility,
                ProbeState::Passed
            );
        if native_ready {
            let jcode = host.probe_jcode_round_trip(native_evidence[index].clone())?;
            native_evidence[index] = jcode;
        }
    }
    let readiness = host.readiness_from_collected_evidence(
        native_evidence.clone(),
        additional_evidence.clone(),
    )?;
    let snapshot = AndroidHostProbeSnapshot {
        schema: ANDROID_HOST_FFI_SCHEMA,
        native_loaded: true,
        probe_ok: true,
        abi_version: ANDROID_HOST_ABI_VERSION,
        target: "android/arm64-v8a",
        native_evidence,
        additional_evidence,
        readiness,
    };
    serde_json::to_vec(&snapshot).map_err(|_| host_error("android_host_ffi_json_failed"))
}

unsafe fn ffi_path(pointer: *const c_char) -> Result<PathBuf> {
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
        return Err(host_error("android_host_ffi_path_invalid"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| host_error("android_host_ffi_path_non_utf8"))?;
    Ok(PathBuf::from(text))
}

const JCODE_COMPONENT_ID: &str = "jcode";
const NODE_COMPONENT_ID: &str = "node";
const NODE_RUNTIME_SERVICE_ID: &str = "node-runtime";
const JCODE_VERSION_ARGS: &[&str] = &["--version"];
const NODE_VERSION_ARGS: &[&str] = &["--version"];
const NATIVE_VERSION_PROBE_TIMEOUT_MS: u64 = 5_000;

/// Minimal C ABI smoke symbol. A later Android/Kotlin shell can resolve this symbol after loading
/// `libvibecoder_android_host.so`; returning a number does not require JNI string/object handling.
#[unsafe(no_mangle)]
pub extern "C" fn vibecoder_android_host_abi_version() -> u32 {
    ANDROID_HOST_ABI_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidHostPaths {
    app_private_dir: PathBuf,
    native_library_dir: PathBuf,
    packaged_executable_dir: PathBuf,
}

impl AndroidHostPaths {
    /// Construct Android roots supplied by the future Kotlin/Android shell.
    ///
    /// `native_library_dir` is the package-owned base native-library location. Base-delivered
    /// native executables such as Jcode resolve there as well. `packaged_executable_dir` is the
    /// trusted package-owned execution root for Play-feature-delivered executables such as Node.
    /// Before an optional feature is installed the shell may pass the base native directory for
    /// both roots; normal chat startup supplies the installed Node feature root explicitly.
    pub fn new(
        app_private_dir: impl AsRef<Path>,
        native_library_dir: impl AsRef<Path>,
        packaged_executable_dir: impl AsRef<Path>,
    ) -> Result<Self> {
        let app_private_dir = canonical_directory(
            app_private_dir.as_ref().to_path_buf(),
            "android_app_private",
        )?;
        let native_library_dir = canonical_directory(
            native_library_dir.as_ref().to_path_buf(),
            "android_native_lib",
        )?;
        let packaged_executable_dir = canonical_directory(
            packaged_executable_dir.as_ref().to_path_buf(),
            "android_packaged_executable",
        )?;
        ensure_code_root_outside_writable_data(&app_private_dir, &native_library_dir)?;
        ensure_code_root_outside_writable_data(&app_private_dir, &packaged_executable_dir)?;
        Ok(Self {
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
        })
    }

    pub fn app_private_dir(&self) -> &Path {
        &self.app_private_dir
    }

    pub fn native_library_dir(&self) -> &Path {
        &self.native_library_dir
    }

    pub fn packaged_executable_dir(&self) -> &Path {
        &self.packaged_executable_dir
    }
}

/// Explicit synchronous-to-async boundary for the Android/JNI host.
///
/// The UI/JNI entry points are synchronous, while the gateway and agent contracts are async. A
/// single current-thread Tokio runtime keeps that bridge owned by one component instead of relying
/// on ambient `Handle::current()` state. I/O and time drivers are enabled because OmniRoute uses
/// reqwest and future agent integrations may use timers/network I/O.
pub struct AndroidAsyncExecutor {
    runtime: Mutex<tokio::runtime::Runtime>,
}

impl std::fmt::Debug for AndroidAsyncExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AndroidAsyncExecutor")
            .field("scheduler", &"current_thread")
            .finish_non_exhaustive()
    }
}

impl AndroidAsyncExecutor {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| host_error("android_host_async_runtime_init_failed"))?;
        Ok(Self {
            runtime: Mutex::new(runtime),
        })
    }

    /// Drive one VibeCoder async operation to completion. Calls are serialized because a
    /// current-thread runtime must have exactly one active driver at a time.
    pub fn block_on<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(host_error("android_host_async_runtime_nested_block_on_forbidden"));
        }
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| host_error("android_host_async_runtime_poisoned"))?;
        runtime.block_on(future)
    }
}

pub struct AndroidHostRuntime {
    paths: AndroidHostPaths,
    inventory: AndroidArm64RuntimeInventory,
    process_runtime: Arc<LocalProcessRuntime>,
    async_executor: Arc<AndroidAsyncExecutor>,
}

impl std::fmt::Debug for AndroidHostRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AndroidHostRuntime")
            .field("target", &"android/arm64-v8a")
            .field("component_count", &self.inventory.components.len())
            .finish_non_exhaustive()
    }
}

impl AndroidHostRuntime {
    /// Initialize the UI-free Android host from platform-owned roots.
    ///
    /// The Android shell provides its base native-library directory plus the package-owned
    /// executable root used by on-demand runtime modules. No fallback to PATH, filesDir, cacheDir,
    /// or copied writable code is permitted. Base native executables remain anchored to the base
    /// native-library directory; Play-feature executables resolve only from the feature root.
    pub fn initialize(
        paths: AndroidHostPaths,
        inventory: AndroidArm64RuntimeInventory,
    ) -> Result<Self> {
        inventory.validate()?;
        #[cfg(target_os = "android")]
        {
            verify_android_code_directory_nonwritable(paths.native_library_dir())?;
            verify_android_code_directory_nonwritable(paths.packaged_executable_dir())?;
        }
        let runtime_tools = direct_runtime_tools(&inventory)?;
        let process_runtime = Arc::new(LocalProcessRuntime::initialize(
            paths.app_private_dir(),
            paths.packaged_executable_dir(),
            runtime_tools,
        )?);
        let async_executor = Arc::new(AndroidAsyncExecutor::new()?);
        Ok(Self {
            paths,
            inventory,
            process_runtime,
            async_executor,
        })
    }

    pub fn from_inventory_json(paths: AndroidHostPaths, inventory_json: &[u8]) -> Result<Self> {
        Self::initialize(paths, AndroidArm64RuntimeInventory::from_json(inventory_json)?)
    }

    pub fn inventory(&self) -> &AndroidArm64RuntimeInventory {
        &self.inventory
    }

    pub fn process_runtime(&self) -> Arc<dyn ProcessRuntime> {
        let runtime: Arc<dyn ProcessRuntime> = self.process_runtime.clone();
        runtime
    }

    /// Executor for synchronous Android/JNI callers that need async agent/gateway/core APIs.
    pub fn async_executor(&self) -> Arc<AndroidAsyncExecutor> {
        self.async_executor.clone()
    }

    /// Start the package-installed Node runtime under the hardened local process supervisor.
    ///
    /// The executable must resolve from the package-owned runtime registry. Arguments are passed as
    /// argv entries, never through a shell, and the child inherits no ambient environment. The
    /// underlying supervisor provides bounded stdout/stderr capture, timeout, process-group
    /// cancellation, and Android parent-death cleanup. Exactly one Node runtime service may be
    /// active at a time; OmniRoute will build on this primitive in Part 34.3.
    pub fn start_node_runtime(
        &self,
        args: &[String],
        options: ProcessExecutionOptions,
    ) -> Result<RunningProcess> {
        self.native_component_path(NODE_COMPONENT_ID)?;
        self.process_runtime.start_runtime_service(
            NODE_RUNTIME_SERVICE_ID,
            NODE_COMPONENT_ID,
            args,
            options,
        )
    }

    pub fn node_runtime_active(&self) -> Result<bool> {
        Ok(self
            .process_runtime
            .active_runtime_service(NODE_RUNTIME_SERVICE_ID)?
            == 1)
    }

    pub fn cancel_node_runtime(&self, process_id: ProcessId) -> Result<()> {
        self.process_runtime.cancel(process_id)
    }

    pub fn native_component_path(&self, component_id: &str) -> Result<PathBuf> {
        let component = required_component(&self.inventory, component_id)?;
        if !matches!(
            component.artifact_kind,
            RuntimeArtifactKind::InProcessNative
                | RuntimeArtifactKind::NativeExecutable
                | RuntimeArtifactKind::NativeLibrary
        ) || !matches!(
            component.placement,
            RuntimePlacement::ApkNativeLibrary
                | RuntimePlacement::ApkNativeExecutable
                | RuntimePlacement::PlayFeatureNativeExecutable
        ) {
            return Err(host_error("android_host_component_not_native"));
        }
        let (root, candidate) = self.native_component_candidate(component)?;
        verify_direct_child_file(root, &candidate)?;
        #[cfg(target_os = "android")]
        verify_android_code_file_nonwritable(&candidate)?;
        Ok(candidate)
    }

    fn native_component_candidate(
        &self,
        component: &RuntimeComponentSpec,
    ) -> Result<(&Path, PathBuf)> {
        let relative = component
            .relative_path
            .as_ref()
            .ok_or_else(|| host_error("android_host_native_component_path_missing"))?;
        if relative.components().count() != 1 {
            return Err(host_error("android_host_native_component_not_direct_child"));
        }
        let root = match component.artifact_kind {
            RuntimeArtifactKind::InProcessNative | RuntimeArtifactKind::NativeLibrary => {
                self.paths.native_library_dir()
            }
            RuntimeArtifactKind::NativeExecutable => match component.placement {
                RuntimePlacement::ApkNativeExecutable => self.paths.native_library_dir(),
                RuntimePlacement::PlayFeatureNativeExecutable => self.paths.packaged_executable_dir(),
                _ => return Err(host_error("android_host_native_executable_placement_invalid")),
            },
            _ => return Err(host_error("android_host_component_not_native")),
        };
        Ok((root, root.join(relative)))
    }

    /// Build private Jcode configuration with an explicit package-installed binary path.
    ///
    /// This removes the desktop fallback where the SDK could resolve `jcode` through ambient PATH.
    pub fn jcode_connection_config(&self) -> Result<JcodeConnectionConfig> {
        let binary = self.native_component_path(JCODE_COMPONENT_ID)?;
        let jcode_home = self.process_runtime.runtime_root().join("jcode-home");
        Ok(JcodeConnectionConfig {
            connection: JcodeConnectionMode::Private {
                jcode_home: Some(jcode_home),
                binary: Some(binary),
                inherit_logins: false,
                startup_timeout_ms: 30_000,
                cleanup_timeout_ms: 30_000,
            },
            model_gateway_bridge: Some(
                JcodeModelGatewayBridge::VibeCoderOmniRouteLoopbackV1,
            ),
            ..JcodeConnectionConfig::default()
        })
    }

    /// Inspect every package-native inventory row and, on an actual Android process, execute the
    /// pinned Jcode/Node version probes. Unknown/unpinned Android-build tools remain `NotRun` for
    /// execution/version rather than receiving optimistic evidence.
    pub fn collect_packaged_native_evidence(&self) -> Result<Vec<RuntimeComponentEvidence>> {
        let mut evidence = Vec::new();
        for component in &self.inventory.components {
            if !matches!(
                component.artifact_kind,
                RuntimeArtifactKind::InProcessNative
                    | RuntimeArtifactKind::NativeExecutable
                    | RuntimeArtifactKind::NativeLibrary
            ) {
                continue;
            }
            let (_, path) = self.native_component_candidate(component)?;
            #[cfg(target_os = "android")]
            match fs::symlink_metadata(&path) {
                Ok(_) => verify_android_code_file_nonwritable(&path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(host_error("android_host_code_file_metadata_failed")),
            }
            let probe = match component.component_id.as_str() {
                JCODE_COMPONENT_ID => probe_android_native_executable(
                    &path,
                    JCODE_VERSION_ARGS,
                    &component.version_requirement,
                    NATIVE_VERSION_PROBE_TIMEOUT_MS,
                ),
                NODE_COMPONENT_ID => probe_android_native_executable(
                    &path,
                    NODE_VERSION_ARGS,
                    &component.version_requirement,
                    NATIVE_VERSION_PROBE_TIMEOUT_MS,
                ),
                _ => probe_android_native_artifact(&path),
            };
            let expected = if component.requires_version_probe {
                Some(component.version_requirement.as_str())
            } else {
                None
            };
            evidence.push(probe.into_component_evidence(component.component_id.clone(), expected));
        }
        Ok(evidence)
    }

    /// Perform only the stronger Jcode API-socket handshake using already-collected native
    /// evidence. The caller must pass the Jcode evidence produced by
    /// `collect_packaged_native_evidence`; this deliberately avoids spawning `jcode --version`
    /// twice during one diagnostic snapshot.
    pub fn probe_jcode_round_trip(
        &self,
        mut evidence: RuntimeComponentEvidence,
    ) -> Result<RuntimeComponentEvidence> {
        required_component(&self.inventory, JCODE_COMPONENT_ID)?;
        if evidence.component_id != JCODE_COMPONENT_ID {
            return Err(host_error("android_host_jcode_probe_evidence_mismatch"));
        }
        if evidence.execution != ProbeState::Passed || evidence.version != ProbeState::Passed {
            return Ok(evidence);
        }

        #[cfg(not(target_os = "android"))]
        {
            return Ok(evidence);
        }

        #[cfg(target_os = "android")]
        {
            let runtime = match JcodeAgentRuntime::new(self.jcode_connection_config()?) {
                Ok(runtime) => runtime,
                Err(_) => {
                    evidence.unix_socket_round_trip = ProbeState::Failed;
                    return Ok(evidence);
                }
            };
            match runtime.connect() {
                Ok(snapshot) => {
                    evidence.unix_socket_round_trip = if matches!(
                        snapshot.state,
                        JcodeConnectionState::Connected { .. }
                    ) {
                        ProbeState::Passed
                    } else {
                        ProbeState::Failed
                    };
                    let _ = runtime.disconnect();
                }
                Err(_) => evidence.unix_socket_round_trip = ProbeState::Failed,
            }
            Ok(evidence)
        }
    }

    /// Merge caller-supplied APK-asset/service attestations with freshly collected native probes.
    pub fn readiness_with_additional_evidence(
        &self,
        additional: impl IntoIterator<Item = RuntimeComponentEvidence>,
    ) -> Result<AndroidRuntimeReadinessReport> {
        self.readiness_from_collected_evidence(
            self.collect_packaged_native_evidence()?,
            additional,
        )
    }

    /// Apply readiness to one already-collected native evidence set plus caller-supplied APK asset
    /// or service evidence. This is used by the JNI snapshot so native version probes are not run a
    /// second time just to merge Java-side asset observations.
    pub fn readiness_from_collected_evidence(
        &self,
        native: impl IntoIterator<Item = RuntimeComponentEvidence>,
        additional: impl IntoIterator<Item = RuntimeComponentEvidence>,
    ) -> Result<AndroidRuntimeReadinessReport> {
        let mut merged: HashMap<String, RuntimeComponentEvidence> = native
            .into_iter()
            .map(|item| (item.component_id.clone(), item))
            .collect();
        for item in additional {
            if merged.insert(item.component_id.clone(), item).is_some() {
                return Err(host_error("android_host_duplicate_probe_evidence"));
            }
        }
        evaluate_android_arm64_readiness(
            &self.inventory,
            &merged.into_values().collect::<Vec<_>>(),
        )
    }
}

fn direct_runtime_tools(inventory: &AndroidArm64RuntimeInventory) -> Result<Vec<RuntimeToolSpec>> {
    let mut tools = Vec::new();
    // Node is directly executable and is needed by the gateway plus future interpreted package
    // manager wrappers. npm itself is deliberately NOT registered here until its package asset and
    // entrypoint are pinned/materialized/verified; registering a writable npm script as executable
    // would reintroduce the Android W^X bug fixed in Part 26.
    let node = required_component(inventory, NODE_COMPONENT_ID)?;
    let node_path = native_executable_relative_path(node)?;
    tools.push(RuntimeToolSpec::new("node", node_path)?);
    Ok(tools)
}

/// Build a trusted interpreted-tool launch spec after a caller has verified a package asset and
/// materialized it as non-executable data. This is the safe shape for npm/Gradle-JAR-style tools.
pub fn interpreted_runtime_tool(
    tool_id: &str,
    interpreter_native_relative_path: &Path,
    verified_script_or_archive: &Path,
    fixed_prefix_args: &[&str],
) -> Result<RuntimeToolSpec> {
    if !verified_script_or_archive.is_absolute() {
        return Err(host_error("android_host_interpreted_entrypoint_not_absolute"));
    }
    let entry = verified_script_or_archive
        .to_str()
        .ok_or_else(|| host_error("android_host_interpreted_entrypoint_non_utf8"))?;
    let mut args = fixed_prefix_args
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    args.push(entry.to_owned());
    RuntimeToolSpec::with_fixed_args(tool_id, interpreter_native_relative_path, args)
}

fn required_component<'a>(
    inventory: &'a AndroidArm64RuntimeInventory,
    component_id: &str,
) -> Result<&'a RuntimeComponentSpec> {
    inventory
        .components
        .iter()
        .find(|component| component.component_id == component_id)
        .ok_or_else(|| host_error("android_host_required_component_missing"))
}

fn native_executable_relative_path(component: &RuntimeComponentSpec) -> Result<PathBuf> {
    if component.artifact_kind != RuntimeArtifactKind::NativeExecutable
        || !matches!(component.placement, RuntimePlacement::ApkNativeExecutable | RuntimePlacement::PlayFeatureNativeExecutable)
    {
        return Err(host_error("android_host_runtime_tool_not_native_executable"));
    }
    component
        .relative_path
        .clone()
        .ok_or_else(|| host_error("android_host_runtime_tool_path_missing"))
}

fn canonical_directory(path: PathBuf, label: &'static str) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(host_error(match label {
            "android_app_private" => "android_host_app_private_not_absolute",
            "android_native_lib" => "android_host_native_library_not_absolute",
            _ => "android_host_packaged_executable_not_absolute",
        }));
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| host_error("android_host_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(host_error("android_host_directory_invalid"));
    }
    fs::canonicalize(&path).map_err(|_| host_error("android_host_directory_canonicalize_failed"))
}

#[cfg(target_os = "android")]
fn verify_android_code_directory_nonwritable(path: &Path) -> Result<()> {
    // This is a fail-closed app-UID writability sanity check, not a cryptographic integrity or
    // complete SELinux attestation. Android package ownership/signing remains the stronger code
    // provenance boundary; if the app can directly write this path we reject it immediately.
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| host_error("android_host_code_root_invalid"))?;
    if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
        return Err(host_error("android_host_code_root_writable"));
    }
    Ok(())
}


#[cfg(target_os = "android")]
fn verify_android_code_file_nonwritable(path: &Path) -> Result<()> {
    // Same scope as the directory check above: reject files writable by the app identity, while
    // package installation/signing supplies the stronger provenance guarantee.
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|_| host_error("android_host_code_file_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(host_error("android_host_code_file_invalid"));
    }
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| host_error("android_host_code_file_invalid"))?;
    if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
        return Err(host_error("android_host_code_file_writable"));
    }
    Ok(())
}

fn ensure_code_root_outside_writable_data(app_private: &Path, code_root: &Path) -> Result<()> {
    if app_private == code_root
        || app_private.starts_with(code_root)
        || code_root.starts_with(app_private)
    {
        return Err(host_error("android_host_code_data_roots_overlap"));
    }
    Ok(())
}

fn verify_direct_child_file(root: &Path, candidate: &Path) -> Result<()> {
    if candidate.parent() != Some(root) {
        return Err(host_error("android_host_native_component_not_direct_child"));
    }
    let metadata = fs::symlink_metadata(candidate)
        .map_err(|_| host_error("android_host_native_component_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(host_error("android_host_native_component_invalid"));
    }
    Ok(())
}

fn host_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Config(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cdylib_abi_version_is_stable() {
        assert_eq!(vibecoder_android_host_abi_version(), ANDROID_HOST_ABI_VERSION);
    }

    #[cfg(unix)]
    fn temp_host_roots(label: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "vibecoder-part27-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let app = root.join("app");
        let native = root.join("native");
        let exec = root.join("exec");
        fs::create_dir_all(&app).expect("app root");
        fs::create_dir_all(&native).expect("native root");
        fs::create_dir_all(&exec).expect("exec root");
        (root, app, native, exec)
    }

    #[cfg(unix)]
    #[test]
    fn host_paths_keep_writable_data_outside_both_code_roots() {
        let (root, app, native, exec) = temp_host_roots("roots");
        AndroidHostPaths::new(&app, &native, &exec).expect("separate host roots");
        assert!(AndroidHostPaths::new(&app, &app, &exec).is_err());
        assert!(AndroidHostPaths::new(&app, &native, &app).is_err());
        // The two code concepts may use the same directory only when the package layout really
        // exposes executable files there; Android initialization performs the additional
        // non-writable check.
        AndroidHostPaths::new(&app, &native, &native).expect("shared package code root");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn jcode_private_config_uses_explicit_package_binary_not_path() {
        let (root, app, native, exec) = temp_host_roots("jcode");
        let expected = native.join("libvibecoder_jcode_exec.so");
        fs::write(&expected, b"test-only-placeholder").expect("jcode placeholder");
        let paths = AndroidHostPaths::new(&app, &native, &exec).expect("paths");
        let inventory = AndroidArm64RuntimeInventory::from_json(include_bytes!(
            "../../../config/android-runtime-inventory.json"
        ))
        .expect("inventory");
        let host = AndroidHostRuntime::initialize(paths, inventory).expect("host");
        let config = host.jcode_connection_config().expect("Jcode config");
        assert_eq!(
            config.model_gateway_bridge,
            Some(JcodeModelGatewayBridge::VibeCoderOmniRouteLoopbackV1)
        );
        match config.connection {
            JcodeConnectionMode::Private { binary, .. } => {
                assert_eq!(binary.as_deref(), Some(expected.as_path()));
            }
            JcodeConnectionMode::Shared { .. } => panic!("Android host unexpectedly chose shared Jcode"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn node_play_feature_resolves_from_feature_root_not_base_native_root() {
        let (root, app, native, exec) = temp_host_roots("node-feature-root");
        let expected = exec.join("libvibecoder_node_exec.so");
        fs::write(&expected, b"test-only-placeholder").expect("node placeholder");
        let paths = AndroidHostPaths::new(&app, &native, &exec).expect("paths");
        let inventory = AndroidArm64RuntimeInventory::from_json(include_bytes!(
            "../../../config/android-runtime-inventory.json"
        ))
        .expect("inventory");
        let host = AndroidHostRuntime::initialize(paths, inventory).expect("host");
        assert_eq!(
            host.native_component_path(NODE_COMPONENT_ID).expect("Node feature path"),
            expected
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn android_async_executor_drives_domain_result_future() {
        let executor = AndroidAsyncExecutor::new().expect("async executor");
        let value = executor
            .block_on(async { Ok::<u32, VibeCoderError>(7) })
            .expect("async result");
        assert_eq!(value, 7);
    }

    #[cfg(unix)]
    #[test]
    fn node_runtime_service_is_inactive_after_clean_host_initialization() {
        let (root, app, native, exec) = temp_host_roots("node-runtime");
        let inventory = AndroidArm64RuntimeInventory::from_json(include_bytes!(
            "../../../config/android-runtime-inventory.json"
        ))
        .expect("inventory");
        let host = AndroidHostRuntime::initialize(
            AndroidHostPaths::new(&app, &native, &exec).expect("paths"),
            inventory,
        )
        .expect("host");
        assert!(!host.node_runtime_active().expect("node active state"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interpreted_tool_requires_absolute_verified_data_entrypoint() {
        assert!(interpreted_runtime_tool(
            "npm",
            Path::new("libvibecoder_node_exec.so"),
            Path::new("node/npm/bin/npm-cli.js"),
            &[],
        )
        .is_err());
        let spec = interpreted_runtime_tool(
            "npm",
            Path::new("libvibecoder_node_exec.so"),
            Path::new("/data/user/0/example/files/vibecoder/runtime/assets/node/npm/bin/npm-cli.js"),
            &[],
        )
        .expect("interpreted npm spec");
        assert_eq!(spec.tool_id(), "npm");
        assert_eq!(spec.fixed_args().len(), 1);
    }
}
