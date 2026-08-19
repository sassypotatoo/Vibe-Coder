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
config = read("crates/vibecoder-agent-jcode/src/config.rs")
turn = read("crates/vibecoder-agent-jcode/src/turn.rs")
runtime = read("crates/vibecoder-agent-jcode/src/runtime.rs")
checkpoint = read("crates/vibecoder-checkpoint-contract/src/lib.rs")
workflow = read(".github/workflows/android-diagnostic-apk.yml")
doc = read("docs/PART34_8_FIRST_AGENT_ACTION_TURN.md")

for token in (
    "pub struct PersistedAgentActionTurnOutcome",
    "pub async fn run_persisted_agent_action_turn(",
    "pub async fn run_persisted_agent_action_turn_resolved<R: SecretResolver>(",
    "const MAX_AGENT_ACTION_TOOL_CALLS: usize = 32",
    "const MAX_AGENT_ACTION_CALL_ID_BYTES: usize = 256",
    "const AGENT_ACTION_FILE_TOOLS: &[&str]",
    "const AGENT_ACTION_MUTATION_TOOLS: &[&str]",
):
    need(core, token, "agent_action_api")

for tool in ("read", "write", "edit", "multiedit", "apply_patch", "patch", "agentgrep", "ls"):
    need(core, f'"{tool}"', "core_file_tool_allowlist")
    need(config, f'"{tool}"', "jcode_file_tool_allowlist")

for token in (
    "VIBECODER_BRIDGED_FILE_TOOLS",
    "allowed_tools: Option<&'static [&'static str]>",
    "tool_policy_failure: Arc<AtomicBool>",
    "fn tool_policy_failure",
    "!allowed.iter().any(|candidate| *candidate == name.as_str())",
    "let _ = safety_client.cancel(&callback_session.0);",
):
    need(turn + config, token, "live_tool_policy")
need(runtime, "crate::VIBECODER_BRIDGED_FILE_TOOLS", "runtime_live_tool_policy")
need(runtime, "safety_state.tool_policy_failure()", "runtime_live_tool_policy")
need(runtime, "Jcode bridged tool policy failed closed for this turn", "runtime_live_tool_policy")

for token in (
    "if capabilities.command_tools",
    "agent_action_command_tools_must_remain_disabled",
    "model_gateway_bridge_identity().ok_or(",
    "agent_action_exact_model_bridge_required",
    "CheckpointReason::BeforeAgentChange",
    "conversation.append_message(ConversationRole::User, prompt.to_owned())?;",
    "conversation.turn_pending = true;",
    ".run_backend_task(",
    "state.observe(&event);",
    "validate_agent_action_turn(outcome.turn(), &state)",
    "CheckpointReason::AgentChangeVerification",
    "post_action_checkpoint.tree_sha256 == checkpoint.tree_sha256",
    "agent_action_workspace_unchanged",
    "completed.append_message(ConversationRole::Assistant, assistant_text)",
    "recover_failed_persisted_agent_action(",
    "rollback_project_checkpoint_for_pending_conversation(",
    "checkpoint_rollback_permitted_pending_conversation_missing",
    "conversation_agent_action_rollback_failed_pending_preserved",
    "cleared.turn_pending = false;",
):
    need(core, token, "bounded_durable_action_turn")

for token in (
    "agent_action_tool_event_protocol_invalid",
    "agent_action_tool_transcript_count_mismatch",
    "agent_action_tool_result_unobserved",
    "agent_action_tool_result_event_mismatch",
    "agent_action_no_successful_file_tool",
    "agent_action_no_successful_mutation",
    "AGENT_ACTION_MUTATION_TOOLS.contains(&call.tool.as_str())",
):
    need(core, token, "tool_transcript_acceptance")

need(checkpoint, "AgentChangeVerification", "post_action_checkpoint_reason")

start = core.find("pub async fn run_persisted_agent_action_turn(")
end = core.find("pub async fn run_persisted_agent_action_turn_resolved", start)
method = core[start:end]
if start < 0 or end < 0:
    problems.append("agent_action_method_slice_missing")
