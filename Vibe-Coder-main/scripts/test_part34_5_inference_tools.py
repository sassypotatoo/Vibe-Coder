#!/usr/bin/env python3
"""Fail-closed source regression checks for the Part 34.5 first-model request lane."""
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
chat = (ROOT / "crates/vibecoder-gateway-omniroute/src/chat.rs").read_text()
inference = (ROOT / "crates/vibecoder-android-host/src/inference.rs").read_text()
ffi = (ROOT / "crates/vibecoder-android-host/src/omniroute_ffi.rs").read_text()
activity = (ROOT / "android/app/src/main/java/com/vibecoder/shell/MainActivity.java").read_text()

checks = {
    "non_streaming_wire_request": "stream: false" in chat and "max_tokens: request.max_output_tokens" in chat,
    "bounded_messages": "MAX_TOTAL_MESSAGE_BYTES: usize = 256 * 1024" in chat,
    "tool_calls_fail_closed": "inference_tool_call_not_allowed_part34_5" in chat,
    "stable_rate_limit": '429 => "inference_rate_limited"' in chat,
    "profile_catalog_inference_order": inference.find("execution_profile(credential)") < inference.find("client.list_models(credential)") < inference.find("client.chat_completion(credential, &request)"),
    "one_request_only": "inference_requests_count: 1" in inference and "automatic_retry_or_model_fallback: false" in inference,
    "diagnostic_text_not_persisted": "prompt_persisted: false" in inference and "response_text_persisted: false" in inference,
    "ffi_one_shot": "vibecoder_android_host_omniroute_inference_probe_json" in ffi and "output.is_null() || output_capacity == 0" in ffi,
    "android_fixed_prompt": "DIAGNOSTIC_INFERENCE_PROMPT" in activity and "nativeOmniRouteInferenceProbe" in activity,
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit("Part 34.5 inference-tool regression FAILED: " + ",".join(failed))

for forbidden in (
    "RouteDecision::Fallback",
    "decision_after_failure",
    "while ",
    "loop {",
):
    if forbidden in inference:
        raise SystemExit(f"Part 34.5 inference-tool regression FAILED: retry/fallback token present:{forbidden}")

# The serialized probe must never gain a raw prompt or assistant-text field.
probe_start = inference.index("pub(crate) struct OmniRouteInferenceProbe")
probe_end = inference.index("impl AndroidHostRuntime", probe_start)
probe = inference[probe_start:probe_end]
for forbidden_field in ("pub prompt:", "pub response_text:", "pub assistant_text:"):
    if forbidden_field in probe:
        raise SystemExit(f"Part 34.5 inference-tool regression FAILED: sensitive diagnostic field:{forbidden_field}")

print("Part 34.5 inference-tool regression PASSED")
