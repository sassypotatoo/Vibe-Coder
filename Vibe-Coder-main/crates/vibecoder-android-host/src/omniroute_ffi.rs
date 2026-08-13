use super::{
    AndroidHostPaths, AndroidHostRuntime, OmniRouteServiceHandle, ffi_path, host_error,
    ANDROID_HOST_FFI_BUFFER_TOO_SMALL, ANDROID_HOST_FFI_ERROR, ANDROID_HOST_FFI_MAX_JSON_BYTES,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};
use vibecoder_domain::Result;
use vibecoder_gateway_contract::GatewayCredential;
use vibecoder_process_contract::{ProcessResult, ProcessTermination};

const MANIFEST_SHA_BYTES: usize = 64;
const MAX_STATUS_JSON_BYTES: usize = 1024 * 1024;
const MAX_GATEWAY_CREDENTIAL_BYTES: usize = 8192;
const MAX_INFERENCE_MODEL_BYTES: usize = 512;
const MAX_INFERENCE_PROMPT_BYTES: usize = 64 * 1024;

static OMNIROUTE_SESSION: OnceLock<Mutex<Option<AndroidOmniRouteSession>>> = OnceLock::new();

struct AndroidOmniRouteSession {
    key: String,
    host: AndroidHostRuntime,
    service: Option<OmniRouteServiceHandle>,
    last_exit: Option<ExitSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct ExitSummary {
    termination: &'static str,
    exit_code: Option<i32>,
    stdout_bytes: usize,
    stderr_bytes: usize,
    stdout_truncated: bool,
    stderr_truncated: bool,
    event_queue_overflowed: bool,
    duration_ms: u64,
}

#[derive(Serialize)]
struct ServiceSnapshot {
    schema: u32,
    component_id: &'static str,
    status: &'static str,
    active: bool,
    ready: bool,
    process_id: Option<String>,
    base_url: Option<String>,
    runtime_tree_sha256: Option<String>,
    signed_manifest_sha256: Option<String>,
    runtime_profile_round_trip_proven: bool,
    exact_model_only: bool,
    hidden_model_reroutes_disabled: bool,
    probe_attempts: Option<usize>,
    readiness_elapsed_ms: Option<u64>,
    last_exit: Option<ExitSummary>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_omniroute_start_json(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    expected_manifest_sha256: *const c_char,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    // Start is mutating: unlike the read-only probe/status ABI, it deliberately has no null-buffer
    // size-query mode. A two-call query/write pattern would execute service startup twice.
    if output.is_null() || output_capacity == 0 {
        return ANDROID_HOST_FFI_ERROR;
    }
    let result = std::panic::catch_unwind(|| unsafe {
        start_bytes(
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
            inventory_json,
            inventory_len,
            expected_manifest_sha256,
        )
    });
    write_ffi_json(result, output, output_capacity)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_omniroute_status_json(
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    let result = std::panic::catch_unwind(status_bytes);
    write_ffi_json(result, output, output_capacity)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_omniroute_gateway_probe_json(
    credential_utf8: *const u8,
    credential_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    // This call performs network I/O and is deliberately one-shot. A null-buffer size query would
    // repeat the gateway request and could consume quota or observe a different catalog state.
    if output.is_null() || output_capacity == 0 {
        return ANDROID_HOST_FFI_ERROR;
    }
    let result = std::panic::catch_unwind(|| unsafe {
        gateway_probe_bytes(credential_utf8, credential_len)
    });
    write_ffi_json(result, output, output_capacity)
}


#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_omniroute_inference_probe_json(
    credential_utf8: *const u8,
    credential_len: usize,
    model_utf8: *const u8,
    model_len: usize,
    prompt_utf8: *const u8,
    prompt_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    // Inference is mutating external quota/state and is strictly one-shot. A null-buffer size query
    // would send the model request twice.
    if output.is_null() || output_capacity == 0 {
        return ANDROID_HOST_FFI_ERROR;
    }
    let result = std::panic::catch_unwind(|| unsafe {
        inference_probe_bytes(
            credential_utf8,
            credential_len,
            model_utf8,
            model_len,
            prompt_utf8,
            prompt_len,
        )
    });
    write_ffi_json(result, output, output_capacity)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_omniroute_stop_json(
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    // Stop is mutating for the same reason as start and is therefore one-shot only.
    if output.is_null() || output_capacity == 0 {
        return ANDROID_HOST_FFI_ERROR;
    }
    let result = std::panic::catch_unwind(stop_bytes);
    write_ffi_json(result, output, output_capacity)
}

unsafe fn start_bytes(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    expected_manifest_sha256: *const c_char,
) -> Result<Vec<u8>> {
    if app_private_dir.is_null()
        || native_library_dir.is_null()
        || packaged_executable_dir.is_null()
        || inventory_json.is_null()
        || inventory_len == 0
        || inventory_len > ANDROID_HOST_FFI_MAX_JSON_BYTES
        || expected_manifest_sha256.is_null()
    {
        return Err(host_error("android_host_omniroute_ffi_arguments_invalid"));
    }
    let app = unsafe { ffi_path(app_private_dir)? };
    let native = unsafe { ffi_path(native_library_dir)? };
    let exec = unsafe { ffi_path(packaged_executable_dir)? };
    let inventory = unsafe { std::slice::from_raw_parts(inventory_json, inventory_len) };
    let manifest_sha = unsafe { ffi_manifest_sha(expected_manifest_sha256)? };
    let paths = AndroidHostPaths::new(app, native, exec)?;
    let key = session_key(&paths, inventory);

    let lock = OMNIROUTE_SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = lock
        .lock()
        .map_err(|_| host_error("android_host_omniroute_session_poisoned"))?;
    if guard.is_none() {
        let host = AndroidHostRuntime::from_inventory_json(paths, inventory)?;
        *guard = Some(AndroidOmniRouteSession {
            key: key.clone(),
            host,
            service: None,
            last_exit: None,
        });
    }
    let session = guard
        .as_mut()
        .ok_or_else(|| host_error("android_host_omniroute_session_missing"))?;
    if session.key != key {
        return Err(host_error("android_host_omniroute_session_identity_mismatch"));
    }

    if let Some(existing) = session.service.as_ref() {
        if session.host.omniroute_service_active()? {
            if existing.runtime_verification().manifest_sha256 != manifest_sha {
                return Err(host_error("android_host_omniroute_running_manifest_mismatch"));
            }
            return snapshot_bytes(session, "already_ready");
        }
    }
    if let Some(stale) = session.service.take() {
        let result = session.host.wait_omniroute_service(stale)?;
        session.last_exit = Some(exit_summary(&result));
    }

    let service = session.host.start_omniroute_service(&manifest_sha)?;
    session.service = Some(service);
    snapshot_bytes(session, "started_ready")
}

unsafe fn gateway_probe_bytes(credential_utf8: *const u8, credential_len: usize) -> Result<Vec<u8>> {
    if credential_len > MAX_GATEWAY_CREDENTIAL_BYTES
        || (credential_len > 0 && credential_utf8.is_null())
    {
        return Err(host_error("android_host_omniroute_gateway_credential_invalid"));
    }
    let credential = if credential_len == 0 {
        GatewayCredential::Anonymous
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(credential_utf8, credential_len) };
        let value = std::str::from_utf8(bytes)
            .map_err(|_| host_error("android_host_omniroute_gateway_credential_invalid"))?;
        if value.is_empty()
            || value.trim() != value
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(host_error("android_host_omniroute_gateway_credential_invalid"));
        }
        GatewayCredential::Secret(value)
    };

    let lock = OMNIROUTE_SESSION.get_or_init(|| Mutex::new(None));
    let guard = lock
        .lock()
        .map_err(|_| host_error("android_host_omniroute_session_poisoned"))?;
    let session = guard
        .as_ref()
        .ok_or_else(|| host_error("android_host_omniroute_not_started"))?;
    let service = session
        .service
        .as_ref()
        .ok_or_else(|| host_error("android_host_omniroute_not_started"))?;
    let probe = session
        .host
        .probe_omniroute_gateway_transport(service, credential)?;
    let bytes = serde_json::to_vec(&probe)
        .map_err(|_| host_error("android_host_omniroute_gateway_probe_json_failed"))?;
    if bytes.len() > MAX_STATUS_JSON_BYTES {
        return Err(host_error("android_host_omniroute_gateway_probe_json_too_large"));
    }
    Ok(bytes)
}


unsafe fn inference_probe_bytes(
    credential_utf8: *const u8,
    credential_len: usize,
    model_utf8: *const u8,
    model_len: usize,
    prompt_utf8: *const u8,
    prompt_len: usize,
) -> Result<Vec<u8>> {
    if credential_len > MAX_GATEWAY_CREDENTIAL_BYTES
        || (credential_len > 0 && credential_utf8.is_null())
        || model_len == 0
        || model_len > MAX_INFERENCE_MODEL_BYTES
        || model_utf8.is_null()
        || prompt_len == 0
        || prompt_len > MAX_INFERENCE_PROMPT_BYTES
        || prompt_utf8.is_null()
    {
        return Err(host_error("android_host_omniroute_inference_arguments_invalid"));
    }
    let credential = if credential_len == 0 {
        GatewayCredential::Anonymous
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(credential_utf8, credential_len) };
        let value = std::str::from_utf8(bytes)
            .map_err(|_| host_error("android_host_omniroute_gateway_credential_invalid"))?;
        if value.is_empty()
            || value.trim() != value
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(host_error("android_host_omniroute_gateway_credential_invalid"));
        }
        GatewayCredential::Secret(value)
    };
    let model_bytes = unsafe { std::slice::from_raw_parts(model_utf8, model_len) };
    let model = std::str::from_utf8(model_bytes)
        .map_err(|_| host_error("android_host_omniroute_inference_model_invalid"))?;
    let prompt_bytes = unsafe { std::slice::from_raw_parts(prompt_utf8, prompt_len) };
    let prompt = std::str::from_utf8(prompt_bytes)
        .map_err(|_| host_error("android_host_omniroute_inference_prompt_invalid"))?;

