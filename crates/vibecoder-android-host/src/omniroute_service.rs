use super::{AndroidHostRuntime, NODE_COMPONENT_ID, NODE_RUNTIME_SERVICE_ID, host_error};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_gateway_contract::{GatewayCredential, ModelGateway};
use vibecoder_gateway_omniroute::{OmniRouteClient, OmniRouteConfig};
use vibecoder_process_contract::{
    MAX_EVENT_DRAIN, ProcessEvent, ProcessId, ProcessResult, ProcessRuntime, ProcessTermination,
    RunningProcess,
};

const RUNTIME_PROFILE_JSON: &str = include_str!("../../../config/omniroute-android-runtime-profile.json");
const RUNTIME_RELATIVE_ROOT: &str = "omniroute";
const MANIFEST_NAME: &str = ".vibecoder-omniroute-bundle.json";
const RECEIPT_NAME: &str = ".vibecoder-omniroute-install.json";
const EXPECTED_COMPONENT: &str = "omniroute";
const EXPECTED_VERSION: &str = "3.8.50";
const EXPECTED_PROFILE_ID: &str = "vibecoder-omniroute-android-backend-v1";
const EXPECTED_NODE_VERSION: &str = "24.19.0";
const EXPECTED_SOURCE_SHA256: &str =
    "1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7";
const EXPECTED_ROUTING_PROFILE_SHA256: &str =
    "aec0f63fb0dec08f24fffde9209504ec447e9428bec1cd64c033649ed275fe3d";
const EXPECTED_GATEWAY_PROFILE_ID: &str = "vibecoder-omniroute-exact-model-v1";
pub(crate) const LOOPBACK_BASE_URL: &str = "http://127.0.0.1:20128/v1";
const READY_TIMEOUT_MS: u64 = 60_000;
const READY_POLL_MS: u64 = 100;
const READY_CONSECUTIVE_ATTESTATIONS: usize = 2;
const READY_HTTP_TIMEOUT_MS: u64 = 750;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = 128 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const SERVICE_STDOUT_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const SERVICE_STDERR_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmniRouteRuntimeVerification {
    pub root: PathBuf,
    pub manifest_sha256: String,
    pub tree_sha256: String,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmniRouteServiceReadiness {
    pub process_id: ProcessId,
    pub base_url: String,
    pub upstream_version: String,
    pub routing_profile_id: String,
    pub routing_profile_sha256: String,
    pub exact_model_only: bool,
    pub hidden_model_reroutes_disabled: bool,
    pub probe_attempts: usize,
    pub elapsed_ms: u64,
}

pub struct OmniRouteServiceHandle {
    process: RunningProcess,
    readiness: OmniRouteServiceReadiness,
    runtime: OmniRouteRuntimeVerification,
}

impl std::fmt::Debug for OmniRouteServiceHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OmniRouteServiceHandle")
            .field("process_id", &self.process.process_id())
            .field("readiness", &self.readiness)
            .field("runtime_tree_sha256", &self.runtime.tree_sha256)
            .finish_non_exhaustive()
    }
}

impl OmniRouteServiceHandle {
    pub const fn process_id(&self) -> ProcessId {
        self.process.process_id()
    }

    pub fn readiness(&self) -> &OmniRouteServiceReadiness {
        &self.readiness
    }

    pub fn runtime_verification(&self) -> &OmniRouteRuntimeVerification {
        &self.runtime
    }

    /// Drain bounded process events. Output bytes remain captured by the process supervisor and are
    /// also returned in the final `ProcessResult`; this method is for live console/status surfaces.
    pub fn drain_events(&self, max_events: usize) -> Result<Vec<ProcessEvent>> {
        self.process.drain_events(max_events)
    }

    fn into_running_process(self) -> RunningProcess {
        self.process
    }
}

