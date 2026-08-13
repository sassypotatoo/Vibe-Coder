#!/usr/bin/env python3
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
problems = []

def read(rel):
    p = ROOT / rel
    if not p.is_file():
        problems.append(f"missing:{rel}")
        return ""
    return p.read_text(encoding="utf-8")

def need(text, token, label):
    if token not in text:
        problems.append(f"missing_contract:{label}:{token}")

def forbid(text, token, label):
    if token in text:
        problems.append(f"forbidden_contract:{label}:{token}")

manifest = read("android/app/src/main/AndroidManifest.xml")
bridge_java = read("android/app/src/main/java/com/vibecoder/shell/NativeBridge.java")
activity = read("android/app/src/main/java/com/vibecoder/shell/MainActivity.java")
bridge_c = read("android/app/src/main/cpp/native_bridge.c")
host_lib = read("crates/vibecoder-android-host/src/lib.rs")
transport = read("crates/vibecoder-android-host/src/gateway_transport.rs")
service = read("crates/vibecoder-android-host/src/omniroute_service.rs")
ffi = read("crates/vibecoder-android-host/src/omniroute_ffi.rs")
client = read("crates/vibecoder-gateway-omniroute/src/client.rs")
device = read("scripts/test_android_diagnostic_device.sh")
apk = read("scripts/verify_android_diagnostic_apk.sh")
doc = read("docs/PART34_4_LOCAL_GATEWAY_TRANSPORT.md")

need(manifest, 'android:name="android.permission.INTERNET"', "android_socket_permission")
if manifest.count('android:name="android.permission.INTERNET"') != 1:
    problems.append("internet_permission_must_appear_exactly_once")
for token in ("android:usesCleartextTraffic", "android:networkSecurityConfig"):
    forbid(manifest, token, "no_global_cleartext_relaxation")

need(host_lib, "mod gateway_transport;", "host_gateway_transport_module")
need(service, 'pub(crate) const LOOPBACK_BASE_URL: &str = "http://127.0.0.1:20128/v1"', "fixed_loopback_url")
for token in (
    'LOOPBACK_BASE_URL',
    'probe_omniroute_gateway_transport',
    'GatewayCredential',
    'execution_profile(credential)',
    'client.health(credential)',
    'credential_persisted: false',
    'inference_request_sent: false',
    'first_model_request_proven: false',
    'service_attested',
    'catalog_round_trip_reached',
    'local_transport_round_trip_proven',
):
    need(transport, token, "gateway_transport")
for token in ('chat/completions', '/responses', 'send_inference', 'completion_request'):
    forbid(transport, token, "part34_4_no_inference")

for token in (
    '.redirect(reqwest::redirect::Policy::none())',
    '.no_proxy()',
    'max_response_bytes',
):
    need(client, token, "hardened_gateway_client")

for token in (
    'MAX_GATEWAY_CREDENTIAL_BYTES: usize = 8192',
    'vibecoder_android_host_omniroute_gateway_probe_json',
    'output.is_null() || output_capacity == 0',
    'GatewayCredential::Anonymous',
    'GatewayCredential::Secret(value)',
):
    need(ffi, token, "gateway_probe_ffi")
for token in ('println!', 'eprintln!', 'dbg!'):
    forbid(ffi, token, "ffi_credential_not_logged")

need(bridge_java, 'nativeOmniRouteGatewayProbe(byte[] credentialUtf8)', "java_gateway_probe")
for token in (
    'vibecoder_android_host_omniroute_gateway_probe_json',
    'credential_len > 8192',
    'Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteGatewayProbe',
):
    need(bridge_c, token, "jni_gateway_probe")

for token in (
    'vibecoder_omniroute_gateway_test',
    'nativeOmniRouteGatewayProbe(new byte[0])',
    'omniroute_gateway_transport',
):
    need(activity, token, "android_gateway_diagnostic")
for token in ('Bearer ', 'Authorization', 'api_key'):
    forbid(activity, token, "diagnostic_no_secret_persistence")

for text, label in ((device, "device_gateway_mode"), (apk, "apk_gateway_mode")):
    need(text, 'omniroute_gateway', label)
need(device, 'catalog_round_trip_reached', "device_catalog_transport_proof")
need(device, 'first_model_request_proven', "device_no_inference_overclaim")

need(doc, 'Part 34.5 owns the first real model request.', "next_step_boundary")

try:
    p34 = json.loads(read("PART34_STATE.json"))
    gt = p34["gateway_transport"]
    expected = {
        "step": "34.4-local-model-gateway-transport",
        "status": "source_lane_complete_physical_catalog_round_trip_pending",
        "fixed_loopback_base_url": "http://127.0.0.1:20128/v1",
        "android_internet_permission_declared": True,
        "requires_active_attested_service": True,
        "runtime_profile_rechecked": True,
        "credential_ephemeral_only": True,
        "anonymous_mode_supported": True,
        "ephemeral_bearer_mode_supported": True,
        "jni_gateway_probe_ready": True,
        "device_gateway_acceptance_mode_ready": True,
        "physical_catalog_round_trip_proven": False,
        "first_model_request_proven": False,
        "android_secure_store_implemented": False,
    }
    for key, value in expected.items():
        if gt.get(key) != value:
            problems.append(f"part34_state_mismatch:{key}:{gt.get(key)!r}")
except Exception as exc:
    problems.append(f"part34_state_invalid:{exc}")

try:
    project = json.loads(read("PROJECT_STATE.json"))
    gt = project["part34_4_local_gateway_transport"]
    if gt.get("source_transport_bridge_ready") is not True:
        problems.append("project_state_gateway_bridge_not_ready")
    for key in ("physical_catalog_round_trip_proven", "first_model_request_proven", "android_secure_store_implemented"):
        if gt.get(key) is not False:
            problems.append(f"project_state_overclaim:{key}")
except Exception as exc:
    problems.append(f"project_state_invalid:{exc}")

if problems:
    print(f"Part 34.4 source validation FAILED ({len(problems)} problem(s))")
    for i, problem in enumerate(problems, 1):
        print(f"{i}. {problem}")
    sys.exit(1)
print("Part 34.4 source validation PASSED")
print("Scope: attested Android-local OmniRoute profile/catalog transport; no inference or device proof claimed")