    let lock = OMNIROUTE_SESSION.get_or_init(|| Mutex::new(None));
    let guard = lock
        .lock()
        .map_err(|_| host_error("android_host_omniroute_session_poisoned"))?;
    let session = guard
        .as_ref()
        .ok_or_else(|| host_error("android_host_omniroute_not_started"))?;
    let service = session
        .service
        .as_ref()
        .ok_or_else(|| host_error("android_host_omniroute_not_started"))?;
    let probe = session
        .host
        .probe_omniroute_first_inference(service, credential, model, prompt)?;
    let bytes = serde_json::to_vec(&probe)
        .map_err(|_| host_error("android_host_omniroute_inference_probe_json_failed"))?;
    if bytes.len() > MAX_STATUS_JSON_BYTES {
        return Err(host_error("android_host_omniroute_inference_probe_json_too_large"));
    }
    Ok(bytes)
}

fn status_bytes() -> Result<Vec<u8>> {
    let lock = OMNIROUTE_SESSION.get_or_init(|| Mutex::new(None));
    let guard = lock
        .lock()
        .map_err(|_| host_error("android_host_omniroute_session_poisoned"))?;
    let Some(session) = guard.as_ref() else {
        return serialize_snapshot(ServiceSnapshot {
            schema: 1,
            component_id: "omniroute",
            status: "not_started",
            active: false,
            ready: false,
            process_id: None,
            base_url: None,
            runtime_tree_sha256: None,
            signed_manifest_sha256: None,
            runtime_profile_round_trip_proven: false,
            exact_model_only: false,
            hidden_model_reroutes_disabled: false,
            probe_attempts: None,
            readiness_elapsed_ms: None,
            last_exit: None,
        });
    };
    let status = if session.host.omniroute_service_active()? {
        "ready"
    } else if session.service.is_some() {
        "exited_pending_reap"
    } else {
        "stopped"
    };
    snapshot_bytes(session, status)
}