#[derive(Debug, Deserialize)]
struct BundleManifest {
    schema: u32,
    component_id: String,
    version: String,
    profile_id: String,
    source_archive_sha256: String,
    routing_patch_profile_sha256: String,
    required_node_version: String,
    runtime: Value,
    file_count: usize,
    total_bytes: u64,
    tree_sha256: String,
    files: Vec<ManifestFile>,
    apk_asset_packaged: bool,
    service_round_trip_proven: bool,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct InstallReceipt {
    schema: u32,
    component_id: String,
    version: String,
    profile_id: String,
    manifest_sha256: String,
    tree_sha256: String,
}

#[derive(Debug, Clone)]
struct LaunchPlan {
    working_directory: PathBuf,
    args: Vec<String>,
    environment: Vec<(String, String)>,
}

impl AndroidHostRuntime {
    /// Verify and start the installed OmniRoute runtime with the package-owned Node executable.
    ///
    /// Readiness is not inferred from process creation or a log substring. The child must remain
    /// active while the exact hash-pinned VibeCoder runtime-profile endpoint succeeds twice on
    /// `127.0.0.1:20128`. A failed readiness probe cancels and reaps the child before returning.
    pub fn start_omniroute_service(&self, expected_manifest_sha256: &str) -> Result<OmniRouteServiceHandle> {
        self.native_component_path(NODE_COMPONENT_ID)?;
        if self.omniroute_service_active()? {
            return Err(host_error("android_host_omniroute_service_already_active"));
        }

        let runtime = self.verify_installed_omniroute_runtime(expected_manifest_sha256)?;
        let service_data = self
            .process_runtime
            .runtime_service_private_directory(NODE_RUNTIME_SERVICE_ID)?;
        let plan = build_launch_plan(&service_data)?;
        let process = self.process_runtime.start_persistent_runtime_service(
            NODE_RUNTIME_SERVICE_ID,
            NODE_COMPONENT_ID,
            &plan.working_directory,
            &plan.args,
            &plan.environment,
            SERVICE_STDOUT_LIMIT_BYTES,
            SERVICE_STDERR_LIMIT_BYTES,
        )?;
        let process_id = process.process_id();

        let readiness_result = self.async_executor.block_on(wait_for_omniroute_readiness(
            self.process_runtime.as_ref(),
            &process,
            READY_TIMEOUT_MS,
        ));
        let readiness = match readiness_result {
            Ok(readiness) => readiness,
            Err(error) => {
                let _ = self.process_runtime.cancel(process_id);
                let _ = self.async_executor.block_on(process.wait());
                return Err(error);
            }
        };

        Ok(OmniRouteServiceHandle {
            process,
            readiness,
            runtime,
        })
    }

    pub fn omniroute_service_active(&self) -> Result<bool> {
        Ok(self
            .process_runtime
            .active_runtime_service(NODE_RUNTIME_SERVICE_ID)?
            == 1)
    }

    /// Explicitly stop and reap one OmniRoute service. No automatic restart is performed.
    pub fn stop_omniroute_service(&self, service: OmniRouteServiceHandle) -> Result<ProcessResult> {
        let process_id = service.process_id();
        if self.omniroute_service_active()? {
            self.process_runtime.cancel(process_id)?;
        }
        let result = self
            .async_executor
            .block_on(service.into_running_process().wait())?;
        if !matches!(
            result.termination,
            ProcessTermination::Cancelled | ProcessTermination::Exited | ProcessTermination::Signaled
        ) {
            return Err(host_error("android_host_omniroute_stop_unexpected_termination"));
        }
        if self.omniroute_service_active()? {
            return Err(host_error("android_host_omniroute_stop_still_active"));
        }
        Ok(result)
    }

    /// Wait for a service that already exited/crashed and return its bounded captured result.
    pub fn wait_omniroute_service(&self, service: OmniRouteServiceHandle) -> Result<ProcessResult> {
        self.async_executor.block_on(service.into_running_process().wait())
    }

    /// Explicit stop -> full re-verification -> fresh launch. Crash recovery never loops by itself.
    pub fn restart_omniroute_service(
        &self,
        service: OmniRouteServiceHandle,
    ) -> Result<(ProcessResult, OmniRouteServiceHandle)> {
        let expected_manifest_sha256 = service.runtime.manifest_sha256.clone();
        let stopped = self.stop_omniroute_service(service)?;
        let restarted = self.start_omniroute_service(&expected_manifest_sha256)?;
        Ok((stopped, restarted))
    }

