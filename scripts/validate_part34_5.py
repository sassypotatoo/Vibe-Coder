#!/usr/bin/env python3
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
problems = []

def read(rel):
    path = ROOT / rel
    if not path.is_file():
        problems.append(f"missing:{rel}")
        return ""
    return path.read_text(encoding="utf-8")

def need(text, token, label):
    if token not in text:
        problems.append(f"missing_contract:{label}:{token}")

def forbid(text, token, label):
    if token in text:
        problems.append(f"forbidden_contract:{label}:{token}")

contract = read("crates/vibecoder-gateway-contract/src/lib.rs")
client = read("crates/vibecoder-gateway-omniroute/src/client.rs")
chat = read("crates/vibecoder-gateway-omniroute/src/chat.rs")
gateway = read("crates/vibecoder-gateway-omniroute/src/gateway.rs")
inference = read("crates/vibecoder-android-host/src/inference.rs")
ffi = read("crates/vibecoder-android-host/src/omniroute_ffi.rs")
bridge_c = read("android/app/src/main/cpp/native_bridge.c")
bridge_java = read("android/app/src/main/java/com/vibecoder/shell/NativeBridge.java")
activity = read("android/app/src/main/java/com/vibecoder/shell/MainActivity.java")
device = read("scripts/test_android_diagnostic_device.sh")
apk = read("scripts/verify_android_diagnostic_apk.sh")
doc = read("docs/PART34_5_FIRST_MODEL_REQUEST.md")

for token in (
    "pub struct GatewayChatRequest",
    "pub struct GatewayChatResponse",
    "pub enum GatewayChatRole",
    "async fn chat_completion(",
):
    need(contract, token, "provider_neutral_inference_contract")

for token in (
    'ChatCompletions => "chat/completions"',
    "post_chat_completion_raw",
    '.header("Content-Type", "application/json")',
    "MAX_REQUEST_BYTES: usize = 512 * 1024",
    ".redirect(reqwest::redirect::Policy::none())",
    ".no_proxy()",
):
    need(client, token, "hardened_chat_transport")

for token in (
    "MAX_MESSAGES: usize = 64",
    "MAX_TOTAL_MESSAGE_BYTES: usize = 256 * 1024",
    "MAX_OUTPUT_TOKENS: u32 = 8192",
    "stream: false",
    "max_tokens: request.max_output_tokens",
    "inference_tool_call_not_allowed_part34_5",
    "inference_rate_limited",
    "TokenUsage",
):
    need(chat, token, "chat_parser")
for token in ("tools:", "tool_choice:", "stream: true"):
    forbid(chat, token, "part34_5_no_tools_or_streaming")
need(gateway, "execute_chat_completion(self, credential, request).await", "gateway_adapter_inference")

for token in (
    "probe_omniroute_first_inference",
    "execution_profile(credential)",
    "client.list_models(credential)",
    "find(|model| model.id == requested_model_id)",
    "client.chat_completion(credential, &request)",
    "inference_requests_count: 1",
    "automatic_retry_or_model_fallback: false",
    "prompt_persisted: false",
    "response_text_persisted: false",
    "first_model_request_proven: active_after && response_nonempty",
):
    need(inference, token, "android_first_inference")
for token in ("decision_after_failure", "RouteDecision::Fallback", "loop {", "while "):
    forbid(inference, token, "no_vibecoder_inference_retry_loop")
if not (inference.find("execution_profile(credential)") < inference.find("client.list_models(credential)") < inference.find("client.chat_completion(credential, &request)")):
    problems.append("inference_gate_order_invalid")

for token in (
    "MAX_INFERENCE_MODEL_BYTES: usize = 512",
    "MAX_INFERENCE_PROMPT_BYTES: usize = 64 * 1024",
    "vibecoder_android_host_omniroute_inference_probe_json",
    "output.is_null() || output_capacity == 0",
    "inference_probe_bytes",
):
    need(ffi, token, "one_shot_inference_ffi")
for token in ("println!", "eprintln!", "dbg!"):
    forbid(ffi, token, "ffi_no_prompt_or_secret_logging")

