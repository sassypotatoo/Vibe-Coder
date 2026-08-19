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

core = read("crates/vibecoder-core/src/lib.rs")
workflow = read(".github/workflows/android-diagnostic-apk.yml")
doc = read("docs/PART34_6_CONVERSATION_MODEL_CONTROLLER.md")

for token in (
    "pub struct ConversationModelTurnOutcome",
    "pub async fn run_persisted_model_conversation_turn(",
    "pub async fn run_persisted_model_conversation_turn_resolved<R: SecretResolver>(",
    "MAX_CONVERSATION_MODEL_MESSAGES: usize = 64",
    "MAX_CONVERSATION_MODEL_MESSAGE_BYTES: usize = 128 * 1024",
    "MAX_CONVERSATION_MODEL_CONTEXT_BYTES: usize = 256 * 1024",
    "MAX_CONVERSATION_MODEL_OUTPUT_TOKENS: u32 = 8192",
):
    need(core, token, "controller_api_and_bounds")

for token in (
    "conversation.append_message(ConversationRole::User, prompt.to_owned())?;",
    "conversation.turn_pending = true;",
    "store.update_conversation(expected, &conversation).await?;",
    "self.gateway.execution_profile(gateway_credential).await?",
    "self.gateway.list_models(gateway_credential).await?",
    "select_exact_gateway_model(&catalog, requested_model_id)?",
    "build_gateway_conversation_context(&conversation)?",
    ".chat_completion(gateway_credential, &request)",
    "validate_conversation_model_response(&model, &response)?",
    "conversation.append_message(ConversationRole::Assistant, assistant_text.clone())?;",
    "conversation_model_turn_failure_cleanup_failed",
    "ensure_conversation_model_turn_capacity(&conversation, prompt)?",
    "MAX_CONVERSATION_MESSAGES.saturating_sub(2)",
    "checked_add(MAX_CONVERSATION_MESSAGE_BYTES)",
):
    need(core, token, "durable_single_model_turn")

# Ordering is the main authority boundary: durable user/pending commit -> fresh profile/catalog ->
# exactly one completion -> durable assistant completion.
user_commit = core.find("conversation.append_message(ConversationRole::User, prompt.to_owned())?;")
profile = core.find("self.gateway.execution_profile(gateway_credential).await?", user_commit)
catalog = core.find("self.gateway.list_models(gateway_credential).await?", profile)
inference = core.find(".chat_completion(gateway_credential, &request)", catalog)
assistant_commit = core.find("conversation.append_message(ConversationRole::Assistant, assistant_text.clone())?;", inference)
if min(user_commit, profile, catalog, inference, assistant_commit) < 0 or not (user_commit < profile < catalog < inference < assistant_commit):
    problems.append("controller_turn_authority_order_invalid")

method_start = core.find("pub async fn run_persisted_model_conversation_turn(")
method_end = core.find("pub async fn run_persisted_model_conversation_turn_resolved", method_start)
method = core[method_start:method_end]
for token in (
    "run_backend_task(",
    ".run_turn(",
    "RouteDecision::Fallback",
    "decision_after_failure",
    "loop {",
    "while ",
):
    forbid(method, token, "part34_6_no_agent_tools_retry_or_loop")
if method.count(".chat_completion(gateway_credential, &request)") != 1:
    problems.append("controller_inference_call_count_not_exactly_one")

for token in (
    "reversed.len() == MAX_CONVERSATION_MODEL_MESSAGES",
    "next_total > MAX_CONVERSATION_MODEL_CONTEXT_BYTES",
    "GatewayChatRole::Assistant",
    "reversed.remove(0)",
    "conversation_model_context_latest_user_missing",
):
    need(core, token, "bounded_recent_context")

for token in (
    "response.requested_model_id != model.id",
    "conversation_model_response_identity_mismatch",
    "response.text.len() > MAX_CONVERSATION_MESSAGE_BYTES",
):
    need(core, token, "response_persistence_gate")

for token in (
    '"assistant_text",',
    'format_args!("[REDACTED; {} byte(s)]", self.assistant_text.len())',
):
    need(core, token, "outcome_debug_redaction")

for token in (
    '- "crates/vibecoder-core/**"',
    "python3 scripts/validate_part34_6.py",
    "python3 scripts/test_part34_6_controller_tools.py",
):
    need(workflow, token, "ci_controller_coverage")

for token in (
    "One call to `run_persisted_model_conversation_turn` performs exactly one turn",
    "Jcode/tool execution remains",
    "real_android_conversation_turn_proven=false",
):
    need(doc, token, "part34_6_docs")

try:
    state = json.loads(read("PART34_STATE.json"))["controller_real_model"]
    expected = {
        "step": "34.6-controller-real-model",
        "status": "source_lane_complete_real_android_conversation_turn_pending",
        "durable_user_before_inference": True,
        "turn_pending_crash_marker": True,
        "fresh_runtime_profile": True,
        "fresh_catalog_exact_model": True,
        "context_messages_max": 64,
        "context_bytes_max": 262144,
        "message_bytes_max": 131072,
        "max_output_tokens": 8192,
        "exactly_one_inference_request": True,
        "automatic_retry": False,
        "alternate_model_fallback": False,
        "jcode_tools_invoked": False,
        "assistant_response_persisted": True,
        "failure_clears_pending_marker": True,
        "secret_reference_resolver_path": True,
        "real_android_conversation_turn_proven": False,
        "fresh_rust_compile": False,
    }
    for key, value in expected.items():
        if state.get(key) != value:
            problems.append(f"part34_state_mismatch:{key}:{state.get(key)!r}")
except Exception as exc:
    problems.append(f"part34_state_invalid:{exc}")

try:
    state = json.loads(read("PROJECT_STATE.json"))["part34_6_controller_real_model"]
    for key in (
        "controller_real_model_connected",
        "durable_prompt_before_network",
        "fresh_profile_and_catalog_before_inference",
        "bounded_recent_history_context",
        "exact_model_identity_rechecked",
        "exactly_one_gateway_completion",
        "assistant_response_durable",
        "failure_cleanup_fail_closed",
        "secret_reference_resolution_supported",
    ):
        if state.get(key) is not True:
            problems.append(f"project_state_missing_true:{key}")
    for key in (
        "automatic_retry_or_fallback",
        "jcode_tool_bridge_enabled",
        "streaming_controller_enabled",
        "real_android_conversation_turn_proven",
        "fresh_rust_compile_for_34_6",
    ):
        if state.get(key) is not False:
            problems.append(f"project_state_overclaim:{key}")
except Exception as exc:
    problems.append(f"project_state_invalid:{exc}")

if problems:
    print(f"Part 34.6 source validation FAILED ({len(problems)} problem(s))")
    for index, problem in enumerate(problems, 1):
        print(f"{index}. {problem}")
    sys.exit(1)
print("Part 34.6 source validation PASSED")
print("Scope: durable one-turn conversation -> exact OmniRoute model -> durable assistant; no Jcode tools")