    pub fn verify_installed_omniroute_runtime(
        &self,
        expected_manifest_sha256: &str,
    ) -> Result<OmniRouteRuntimeVerification> {
        let root = self
            .paths
            .app_private_dir()
            .join("vibecoder")
            .join("runtime")
            .join(RUNTIME_RELATIVE_ROOT);
        verify_runtime_tree(&root, expected_manifest_sha256)
    }
}

fn build_launch_plan(service_data: &Path) -> Result<LaunchPlan> {
    if !service_data.is_absolute() {
        return Err(host_error("android_host_omniroute_data_dir_not_absolute"));
    }
    let profile: Value = serde_json::from_str(RUNTIME_PROFILE_JSON)
        .map_err(|_| host_error("android_host_omniroute_runtime_profile_invalid"))?;
    let runtime = profile
        .get("runtime")
        .and_then(Value::as_object)
        .ok_or_else(|| host_error("android_host_omniroute_runtime_profile_missing"))?;
    if runtime.get("entrypoint").and_then(Value::as_str) != Some("server-ws.mjs")
        || runtime.get("bind_host").and_then(Value::as_str) != Some("127.0.0.1")
        || runtime.get("port").and_then(Value::as_u64) != Some(20128)
        || runtime.get("working_directory").and_then(Value::as_str) != Some(RUNTIME_RELATIVE_ROOT)
        || runtime.get("data_dir_policy").and_then(Value::as_str)
            != Some("runtime_service_private")
    {
        return Err(host_error("android_host_omniroute_runtime_profile_contract_mismatch"));
    }
    let configured_env = runtime
        .get("environment")
        .and_then(Value::as_object)
        .ok_or_else(|| host_error("android_host_omniroute_runtime_environment_missing"))?;
    let required_env: HashMap<&str, &str> = HashMap::from([
        ("API_PORT", "20128"),
        ("DASHBOARD_PORT", "20128"),
        ("HOSTNAME", "127.0.0.1"),
        ("NODE_ENV", "production"),
        ("OMNIROUTE_MEMORY_MB", "512"),
        ("OMNIROUTE_MITM_STUB", "1"),
        ("OMNIROUTE_PORT", "20128"),
        ("PORT", "20128"),
        ("VECTOR_STORE_DISABLE_VEC", "true"),
    ]);
    if configured_env.len() != required_env.len()
        || required_env.iter().any(|(key, value)| {
            configured_env.get(*key).and_then(Value::as_str) != Some(*value)
        })
    {
        return Err(host_error("android_host_omniroute_runtime_environment_mismatch"));
    }

    let mut environment = configured_env
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|text| (key.clone(), text.to_owned()))
                .ok_or_else(|| host_error("android_host_omniroute_runtime_environment_invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    let data_dir = service_data
        .to_str()
        .ok_or_else(|| host_error("android_host_omniroute_data_dir_non_utf8"))?;
    environment.push(("DATA_DIR".into(), data_dir.to_owned()));
    environment.sort_by(|left, right| left.0.cmp(&right.0));

    Ok(LaunchPlan {
        working_directory: PathBuf::from(RUNTIME_RELATIVE_ROOT),
        args: vec![
            "--dns-result-order=ipv4first".into(),
            "--max-old-space-size=512".into(),
            "server-ws.mjs".into(),
        ],
        environment,
    })
}

async fn wait_for_omniroute_readiness(
    process_runtime: &vibecoder_process_local::LocalProcessRuntime,
    process: &RunningProcess,
    timeout_ms: u64,
) -> Result<OmniRouteServiceReadiness> {
    let client = OmniRouteClient::new(OmniRouteConfig {
        base_url: LOOPBACK_BASE_URL.into(),
        request_timeout_ms: READY_HTTP_TIMEOUT_MS,
        max_response_bytes: 64 * 1024,
    })?;
    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms);
    let mut attempts = 0usize;
    let mut consecutive = 0usize;

    loop {
        for event in process.drain_events(MAX_EVENT_DRAIN)? {
            if matches!(event, ProcessEvent::Finished { .. }) {
                return Err(host_error("android_host_omniroute_exited_before_ready"));
            }
        }
        if process_runtime.active_runtime_service(NODE_RUNTIME_SERVICE_ID)? != 1 {
            return Err(host_error("android_host_omniroute_inactive_before_ready"));
        }

        attempts = attempts.saturating_add(1);
        match client.execution_profile(GatewayCredential::Anonymous).await {
            Ok(profile) => {
                if !profile.permits_exact_model_execution()
                    || profile.gateway_id != EXPECTED_COMPONENT
                    || profile.upstream_version != EXPECTED_VERSION
                    || profile.profile_id != EXPECTED_GATEWAY_PROFILE_ID
                    || profile.profile_sha256 != EXPECTED_ROUTING_PROFILE_SHA256
                {
                    return Err(host_error("android_host_omniroute_runtime_attestation_mismatch"));
                }
                consecutive = consecutive.saturating_add(1);
                if consecutive >= READY_CONSECUTIVE_ATTESTATIONS {
                    if process_runtime.active_runtime_service(NODE_RUNTIME_SERVICE_ID)? != 1 {
                        return Err(host_error("android_host_omniroute_inactive_after_attestation"));
                    }
                    let elapsed = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                    return Ok(OmniRouteServiceReadiness {
                        process_id: process.process_id(),
                        base_url: LOOPBACK_BASE_URL.into(),
                        upstream_version: profile.upstream_version,
                        routing_profile_id: profile.profile_id,
                        routing_profile_sha256: profile.profile_sha256,
                        exact_model_only: profile.exact_model_only,
                        hidden_model_reroutes_disabled: profile.hidden_model_reroutes_disabled,
                        probe_attempts: attempts,
                        elapsed_ms: elapsed,
                    });
                }
            }
            Err(VibeCoderError::Gateway(code)) => {
                if code == "runtime_profile_attestation_mismatch" {
                    return Err(host_error("android_host_omniroute_runtime_attestation_mismatch"));
                }
                consecutive = 0;
            }
            Err(_) => {
                consecutive = 0;
            }
        }

        if Instant::now() >= deadline {
            return Err(host_error("android_host_omniroute_readiness_timeout"));
        }
        tokio::time::sleep(Duration::from_millis(READY_POLL_MS)).await;
    }
}

fn verify_runtime_tree(
    root: &Path,
    expected_manifest_sha256: &str,
) -> Result<OmniRouteRuntimeVerification> {
    let root_meta = fs::symlink_metadata(root)
        .map_err(|_| host_error("android_host_omniroute_runtime_missing"))?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(host_error("android_host_omniroute_runtime_root_invalid"));
    }
    let canonical_root = fs::canonicalize(root)
        .map_err(|_| host_error("android_host_omniroute_runtime_canonicalize_failed"))?;
    if canonical_root != root {
        return Err(host_error("android_host_omniroute_runtime_root_identity_mismatch"));
    }

