#!/usr/bin/env python3
"""Fail-closed source regression checks for Part 34.6 durable model conversation control."""
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
core = (ROOT / "crates/vibecoder-core/src/lib.rs").read_text()

start = core.index("pub async fn run_persisted_model_conversation_turn(")
end = core.index("pub async fn run_persisted_model_conversation_turn_resolved", start)
method = core[start:end]

checks = {
    "user_persisted_before_inference": method.index("append_message(ConversationRole::User") < method.index("execution_profile(gateway_credential)"),
    "profile_before_catalog": method.index("execution_profile(gateway_credential)") < method.index("list_models(gateway_credential)"),
    "catalog_before_completion": method.index("list_models(gateway_credential)") < method.index("chat_completion(gateway_credential, &request)"),
    "one_completion": method.count("chat_completion(gateway_credential, &request)") == 1,
    "assistant_persisted": "append_message(ConversationRole::Assistant, assistant_text.clone())" in method,
    "failure_cleanup": "conversation_model_turn_failure_cleanup_failed" in method,
    "no_jcode_turn": ".run_turn(" not in method and "run_backend_task(" not in method,
    "no_loop": "loop {" not in method and "while " not in method,
    "no_fallback": "decision_after_failure" not in method and "RouteDecision::Fallback" not in method,
    "context_suffix_bounded": "MAX_CONVERSATION_MODEL_CONTEXT_BYTES" in core and "reversed.remove(0)" in core,
    "response_persistable": "response.text.len() > MAX_CONVERSATION_MESSAGE_BYTES" in core,
    "persistence_capacity_reserved": "ensure_conversation_model_turn_capacity(&conversation, prompt)?" in method and "MAX_CONVERSATION_MESSAGES.saturating_sub(2)" in core,
    "debug_redacted": '[REDACTED; {} byte(s)]' in core,
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit("Part 34.6 controller-tool regression FAILED: " + ",".join(failed))

# Model-only path must not mutate the persisted Jcode model preference. Part 34.7 owns bridging.
for forbidden in ("preferred_model =", "set_session_model("):
    if forbidden in method:
        raise SystemExit(f"Part 34.6 controller-tool regression FAILED: Jcode preference mutation:{forbidden}")

print("Part 34.6 controller-tool regression PASSED")