for token in (
    "omniroute_inference_probe_fn",
    "vibecoder_android_host_omniroute_inference_probe_json",
    "Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteInferenceProbe",
    "prompt_len > (64 * 1024)",
):
    need(bridge_c, token, "jni_inference_bridge")
need(bridge_java, "nativeOmniRouteInferenceProbe(", "java_inference_bridge")

for token in (
    '"vibecoder_omniroute_inference_test"',
    '"vibecoder_omniroute_model"',
    "NativeBridge.nativeOmniRouteInferenceProbe(",
    'state.put("inference", inference)',
    'report.put("omniroute_inference", omniRouteInferenceState)',
):
    need(activity, token, "android_inference_diagnostic")
for token in ("Authorization", "Bearer ", "response_text\""):
    forbid(activity, token, "diagnostic_no_credential_or_response_persistence")

for text, label in ((device, "device_inference_mode"), (apk, "apk_inference_mode")):
    need(text, "omniroute_inference", label)
need(device, "OMNIROUTE_TEST_MODEL_ID_required_for_omniroute_inference", "device_model_input")
need(device, "omniroute_inference_exactly_one_request_not_proven", "device_one_request_acceptance")
need(device, "omniroute_first_model_request_not_proven", "device_first_model_acceptance")

for token in (
    "one `POST /v1/chat/completions` request",
    "No real model response has been produced in this runner.",
    "No streaming response path is claimed yet.",
):
    need(doc, token, "part34_5_docs")

try:
    state = json.loads(read("PART34_STATE.json"))["first_model_request"]
    expected = {
        "step": "34.5-first-exact-model-request",
        "status": "source_lane_complete_real_android_model_response_pending",
        "endpoint": "/v1/chat/completions",
        "stream": False,
        "exact_model_catalog_precheck": True,
        "runtime_profile_rechecked": True,
        "max_output_tokens": 256,
        "vibecoder_inference_retry_count": 0,
        "alternate_model_fallback": False,
        "tool_calls_allowed": False,
        "prompt_persisted": False,
        "response_text_persisted": False,
        "jni_inference_probe_ready": True,
        "device_inference_acceptance_mode_ready": True,
        "first_model_request_proven": False,
        "fresh_rust_compile": False,
    }
    for key, value in expected.items():
        if state.get(key) != value:
            problems.append(f"part34_state_mismatch:{key}:{state.get(key)!r}")
except Exception as exc:
    problems.append(f"part34_state_invalid:{exc}")

try:
    state = json.loads(read("PROJECT_STATE.json"))["part34_5_first_model_request"]
    for key in (
        "provider_neutral_chat_contract_ready",
        "chat_completions_transport_ready",
        "requires_active_attested_service",
        "runtime_profile_rechecked_before_inference",
        "fresh_catalog_exact_model_required",
        "exactly_one_inference_request",
        "token_usage_parser_ready",
        "jni_inference_probe_ready",
        "device_inference_acceptance_mode_ready",
    ):
        if state.get(key) is not True:
            problems.append(f"project_state_missing_true:{key}")
    for key in (
        "streaming_inference_ready",
        "automatic_retry_or_model_fallback",
        "tool_calls_allowed",
        "prompt_persisted_in_diagnostic",
        "response_text_persisted_in_diagnostic",
        "first_model_request_proven",
        "fresh_rust_compile_for_34_5",
    ):
        if state.get(key) is not False:
            problems.append(f"project_state_overclaim:{key}")
    if not isinstance(state.get("controller_real_model_connected"), bool):
        problems.append("project_state_invalid_bool:controller_real_model_connected")
except Exception as exc:
    problems.append(f"project_state_invalid:{exc}")

if problems:
    print(f"Part 34.5 source validation FAILED ({len(problems)} problem(s))")
    for index, problem in enumerate(problems, 1):
        print(f"{index}. {problem}")
    sys.exit(1)
print("Part 34.5 source validation PASSED")
print("Scope: exactly-one non-streaming exact-model request lane; real Android response not claimed")