    let manifest_path = canonical_root.join(MANIFEST_NAME);
    let manifest_bytes = read_bounded_file(&manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest_sha256 = sha256_bytes(&manifest_bytes);
    if !is_sha256(expected_manifest_sha256) || manifest_sha256 != expected_manifest_sha256 {
        return Err(host_error("android_host_omniroute_signed_manifest_sha_mismatch"));
    }
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| host_error("android_host_omniroute_manifest_invalid"))?;
    validate_manifest_identity(&manifest)?;

    let profile: Value = serde_json::from_str(RUNTIME_PROFILE_JSON)
        .map_err(|_| host_error("android_host_omniroute_runtime_profile_invalid"))?;
    if profile.get("runtime") != Some(&manifest.runtime) {
        return Err(host_error("android_host_omniroute_manifest_runtime_profile_mismatch"));
    }

    let receipt_path = canonical_root.join(RECEIPT_NAME);
    let receipt_bytes = read_bounded_file(&receipt_path, MAX_RECEIPT_BYTES)?;
    let receipt: InstallReceipt = serde_json::from_slice(&receipt_bytes)
        .map_err(|_| host_error("android_host_omniroute_receipt_invalid"))?;
    if receipt.schema != 1
        || receipt.component_id != EXPECTED_COMPONENT
        || receipt.version != EXPECTED_VERSION
        || receipt.profile_id != EXPECTED_PROFILE_ID
        || receipt.manifest_sha256 != manifest_sha256
        || receipt.tree_sha256 != manifest.tree_sha256
    {
        return Err(host_error("android_host_omniroute_receipt_mismatch"));
    }