fn stop_bytes() -> Result<Vec<u8>> {
    let lock = OMNIROUTE_SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = lock
        .lock()
        .map_err(|_| host_error("android_host_omniroute_session_poisoned"))?;
    let Some(session) = guard.as_mut() else {
        return Err(host_error("android_host_omniroute_not_started"));
    };
    let Some(service) = session.service.take() else {
        return snapshot_bytes(session, "already_stopped");
    };
    let result = if session.host.omniroute_service_active()? {
        session.host.stop_omniroute_service(service)?
    } else {
        session.host.wait_omniroute_service(service)?
    };
    session.last_exit = Some(exit_summary(&result));
    snapshot_bytes(session, "stopped")
}

fn snapshot_bytes(session: &AndroidOmniRouteSession, status: &'static str) -> Result<Vec<u8>> {
    let active = session.host.omniroute_service_active()?;
    let service = session.service.as_ref();
    let readiness = service.map(|value| value.readiness());
    let runtime = service.map(|value| value.runtime_verification());
    serialize_snapshot(ServiceSnapshot {
        schema: 1,
        component_id: "omniroute",
        status,
        active,
        ready: active && readiness.is_some(),
        process_id: service.map(|value| value.process_id().as_uuid().to_string()),
        base_url: readiness.map(|value| value.base_url.clone()),
        runtime_tree_sha256: runtime.map(|value| value.tree_sha256.clone()),
        signed_manifest_sha256: runtime.map(|value| value.manifest_sha256.clone()),
        runtime_profile_round_trip_proven: active && readiness.is_some(),
        exact_model_only: readiness.is_some_and(|value| value.exact_model_only),
        hidden_model_reroutes_disabled: readiness
            .is_some_and(|value| value.hidden_model_reroutes_disabled),
        probe_attempts: readiness.map(|value| value.probe_attempts),
        readiness_elapsed_ms: readiness.map(|value| value.elapsed_ms),
        last_exit: session.last_exit.clone(),
    })
}