else:
    if method.count(".run_backend_task(") != 1:
        problems.append("agent_action_backend_task_count_not_exactly_one")
    for token in ("loop {", "while "):
        forbid(method, token, "no_outer_autonomous_loop")
    pre_checkpoint = method.find("CheckpointReason::BeforeAgentChange")
    user = method.find("append_message(ConversationRole::User")
    backend = method.find(".run_backend_task(")
    post_checkpoint = method.find("CheckpointReason::AgentChangeVerification")
    assistant = method.find("append_message(ConversationRole::Assistant")
    if min(pre_checkpoint, user, backend, post_checkpoint, assistant) < 0 or not (
        pre_checkpoint < user < backend < post_checkpoint < assistant
    ):
        problems.append("agent_action_authority_order_invalid")

for token in (
    "action_acceptance_requires_successful_mutation",
    "read_only_turn_is_not_an_action_acceptance_success",
    "transcript_mismatch_fails_closed",
):
    need(core, token, "rust_helper_tests")

for token in (
    'scripts/validate_part34_8.py',
    'scripts/test_part34_8_agent_action_tools.py',
    'docs/PART34_8_FIRST_AGENT_ACTION_TURN.md',
    'python3 scripts/validate_part34_8.py',
    'python3 scripts/test_part34_8_agent_action_tools.py',
):
    need(workflow, token, "ci_part34_8_coverage")

for token in (
    "Source lane complete",
    "one outer turn",
    "at least one successful mutation",
    "project-tree SHA-256",
    "rollback",
    "Part 34.9",
    "real Android model-driven workspace mutation",
):
    need(doc, token, "part34_8_docs")

try:
    state = json.loads(read("PART34_STATE.json"))["agent_action_turn"]
    expected = {
        "step": "34.8-first-agent-action-turn",
        "status": "source_lane_complete_real_android_agent_action_pending",
        "one_outer_turn": True,
        "durable_user_before_action": True,
        "pre_action_checkpoint": True,
        "post_action_tree_verification_checkpoint": True,
        "command_tools_enabled": False,
        "max_tool_calls_per_turn": 32,
        "live_unexpected_tool_cancel": True,
        "tool_transcript_required": True,
        "successful_file_tool_required": True,
        "successful_mutation_required": True,
        "workspace_tree_change_required": True,
        "assistant_response_persisted": True,
        "failure_rollback_to_pre_action_checkpoint": True,
        "rollback_failure_keeps_pending_marker_armed": True,
        "temporary_checkpoint_cleanup": True,
        "automatic_outer_loop": False,
        "real_android_agent_action_proven": False,
        "fresh_rust_compile": False,
    }
    for key, value in expected.items():
        if state.get(key) != value:
            problems.append(f"part34_state_mismatch:{key}:{state.get(key)!r}")
    if state.get("allowed_file_tools") != [
        "read", "write", "edit", "multiedit", "apply_patch", "patch", "agentgrep", "ls"
    ]:
        problems.append("part34_state_allowed_file_tools_mismatch")
except Exception as exc:
    problems.append(f"part34_state_invalid:{exc}")

try:
    state = json.loads(read("PROJECT_STATE.json"))["part34_8_first_agent_action_turn"]
    for key in (
        "durable_prompt_before_agent_activity",
        "immutable_pre_action_checkpoint",
        "live_file_tool_allowlist_enforced",
        "live_tool_transcript_corroboration",
        "successful_mutation_required",
        "workspace_tree_sha_change_required",
        "failure_rolls_back_project",
        "rollback_failure_keeps_pending_marker_armed",
        "assistant_response_durable",
    ):
        if state.get(key) is not True:
            problems.append(f"project_state_missing_true:{key}")
    if state.get("max_tool_calls_per_normal_turn") != 32:
        problems.append("project_state_tool_limit_mismatch")
    for key in (
        "command_tools_enabled",
        "automatic_outer_loop",
        "real_android_agent_action_proven",
        "fresh_rust_compile_for_34_8",
    ):
        if state.get(key) is not False:
            problems.append(f"project_state_overclaim:{key}")
except Exception as exc:
    problems.append(f"project_state_invalid:{exc}")

if problems:
    print(f"Part 34.8 source validation FAILED ({len(problems)} problem(s))")
    for index, problem in enumerate(problems, 1):
        print(f"{index}. {problem}")
    sys.exit(1)

print("Part 34.8 source validation PASSED")
print("Scope: one durable coding action -> exact bridged Jcode file tools -> real tree change -> durable answer -> stop")