    let mut expected_paths = HashSet::new();
    let mut total_bytes = 0u64;
    let mut tree_hasher = Sha256::new();
    for item in &manifest.files {
        validate_manifest_relative_path(&item.path)?;
        if item.size > MAX_FILE_BYTES || !is_sha256(&item.sha256) || !expected_paths.insert(item.path.clone()) {
            return Err(host_error("android_host_omniroute_manifest_file_invalid"));
        }
        let path = canonical_relative_file(&canonical_root, &item.path)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| host_error("android_host_omniroute_payload_missing"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != item.size {
            return Err(host_error("android_host_omniroute_payload_identity_mismatch"));
        }
        let actual = sha256_file(&path)?;
        if actual != item.sha256 {
            return Err(host_error("android_host_omniroute_payload_sha_mismatch"));
        }
        total_bytes = total_bytes
            .checked_add(item.size)
            .ok_or_else(|| host_error("android_host_omniroute_total_bytes_overflow"))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(host_error("android_host_omniroute_total_bytes_exceeded"));
        }
        tree_hasher.update(item.path.as_bytes());
        tree_hasher.update(b"\0");
        tree_hasher.update(item.size.to_string().as_bytes());
        tree_hasher.update(b"\0");
        tree_hasher.update(item.sha256.as_bytes());
        tree_hasher.update(b"\n");
    }
    if manifest.files.len() != manifest.file_count
        || manifest.files.len() > MAX_FILES
        || total_bytes != manifest.total_bytes
        || hex_digest(tree_hasher.finalize().as_slice()) != manifest.tree_sha256
    {
        return Err(host_error("android_host_omniroute_manifest_tree_mismatch"));
    }
    if !expected_paths.contains("server-ws.mjs") {
        return Err(host_error("android_host_omniroute_entrypoint_missing"));
    }

    let mut actual_paths = HashSet::new();
    collect_runtime_files(&canonical_root, &canonical_root, &mut actual_paths)?;
    actual_paths.remove(MANIFEST_NAME);
    actual_paths.remove(RECEIPT_NAME);
    if actual_paths != expected_paths {
        return Err(host_error("android_host_omniroute_unexpected_or_missing_files"));
    }

    Ok(OmniRouteRuntimeVerification {
        root: canonical_root,
        manifest_sha256,
        tree_sha256: manifest.tree_sha256,
        file_count: manifest.file_count,
        total_bytes,
    })
}

fn validate_manifest_identity(manifest: &BundleManifest) -> Result<()> {
    if manifest.schema != 1
        || manifest.component_id != EXPECTED_COMPONENT
        || manifest.version != EXPECTED_VERSION
        || manifest.profile_id != EXPECTED_PROFILE_ID
        || manifest.required_node_version != EXPECTED_NODE_VERSION
        || manifest.source_archive_sha256 != EXPECTED_SOURCE_SHA256
        || manifest.routing_patch_profile_sha256 != EXPECTED_ROUTING_PROFILE_SHA256
        || manifest.apk_asset_packaged
        || manifest.service_round_trip_proven
        || !is_sha256(&manifest.tree_sha256)
    {
        return Err(host_error("android_host_omniroute_manifest_identity_mismatch"));
    }
    Ok(())
}

fn validate_manifest_relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains("//")
        || path == MANIFEST_NAME
        || path == RECEIPT_NAME
    {
        return Err(host_error("android_host_omniroute_manifest_path_invalid"));
    }
    let parsed = Path::new(path);
    if parsed.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(host_error("android_host_omniroute_manifest_path_invalid"));
    }
    Ok(())
}

