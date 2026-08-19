#!/usr/bin/env python3
"""Fail-closed source regressions for Part 34.9 explicit bounded agent loops."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
core = (ROOT / "crates/vibecoder-core/src/lib.rs").read_text()

start = core.index("pub async fn run_persisted_explicit_agent_loop(")
end = core.index("pub async fn run_persisted_explicit_agent_loop_resolved", start)
method = core[start:end]

checks = {
    "separate_explicit_api": "pub fn new_explicit_agent_loop(" in core,
    "one_shot_guard": "explicit_agent_loop_guard_already_used" in core and "compare_exchange(false, true" in core,
    "bounded_for_loop": "for iteration in 1..=guard.policy.max_turns" in method,
    "hard_turn_cap": "MAX_EXPLICIT_AGENT_LOOP_TURNS: u8 = 8" in core,
    "default_turn_cap": "DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TURNS: u8 = 4" in core,
    "hard_tool_cap": "MAX_EXPLICIT_AGENT_LOOP_TOTAL_TOOL_CALLS: usize = 256" in core,
    "default_tool_cap": "DEFAULT_EXPLICIT_AGENT_LOOP_MAX_TOTAL_TOOL_CALLS: usize = 96" in core,
    "single_user_persist": method.count("append_message(ConversationRole::User") == 1,
    "pending_held": "conversation.turn_pending = true;" in method,
    "intermediate_assistant_not_persisted": method.count("append_message(ConversationRole::Assistant") == 1,
    "terminal_markers": "EXPLICIT_AGENT_LOOP_COMPLETE_MARKER" in core and "EXPLICIT_AGENT_LOOP_CONTINUE_MARKER" in core,
    "marker_parser": "fn parse_explicit_agent_loop_response(" in core,
    "continue_requires_mutation": "successful_mutations == 0 || current_tree_sha256 == previous_tree_sha256" in method,
    "tree_repeat_detection": "workspace_occurrences" in method and "max_same_workspace_occurrences" in method,
    "baseline_checkpoint": "CheckpointReason::BeforeAgentChange" in method,
    "verification_checkpoint": "CheckpointReason::AgentChangeVerification" in method,
    "whole_loop_rollback": "finish_persisted_explicit_agent_loop_non_success(" in method,
    "error_rollback": "recover_failed_persisted_explicit_agent_loop(" in method,
    "cancellation_rechecked_at_boundaries": method.count("if guard.cancellation.is_requested()") >= 3,
    "active_cancel_api": "pub async fn cancel_active_explicit_agent_loop_turn(" in core,
    "active_cancel_calls_agent": "self.agent.cancel(&session.session_id).await" in core,
    "command_tools_fail_closed": "explicit_agent_loop_command_tools_must_remain_disabled" in method,
    "no_hidden_auto_controller": "auto_poke" not in method.lower() and "autojudge" not in method.lower() and "autoreview" not in method.lower(),
    "private_pending_rollback": "async fn rollback_project_checkpoint_for_pending_conversation(" in core,
    "public_rollback_still_strict": "checkpoint_rollback_conversation_turn_pending" in core,
    "pending_rollback_exact_scope": "checkpoint_rollback_permitted_pending_conversation_missing" in core,
    "rollback_before_clear": (
        lambda recovery: recovery.index("rollback_project_checkpoint_for_pending_conversation(")
        < recovery.index("cleared.turn_pending = false;")
    )(core[core.index("async fn recover_failed_persisted_explicit_agent_loop("):]),
}

failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit("Part 34.9 explicit-loop regression FAILED: " + ",".join(failed))

# Normal Part 34.8 action must remain single-shot.
action_start = core.index("pub async fn run_persisted_agent_action_turn(")
action_end = core.index("pub async fn run_persisted_agent_action_turn_resolved", action_start)
action = core[action_start:action_end]
if "for iteration in" in action or "while " in action or "loop {" in action:
    raise SystemExit("Part 34.9 explicit-loop regression FAILED: normal action became autonomous")
if action.count(".run_backend_task(") != 1:
    raise SystemExit("Part 34.9 explicit-loop regression FAILED: normal action backend count drifted")

# File-only boundary remains explicit in this stage.
for forbidden in ('"bash"', '"browser"', '"mcp"'):
    allowlist = core[core.index("const AGENT_ACTION_FILE_TOOLS"):core.index("const AGENT_ACTION_MUTATION_TOOLS")]
    if forbidden in allowlist:
        raise SystemExit(f"Part 34.9 explicit-loop regression FAILED: forbidden tool entered allowlist:{forbidden}")

print("Part 34.9 explicit-loop regression PASSED")
