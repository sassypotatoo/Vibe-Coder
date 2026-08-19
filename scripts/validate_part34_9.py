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
doc = read("docs/PART34_9_EXPLICIT_AGENT_LOOP.md")

for token in (
    "pub struct ExplicitAgentLoopPolicy",
    "pub struct ExplicitAgentLoopCancellation",
    "pub struct ExplicitAgentLoopGuard",
    "pub enum ExplicitAgentLoopStopReason",
    "pub struct PersistedExplicitAgentLoopOutcome",
    "pub fn new_explicit_agent_loop(",
    "pub async fn run_persisted_explicit_agent_loop(",
    "pub async fn run_persisted_explicit_agent_loop_resolved<R: SecretResolver>(",
    "pub fn request_explicit_agent_loop_cancel(",
    "pub async fn cancel_active_explicit_agent_loop_turn(",
):
    need(core, token, "explicit_loop_api")

for token in (
    "DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TURNS: u8 = 4",
    "MAX_EXPLICIT_AGENT_LOOP_TURNS: u8 = 8",
    "DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TOTAL_TOOL_CALLS: usize = 96",
    "MAX_EXPLICIT_AGENT_LOOP_TOTAL_TOOL_CALLS: usize = 256",
    "DEFAULT_EXPLICIT_AGENT_LOOP_MAX_SAME_WORKSPACE_OCCURRENCES: u8 = 2",
    "MAX_EXPLICIT_AGENT_LOOP_SAME_WORKSPACE_OCCURRENCES: u8 = 3",
):
    need(core, token, "explicit_loop_bounds")

for token in (
    "VIBECODER_LOOP_STATUS=complete",
    "VIBECODER_LOOP_STATUS=continue",
    "fn build_explicit_agent_loop_iteration_prompt(",
    "fn parse_explicit_agent_loop_response(",
    "explicit_agent_loop_status_marker_missing",
    "explicit_agent_loop_response_body_invalid",
):
    need(core, token, "explicit_loop_protocol")

start = core.find("pub async fn run_persisted_explicit_agent_loop(")
end = core.find("pub async fn run_persisted_explicit_agent_loop_resolved", start)
method = core[start:end]
if start < 0 or end < 0:
    problems.append("explicit_loop_method_slice_missing")
else:
    for token in (
        "for iteration in 1..=guard.policy.max_turns",
        "conversation.append_message(ConversationRole::User, prompt.to_owned())?;",
        "conversation.turn_pending = true;",
        "CheckpointReason::BeforeAgentChange",
        "CheckpointReason::AgentChangeVerification",
        "validate_agent_loop_iteration_turn(outcome.turn(), &state)",
        "successful_mutations == 0 || current_tree_sha256 == previous_tree_sha256",
        "workspace_occurrences",
        "ExplicitAgentLoopStopReason::RepeatedWorkspaceState",
        "ExplicitAgentLoopStopReason::TurnBudgetExhausted",
        "ExplicitAgentLoopStopReason::ToolBudgetExhausted",
        "ExplicitAgentLoopStopReason::Cancelled",
        "finish_persisted_explicit_agent_loop_non_success(",
        "recover_failed_persisted_explicit_agent_loop(",
    ):
        need(method, token, "explicit_loop_runtime")
    if method.count("append_message(ConversationRole::User") != 1:
        problems.append("explicit_loop_user_message_not_exactly_once")
    if method.count("append_message(ConversationRole::Assistant") != 1:
        problems.append("explicit_loop_intermediate_assistant_persistence_detected")
    if method.count("if guard.cancellation.is_requested()") < 3:
        problems.append("explicit_loop_cancellation_not_rechecked_at_commit_boundaries")
    need(method, "explicit_agent_loop_command_tools_must_remain_disabled", "file_only_boundary")
    for token in ("auto_poke", "autoreview", "autojudge"):
        forbid(method.lower(), token, "hidden_autonomy")

# 34.8 recovery regression repair must remain private and scope-bound.
for token in (
    "async fn rollback_project_checkpoint_for_pending_conversation(",
    "async fn rollback_project_checkpoint_internal(",
    "checkpoint_rollback_permitted_pending_conversation_missing",
    "checkpoint_rollback_pending_conversation_store_missing",
    "permitted_pending_seen",
):
    need(core, token, "pending_turn_rollback_repair")
public_rb_start = core.find("pub async fn rollback_project_checkpoint(")
private_rb_start = core.find("async fn rollback_project_checkpoint_for_pending_conversation(", public_rb_start)
if public_rb_start < 0 or private_rb_start < 0:
    problems.append("rollback_wrapper_slice_missing")