fn canonical_relative_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    let parts = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in parts.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(host_error("android_host_omniroute_manifest_path_invalid"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| host_error("android_host_omniroute_payload_missing"))?;
        if metadata.file_type().is_symlink() {
            return Err(host_error("android_host_omniroute_runtime_symlink_forbidden"));
        }
        if index + 1 < parts.len() && !metadata.is_dir() {
            return Err(host_error("android_host_omniroute_payload_parent_invalid"));
        }
    }
    let canonical = fs::canonicalize(&current)
        .map_err(|_| host_error("android_host_omniroute_payload_canonicalize_failed"))?;
    if !canonical.starts_with(root) || canonical != current {
        return Err(host_error("android_host_omniroute_payload_path_escape"));
    }
    Ok(canonical)
}

fn collect_runtime_files(root: &Path, directory: &Path, files: &mut HashSet<String>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .map_err(|_| host_error("android_host_omniroute_runtime_walk_failed"))?
    {
        let entry = entry.map_err(|_| host_error("android_host_omniroute_runtime_walk_failed"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| host_error("android_host_omniroute_runtime_walk_failed"))?;
        if metadata.file_type().is_symlink() {
            return Err(host_error("android_host_omniroute_runtime_symlink_forbidden"));
        }
        if metadata.is_dir() {
            collect_runtime_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| host_error("android_host_omniroute_runtime_walk_escape"))?
                .to_str()
                .ok_or_else(|| host_error("android_host_omniroute_runtime_path_non_utf8"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            files.insert(relative);
            if files.len() > MAX_FILES + 2 {
                return Err(host_error("android_host_omniroute_runtime_file_limit"));
            }
        } else {
            return Err(host_error("android_host_omniroute_runtime_special_file_forbidden"));
        }
    }
    Ok(())
}

fn read_bounded_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| host_error("android_host_omniroute_metadata_read_failed"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Err(host_error("android_host_omniroute_bounded_file_invalid"));
    }
    fs::read(path).map_err(|_| host_error("android_host_omniroute_file_read_failed"))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|_| host_error("android_host_omniroute_file_read_failed"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| host_error("android_host_omniroute_file_read_failed"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_plan_is_loopback_only_and_uses_private_data_dir() {
        let plan = build_launch_plan(Path::new("/data/user/0/example/files/vibecoder/runtime/services/node-runtime"))
            .expect("launch plan");
        assert_eq!(plan.working_directory, Path::new("omniroute"));
        assert_eq!(plan.args.last().map(String::as_str), Some("server-ws.mjs"));
        let env = plan.environment.into_iter().collect::<HashMap<_, _>>();
        assert_eq!(env.get("HOSTNAME").map(String::as_str), Some("127.0.0.1"));
        assert_eq!(env.get("PORT").map(String::as_str), Some("20128"));
        assert_eq!(env.get("API_PORT").map(String::as_str), Some("20128"));
        assert_eq!(env.get("NODE_ENV").map(String::as_str), Some("production"));
        assert_eq!(
            env.get("DATA_DIR").map(String::as_str),
            Some("/data/user/0/example/files/vibecoder/runtime/services/node-runtime")
        );
        assert!(!env.contains_key("PATH"));
        assert!(!env.contains_key("NODE_OPTIONS"));
    }

    #[test]
    fn manifest_paths_reject_escape_and_reserved_files() {
        assert!(validate_manifest_relative_path("server-ws.mjs").is_ok());
        assert!(validate_manifest_relative_path("node_modules/sql.js/package.json").is_ok());
        assert!(validate_manifest_relative_path("../escape").is_err());
        assert!(validate_manifest_relative_path("/absolute").is_err());
        assert!(validate_manifest_relative_path("a\\b").is_err());
        assert!(validate_manifest_relative_path(MANIFEST_NAME).is_err());
        assert!(validate_manifest_relative_path(RECEIPT_NAME).is_err());
    }

    #[test]
    fn readiness_requires_two_attestations() {
        assert!(READY_CONSECUTIVE_ATTESTATIONS >= 2);
        assert_eq!(LOOPBACK_BASE_URL, "http://127.0.0.1:20128/v1");
    }
}
