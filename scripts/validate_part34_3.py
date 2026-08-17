#!/usr/bin/env python3
"""Static/source validator for Part 34.3 OmniRoute Android packaging."""
from __future__ import annotations
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ERRORS: list[str] = []
EXPECTED_ARCHIVE = "1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7"
EXPECTED_PROFILE = "aec0f63fb0dec08f24fffde9209504ec447e9428bec1cd64c033649ed275fe3d"
EXPECTED_ANDROID_PROFILE = "c9d8cfa91c5d8ec1e4f5862fe4d6e6266ad02db9286daf0b5350268ad0bc3625"


def read(path: str) -> str:
    p = ROOT / path
    if not p.is_file():
        ERRORS.append(f"missing {path}")
        return ""
    return p.read_text(encoding="utf-8", errors="replace")


def require(text: str, token: str, label: str) -> None:
    if token not in text:
        ERRORS.append(f"{label} missing: {token}")


def main() -> int:
    try:
        provenance = json.loads(read("third_party/provenance/omniroute-3.8.50-reviewed.json"))
    except Exception as exc:
        ERRORS.append(f"invalid OmniRoute provenance JSON: {exc}")
        provenance = {}
    if provenance.get("reviewed_archive_sha256") != EXPECTED_ARCHIVE:
        ERRORS.append("OmniRoute reviewed archive hash drifted")
    if provenance.get("reviewed_archive_size_bytes") != 64028571:
        ERRORS.append("OmniRoute reviewed archive size drifted")
    if provenance.get("reviewed_archive_entry_count") != 13622:
        ERRORS.append("OmniRoute reviewed entry count drifted")
    if provenance.get("package_version") != "3.8.50":
        ERRORS.append("OmniRoute package version drifted")
    if provenance.get("node_engine") != ">=22.22.2 <23 || >=24.0.0 <27":
        ERRORS.append("OmniRoute Node engine drifted")
    if provenance.get("vibecoder_patch_profile_sha256") != EXPECTED_PROFILE:
        ERRORS.append("OmniRoute patch profile drifted")
    if provenance.get("prebuilt_runtime_present_in_reviewed_archive") is not False:
        ERRORS.append("reviewed OmniRoute archive must not be treated as a prebuilt runtime")
    native = provenance.get("native_dependency_android_audit", {})
    for key in (
        "sharp_android_platform_package_present",
        "sqlite_vec_android_platform_package_present",
        "onnxruntime_node_android_supported",
        "wreq_js_android_supported",
    ):
        if native.get(key) is not False:
            ERRORS.append(f"Android native dependency audit marker must remain false: {key}")
    for key in ("node_sqlite_fallback_present", "sql_js_wasm_fallback_present", "sqlite_vec_graceful_degradation_present"):
        if native.get(key) is not True:
            ERRORS.append(f"OmniRoute fallback audit marker missing: {key}")

    prep = read("scripts/prepare_omniroute_android_source.py")
    for token in (
        "omniroute_reviewed_commit_mismatch",
        "omniroute_archive_parent_traversal",
        "omniroute_archive_symlink_forbidden",
        "omniroute_archive_duplicate_path",
        "omniroute_reviewed_archive_contains_generated_runtime",
        "omniroute_output_directory_protected",
        "omniroute_archive_max_entry_size_mismatch",
        "apply_omniroute_runtime_patches.py",
        "omniroute_patched_target_hash_mismatch",
        'runtime_bundle_built": False',
        '"archive_root": source.name',
    ):
        require(prep, token, "OmniRoute source admission")

    try:
        profile = json.loads(read("config/omniroute-android-runtime-profile.json"))
    except Exception as exc:
        ERRORS.append(f"invalid OmniRoute Android profile JSON: {exc}")
        profile = {}
    if profile.get("profile_id") != "vibecoder-omniroute-android-backend-v1":
        ERRORS.append("OmniRoute Android runtime profile id drifted")
    if profile.get("source", {}).get("reviewed_archive_sha256") != EXPECTED_ARCHIVE:
        ERRORS.append("OmniRoute Android profile source hash drifted")
    if profile.get("source", {}).get("routing_patch_profile_sha256") != EXPECTED_PROFILE:
        ERRORS.append("OmniRoute Android profile routing patch drifted")
    if profile.get("build", {}).get("required_node_version") != "24.19.0":
        ERRORS.append("OmniRoute Android build must pin Node 24.19.0")
    build_env = profile.get("build", {}).get("environment", {})
    for key, value in {
        "OMNIROUTE_BUILD_BACKEND_ONLY": "1",
        "OMNIROUTE_BUILD_PROFILE": "minimal",
        "OMNIROUTE_MITM_STUB": "1",
        "VECTOR_STORE_DISABLE_VEC": "true",
    }.items():
        if build_env.get(key) != value:
            ERRORS.append(f"OmniRoute Android build environment drifted: {key}")
    runtime = profile.get("runtime", {})
    if runtime.get("entrypoint") != "server-ws.mjs" or runtime.get("bind_host") != "127.0.0.1" or runtime.get("port") != 20128:
        ERRORS.append("OmniRoute Android loopback runtime profile drifted")
    if runtime.get("working_directory") != "omniroute" or runtime.get("data_dir_policy") != "runtime_service_private":
        ERRORS.append("OmniRoute Android runtime working/data directory policy drifted")
    required_runtime_env = {
        "API_PORT": "20128", "DASHBOARD_PORT": "20128", "HOSTNAME": "127.0.0.1",
        "NODE_ENV": "production", "OMNIROUTE_MEMORY_MB": "512", "OMNIROUTE_MITM_STUB": "1",
        "OMNIROUTE_PORT": "20128", "PORT": "20128", "VECTOR_STORE_DISABLE_VEC": "true",
    }
    if runtime.get("environment") != required_runtime_env:
        ERRORS.append("OmniRoute Android runtime environment drifted")
    forbidden = set(profile.get("forbidden_package_roots", []))
    for pkg in ("sharp", "sqlite-vec", "onnxruntime-node", "wreq-js", "better-sqlite3", "tls-client-node"):
        if pkg not in forbidden:
            ERRORS.append(f"OmniRoute Android native package not fail-closed: {pkg}")

    sealer = read("scripts/prepare_omniroute_android_bundle.py")
    for token in (
        "omniroute_android_bundle_host_native_binary_forbidden",
        "omniroute_android_bundle_symlink_forbidden",
        "omniroute_android_bundle_forbidden_package",
        "omniroute_android_bundle_required_path_missing",
        ".vibecoder-omniroute-bundle.json",
        "tree_sha256",
        "feature_degradations",
        "apk_asset_packaged\": False",
        "service_round_trip_proven\": False",
    ):
        require(sealer, token, "OmniRoute Android bundle sealer")

    verifier = read("scripts/verify_omniroute_android_bundle.py")
    for token in (
        "omniroute_android_bundle_file_manifest_mismatch",
        "omniroute_android_bundle_tree_hash_mismatch",
        "omniroute_android_bundle_manifest_overclaims_runtime_proof",
        "OmniRoute Android runtime bundle verification PASSED",
    ):
        require(verifier, token, "OmniRoute Android bundle verifier")

    root_regression = read("scripts/test_part34_3_source_root_resolution.py")
    for token in (
        "Part 34.3 source-root handoff regression PASSED",
        "OmniRoute-ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f",
        "omniroute_source_admission_root_invalid",
        "omniroute_source_admission_root_missing",
    ):
        require(root_regression, token, "OmniRoute source-root regression")

    regression = read("scripts/test_part34_3_bundle_tools.py")
    for token in (
        "Part 34.3 bundle-tool regression PASSED",
        "omniroute_bundle_external_symlink_forbidden",
        "omniroute_android_bundle_forbidden_package:wreq-js",
    ):
        require(regression, token, "OmniRoute Android bundle regression")

    builder = read("scripts/build_omniroute_android_bundle.py")
    for token in (
        "omniroute_android_build_node_version_mismatch",
        "npm_install_completed",
        "backend_build_completed",
        "android_bundle_sealed",
        "android_bundle_verified",
        "build:backend",
        "prepare_omniroute_android_source.py",
        "prepare_omniroute_android_bundle.py",
        "verify_omniroute_android_bundle.py",
        "resolve_prepared_source",
        "omniroute_source_admission_root_invalid",
        "omniroute_source_admission_root_missing",
    ):
        require(builder, token, "OmniRoute Android bundle builder")


    stager = read("scripts/stage_omniroute_android_asset.py")
    for token in (
        "omniroute_asset_stage_bundle_verification_failed",
        "omniroute_asset_stage_tracked_source_output_forbidden",
        "apk_asset_staged",
        "apk_asset_packaging_proven",
        "device_extraction_proven",
        "ASSET_RELATIVE_ROOT",
    ):
        require(stager, token, "OmniRoute APK asset stager")

    installer = read("android/app/src/main/java/com/vibecoder/shell/OmniRouteAssetInstaller.java")
    for token in (
        "vibecoder/runtime/omniroute",
        "lockChannel.lock()",
        "StandardCopyOption.ATOMIC_MOVE",
        "omniroute_asset_sha_mismatch",
        "omniroute_post_commit_verification_failed",
        ".omniroute-previous",
        ".omniroute-stage-",
        "omniroute_bundle_runtime_contract_mismatch",
        "service_round_trip_proven",
    ):
        require(installer, token, "OmniRoute app-private installer")

    build_gradle = read("android/app/build.gradle.kts")
    require(build_gradle, 'assets.srcDir("build/generated/omnirouteAssets")', "OmniRoute generated asset source set")

    asset_regression = read("scripts/test_part34_3_asset_tools.py")
    for token in (
        "Part 34.3.3 asset-tool regression PASSED",
        "omniroute_asset_stage_tracked_source_output_forbidden",
        "stale staged asset survived atomic replacement",
    ):
        require(asset_regression, token, "OmniRoute asset regression")

    apk_verify = read("scripts/verify_android_diagnostic_apk.sh")
    device_verify = read("scripts/test_android_diagnostic_device.sh")
    for text, label in ((apk_verify, "OmniRoute APK verifier"), (device_verify, "OmniRoute device verifier")):
        require(text, "omniroute_asset", label)
        require(text, "omniroute_service", label)

    process_local = read("crates/vibecoder-process-local/src/lib.rs")
    for token in (
        "pub fn start_persistent_runtime_service", "timeout: None", "runtime_service_private_directory",
        "resolve_runtime_working_directory", "process_runtime_service_env_key_forbidden",
        '"PATH"', '"NODE_OPTIONS"', "PR_SET_PDEATHSIG",
    ):
        require(process_local, token, "OmniRoute persistent process supervisor")

    service = read("crates/vibecoder-android-host/src/omniroute_service.rs")
    for token in (
        "start_omniroute_service", "verify_installed_omniroute_runtime",
        "android_host_omniroute_signed_manifest_sha_mismatch", "start_persistent_runtime_service",
        "READY_CONSECUTIVE_ATTESTATIONS: usize = 2", "GatewayCredential::Anonymous",
        "execution_profile", "http://127.0.0.1:20128/v1",
        "android_host_omniroute_runtime_attestation_mismatch", "android_host_omniroute_readiness_timeout",
        "stop_omniroute_service", "restart_omniroute_service",
    ):
        require(service, token, "OmniRoute service lifecycle")

    ffi = read("crates/vibecoder-android-host/src/omniroute_ffi.rs")
    for token in (
        "OMNIROUTE_SESSION", "vibecoder_android_host_omniroute_start_json",
        "vibecoder_android_host_omniroute_status_json", "vibecoder_android_host_omniroute_stop_json",
        "runtime_profile_round_trip_proven", "signed_manifest_sha256", "exited_pending_reap",
    ):
        require(ffi, token, "OmniRoute persistent FFI session")

    bridge_java = read("android/app/src/main/java/com/vibecoder/shell/NativeBridge.java")
    bridge_c = read("android/app/src/main/cpp/native_bridge.c")
    activity = read("android/app/src/main/java/com/vibecoder/shell/MainActivity.java")
    for token in ("nativeOmniRouteStart", "nativeOmniRouteStatus", "nativeOmniRouteStop"):
        require(bridge_java, token, "OmniRoute Java bridge")
    for token in (
        "vibecoder_android_host_omniroute_start_json", "vibecoder_android_host_omniroute_status_json",
        "vibecoder_android_host_omniroute_stop_json", "const size_t capacity = 1024u * 1024u",
    ):
        require(bridge_c, token, "OmniRoute JNI bridge")
    for token in ("vibecoder_omniroute_service_test", "manifest_sha256", "nativeOmniRouteStart"):
        require(activity, token, "OmniRoute diagnostic service wiring")
    service_regression = read("scripts/test_part34_3_service_tools.py")
    require(service_regression, "Part 34.3.4 service-tool regression PASSED", "OmniRoute service regression")

    provisioning = json.loads(read("config/android-payload-provisioning.json") or "{}")
    entries = provisioning.get("payloads", provisioning.get("components", []))
    omni_payload = next((x for x in entries if isinstance(x, dict) and x.get("component_id") == "omniroute"), {})
    if omni_payload.get("status") != "reviewed_source_verified_android_runtime_build_required":
        ERRORS.append("OmniRoute provisioning status must distinguish reviewed source from runtime bundle")
    if omni_payload.get("runtime_profile") != "config/omniroute-android-runtime-profile.json":
        ERRORS.append("OmniRoute provisioning runtime profile reference missing")

    doc = read("docs/PART34_3_OMNIROUTE_ANDROID_PACKAGING.md")
    for token in (
        "OMNIROUTE_BUILD_BACKEND_ONLY=1",
        "node:sqlite",
        "sql.js",
        "FTS5",
        "does **not** claim",
        "Part 34.3.2",
        "Mach-O payload",
        "Part 34.3.3",
        "files/vibecoder/runtime/omniroute",
        "previous verified runtime",
        "Part 34.3.4",
        "twice consecutively",
        "no automatic wall-clock timeout",
        "service_round_trip_proven=true",
    ):
        require(doc, token, "Part 34.3 documentation")

    state = json.loads(read("PART34_STATE.json") or "{}")
    omni = state.get("omniroute_packaging", {})
    for key in (
        "reviewed_archive_verified", "deterministic_patch_verified", "native_dependency_audit_completed",
        "android_runtime_profile_defined", "backend_bundle_builder_ready", "host_native_pruner_validated",
        "independent_bundle_verifier_ready", "bundle_tool_regression_passed",
        "apk_generated_asset_source_set_wired", "apk_asset_stager_ready", "app_private_installer_ready",
        "asset_manifest_identity_verified_on_device", "asset_file_sha256_verified_on_extract",
        "installed_tree_reverified_before_reuse", "path_traversal_rejected_on_device",
        "symlink_runtime_rejected_on_device", "install_lock_serializes_replacement",
        "atomic_stage_promote_with_previous_rollback", "stale_stage_cleanup_ready",
        "previous_runtime_recovery_ready", "apk_asset_verifier_mode_ready",
        "device_asset_acceptance_mode_ready", "asset_tool_regression_passed",
        "persistent_service_no_wallclock_timeout_ready", "trusted_runtime_working_directory_ready",
        "clean_bounded_runtime_environment_ready", "runtime_service_private_data_dir_ready",
        "rust_installed_tree_reverification_ready", "signed_apk_manifest_hash_bound_at_launch",
        "runtime_profile_readiness_probe_ready", "readiness_requires_consecutive_attestations",
        "loopback_only_launch_profile_ready", "explicit_service_stop_ready", "explicit_service_restart_ready",
        "persistent_ffi_session_ready", "jni_service_start_status_stop_ready",
        "device_service_acceptance_mode_ready", "device_explicit_stop_acceptance_ready",
        "service_tool_regression_passed",
    ):
        if omni.get(key) is not True:
            ERRORS.append(f"Part 34.3 state missing true marker: {key}")
    for key in ("runtime_bundle_built", "apk_asset_packaged", "service_round_trip_proven", "current_runner_build_preflight_passed", "fresh_rust_compile_for_34_3_4"):
        if omni.get(key) is not False:
            ERRORS.append(f"Part 34.3 proof/preflight must remain false until real evidence: {key}")
    if omni.get("required_node_version") != "24.19.0" or omni.get("current_runner_node_version") != "22.16.0":
        ERRORS.append("Part 34.3 current runner Node preflight record drifted")
    if omni.get("android_runtime_profile_sha256") != EXPECTED_ANDROID_PROFILE:
        ERRORS.append("Part 34.3 Android runtime profile hash drifted")
    if omni.get("automatic_service_restart") is not False:
        ERRORS.append("Part 34.3.4 automatic service restart must remain disabled")
    if state.get("blockers", {}).get("reviewed_omniroute_bundle_present") is not False:
        ERRORS.append("runtime bundle blocker must remain false; reviewed source archive is not a runtime bundle")

    if ERRORS:
        print(f"Part 34.3 source validation FAILED ({len(ERRORS)} problem(s))")
        for i, error in enumerate(ERRORS, 1): print(f"{i}. {error}")
        return 1
    print("Part 34.3 source validation PASSED")
    print("Scope: reviewed source + Android bundle/APK install + supervised loopback service source lane")
    return 0

if __name__ == "__main__":
    sys.exit(main())