fn serialize_snapshot(snapshot: ServiceSnapshot) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&snapshot)
        .map_err(|_| host_error("android_host_omniroute_status_json_failed"))?;
    if bytes.len() > MAX_STATUS_JSON_BYTES {
        return Err(host_error("android_host_omniroute_status_json_too_large"));
    }
    Ok(bytes)
}

unsafe fn ffi_manifest_sha(pointer: *const c_char) -> Result<String> {
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.len() != MANIFEST_SHA_BYTES
        || !bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(host_error("android_host_omniroute_manifest_sha_invalid"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| host_error("android_host_omniroute_manifest_sha_invalid"))?;
    Ok(text.to_owned())
}

fn session_key(paths: &AndroidHostPaths, inventory: &[u8]) -> String {
    let mut hasher = Sha256::new();
    for path in [
        paths.app_private_dir(),
        paths.native_library_dir(),
        paths.packaged_executable_dir(),
    ] {
        hasher.update(path.as_os_str().to_string_lossy().as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(inventory);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn exit_summary(result: &ProcessResult) -> ExitSummary {
    ExitSummary {
        termination: match result.termination {
            ProcessTermination::Exited => "exited",
            ProcessTermination::Signaled => "signaled",
            ProcessTermination::TimedOut => "timed_out",
            ProcessTermination::Cancelled => "cancelled",
        },
        exit_code: result.exit_code,
        stdout_bytes: result.stdout.len(),
        stderr_bytes: result.stderr.len(),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        event_queue_overflowed: result.event_queue_overflowed,
        duration_ms: result.duration_ms,
    }
}

fn write_ffi_json(
    result: std::thread::Result<Result<Vec<u8>>>,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_sha_parser_is_exact_lowercase_hex() {
        let valid = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\0";
        let parsed = unsafe { ffi_manifest_sha(valid.as_ptr().cast()) }.expect("valid sha");
        assert_eq!(parsed.len(), 64);
        let upper = b"ABCDEF6789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\0";
        assert!(unsafe { ffi_manifest_sha(upper.as_ptr().cast()) }.is_err());
    }
}
