#!/usr/bin/env python3
"""Static/regression contract checks for Part 34.3.4 OmniRoute service lifecycle."""
from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROFILE_SHA256 = "c9d8cfa91c5d8ec1e4f5862fe4d6e6266ad02db9286daf0b5350268ad0bc3625"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(text: str, *tokens: str) -> None:
    for token in tokens:
        if token not in text:
            raise AssertionError(f"contract token missing: {token}")


def main() -> int:
    profile_path = ROOT / "config/omniroute-android-runtime-profile.json"
    import hashlib
    if hashlib.sha256(profile_path.read_bytes()).hexdigest() != PROFILE_SHA256:
        raise AssertionError("Android runtime profile hash drifted")
    profile = json.loads(profile_path.read_text(encoding="utf-8"))
    runtime = profile["runtime"]
    if runtime.get("working_directory") != "omniroute":
        raise AssertionError("runtime working directory drifted")
    if runtime.get("data_dir_policy") != "runtime_service_private":
        raise AssertionError("runtime data directory policy drifted")
    expected_env = {
        "API_PORT": "20128",
        "DASHBOARD_PORT": "20128",
        "HOSTNAME": "127.0.0.1",
        "NODE_ENV": "production",
        "OMNIROUTE_MEMORY_MB": "512",
        "OMNIROUTE_MITM_STUB": "1",
        "OMNIROUTE_PORT": "20128",
        "PORT": "20128",
        "VECTOR_STORE_DISABLE_VEC": "true",
    }
    if runtime.get("environment") != expected_env:
        raise AssertionError("runtime environment drifted")

    process = read("crates/vibecoder-process-local/src/lib.rs")
    require(
        process,
        "pub fn start_persistent_runtime_service",
        "timeout: None",
        "runtime_service_private_directory",
        "resolve_runtime_working_directory",
        ".env_clear()",
        '"PATH"',
        '"NODE_OPTIONS"',
        '"LD_PRELOAD"',
        "PR_SET_PDEATHSIG",
    )
    persistent = process[process.index("pub fn start_persistent_runtime_service"):process.index("pub fn active_runtime_service")]
    if "ProcessExecutionOptions" in persistent:
        raise AssertionError("persistent service accidentally inherited command timeout options")

    lock = read("Cargo.lock")
    lock_start = lock.index('name = "vibecoder-android-host"')
    lock_end = lock.find("[[package]]", lock_start)
    host_lock = lock[lock_start:lock_end if lock_end != -1 else None]
    for dependency in ('"sha2"', '"vibecoder-gateway-contract"', '"vibecoder-gateway-omniroute"'):
        if dependency not in host_lock:
            raise AssertionError(f"android-host Cargo.lock dependency missing: {dependency}")

    service = read("crates/vibecoder-android-host/src/omniroute_service.rs")
    require(
        service,
        "start_omniroute_service",
        "verify_installed_omniroute_runtime",
        "android_host_omniroute_signed_manifest_sha_mismatch",
        "start_persistent_runtime_service",
        "READY_CONSECUTIVE_ATTESTATIONS",
        "GatewayCredential::Anonymous",
        "execution_profile",
        "http://127.0.0.1:20128/v1",
        "android_host_omniroute_runtime_attestation_mismatch",
        "android_host_omniroute_readiness_timeout",
        "stop_omniroute_service",
        "restart_omniroute_service",
        "No automatic restart",
    )
    if "READY_CONSECUTIVE_ATTESTATIONS: usize = 2" not in service:
        raise AssertionError("readiness must require exactly two consecutive profile attestations")

    ffi = read("crates/vibecoder-android-host/src/omniroute_ffi.rs")
    require(
        ffi,
        "OMNIROUTE_SESSION",
        "vibecoder_android_host_omniroute_start_json",
        "vibecoder_android_host_omniroute_status_json",
        "vibecoder_android_host_omniroute_stop_json",
        "runtime_profile_round_trip_proven",
        "signed_manifest_sha256",
        "exited_pending_reap",
        "Start is mutating",
        "Stop is mutating",
        "if output.is_null() || output_capacity == 0",
    )

    bridge_java = read("android/app/src/main/java/com/vibecoder/shell/NativeBridge.java")
    bridge_c = read("android/app/src/main/cpp/native_bridge.c")
    activity = read("android/app/src/main/java/com/vibecoder/shell/MainActivity.java")
    device = read("scripts/test_android_diagnostic_device.sh")
    apk = read("scripts/verify_android_diagnostic_apk.sh")
    require(bridge_java, "nativeOmniRouteStart", "nativeOmniRouteStatus", "nativeOmniRouteStop")
    require(
        bridge_c,
        "vibecoder_android_host_omniroute_start_json",
        "vibecoder_android_host_omniroute_status_json",
        "vibecoder_android_host_omniroute_stop_json",
        "const size_t capacity = 1024u * 1024u",
    )
    # Mutating start/stop JNI wrappers must be one-shot; a null-buffer size query would execute
    # the mutation twice and create state-dependent JSON length races.
    omni_c = bridge_c[bridge_c.index("static jstring omniroute_call_simple"):]
    if "rust_host_omniroute_start(\n            app" in omni_c and "manifest_sha, NULL, 0u" in omni_c:
        raise AssertionError("OmniRoute JNI start still uses mutating two-call size query")
    require(activity, "vibecoder_omniroute_service_test", "vibecoder_omniroute_service_stop_after_probe",
            "manifest_sha256", "nativeOmniRouteStart", "nativeOmniRouteStatus", "nativeOmniRouteStop",
            "service_round_trip_proven")
    require(device, "omniroute_service", "runtime_profile_round_trip_proven", "signed_manifest_sha256",
            "omniroute_service_explicit_stop_failed", "service_round_trip_proven")
    require(apk, "omniroute_service", "libvibecoder_node_exec.so")

    print("Part 34.3.4 service-tool regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