else:
    public_wrapper = core[public_rb_start:private_rb_start]
    need(public_wrapper, "rollback_project_checkpoint_internal(project, checkpoint_id, None)", "public_rollback_strict")

for token in (
    "explicit_loop_policy_is_bounded",
    "explicit_loop_response_requires_one_terminal_marker",
    "explicit_loop_iteration_prompt_is_machine_bounded",
    "explicit_loop_guard_is_one_shot_and_cancellable",
):
    need(core, token, "rust_helper_tests")

for token in (
    "Source lane complete",
    "Normal VibeCoder behavior remains single-shot",
    "4 outer turns",
    "96 total observed Jcode tool calls",
    "pre-loop checkpoint",
    "whole workspace",
    "bash",
    "browser",
    "MCP",
    "external compile/test verifier",
    "Part 34.10",
):
    need(doc, token, "part34_9_docs")

for token in (
    "scripts/validate_part34_9.py",
    "scripts/test_part34_9_loop_tools.py",
    "docs/PART34_9_EXPLICIT_AGENT_LOOP.md",
    "python3 scripts/validate_part34_9.py",
    "python3 scripts/test_part34_9_loop_tools.py",
):
    need(workflow, token, "ci_part34_9_coverage")

try:
    state = json.loads(read("PART34_STATE.json"))["explicit_agent_loop"]
    expected = {
        "step": "34.9-explicit-agent-loop",
        "status": "source_lane_complete_real_android_explicit_loop_pending",
        "explicit_opt_in_required": True,
        "normal_single_turn_default_unchanged": True,
        "one_shot_guard": True,
        "default_max_turns": 4,
        "hard_max_turns": 8,
        "default_max_total_tool_calls": 96,
        "hard_max_total_tool_calls": 256,
        "default_same_workspace_occurrence_stop": 2,
        "one_durable_user_message": True,
        "intermediate_assistant_messages_persisted": False,
        "machine_terminal_marker_required": True,
        "continue_requires_successful_mutation": True,
        "continue_requires_workspace_tree_change": True,
        "complete_requires_successful_file_tool": True,
        "pre_loop_checkpoint": True,
        "non_success_rolls_back_whole_loop": True,
        "rollback_failure_keeps_pending_marker_armed": True,
        "repeated_workspace_state_detection": True,
        "between_turn_cancellation": True,
        "active_turn_cancellation": True,
        "command_tools_enabled": False,
        "browser_tools_enabled": False,
        "mcp_tools_enabled": False,
        "external_test_verifier_enabled": False,
        "real_android_explicit_loop_proven": False,
        "fresh_rust_compile": False,
    }
    for key, value in expected.items():
        if state.get(key) != value:
            problems.append(f"part34_state_mismatch:{key}:{state.get(key)!r}")
    if json.loads(read("PART34_STATE.json"))["agent_action_turn"].get("pending_turn_recovery_rollback_scope_bound") is not True:
        problems.append("part34_8_recovery_fix_not_recorded")
except Exception as exc:
    problems.append(f"part34_state_invalid:{exc}")

try:
    state = json.loads(read("PROJECT_STATE.json"))["part34_9_explicit_agent_loop"]
    for key in (
        "explicit_user_opt_in_required",
        "normal_action_turn_still_single_shot",
        "one_shot_scope_bound_guard",
        "repeated_workspace_state_detection",
        "active_and_between_turn_cancellation",
        "single_durable_user_message_across_loop",
        "intermediate_assistant_messages_not_persisted",
        "machine_completion_marker_required",
        "continue_requires_mutation_and_tree_change",
        "pre_loop_immutable_checkpoint",
        "non_success_restores_pre_loop_tree",
        "rollback_failure_keeps_pending_marker_armed",
        "completion_evidence_file_tools_only",
    ):
        if state.get(key) is not True:
            problems.append(f"project_state_missing_true:{key}")
    for key in (
        "command_browser_mcp_tools_enabled",
        "external_test_verifier_enabled",
        "real_android_explicit_loop_proven",
        "fresh_rust_compile_for_34_9",
    ):
        if state.get(key) is not False:
            problems.append(f"project_state_overclaim:{key}")
except Exception as exc:
    problems.append(f"project_state_invalid:{exc}")

if problems:
    print(f"Part 34.9 source validation FAILED ({len(problems)} problem(s))")
    for i, p in enumerate(problems, 1):
        print(f"{i}. {p}")
    sys.exit(1)

print("Part 34.9 source validation PASSED")
print("Scope: explicit opt-in -> bounded file-tool turns -> progress/repeat checks -> complete or atomic rollback -> stop")
