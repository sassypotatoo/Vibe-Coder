#!/usr/bin/env python3
"""Fail-closed source regressions for the Part 34.8 first agent action turn."""
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
core = (ROOT / "crates/vibecoder-core/src/lib.rs").read_text()
config = (ROOT / "crates/vibecoder-agent-jcode/src/config.rs").read_text()
turn = (ROOT / "crates/vibecoder-agent-jcode/src/turn.rs").read_text()
runtime = (ROOT / "crates/vibecoder-agent-jcode/src/runtime.rs").read_text()
checkpoint = (ROOT / "crates/vibecoder-checkpoint-contract/src/lib.rs").read_text()

start = core.index("pub async fn run_persisted_agent_action_turn(")
end = core.index("pub async fn run_persisted_agent_action_turn_resolved", start)
method = core[start:end]

checks = {
    "one_backend_task": method.count(".run_backend_task(") == 1,
    "no_outer_loop": "loop {" not in method and "while " not in method,
    "command_tools_fail_closed": "if capabilities.command_tools" in method,
    "bridge_required": "model_gateway_bridge_identity().ok_or(" in method,
    "pre_checkpoint": "CheckpointReason::BeforeAgentChange" in method,
    "durable_pending_before_action": method.index("append_message(ConversationRole::User") < method.index(".run_backend_task("),
    "tool_observer": "state.observe(&event);" in method,
    "tool_validation": "validate_agent_action_turn(outcome.turn(), &state)" in method,
    "post_tree_checkpoint": "CheckpointReason::AgentChangeVerification" in method,
    "tree_must_change": "post_action_checkpoint.tree_sha256 == checkpoint.tree_sha256" in method,
    "assistant_after_tree_proof": method.index("CheckpointReason::AgentChangeVerification") < method.index("append_message(ConversationRole::Assistant"),
    "failure_recovery": method.count("recover_failed_persisted_agent_action(") >= 4,
    "resolved_secret_path": "pub async fn run_persisted_agent_action_turn_resolved<R: SecretResolver>" in core,
    "live_tool_gate": "allowed_tools: Option<&'static [&'static str]>" in turn and "tool_policy_failure" in turn,
    "live_tool_cancel": "!allowed.iter().any(|candidate| *candidate == name.as_str())" in turn and "safety_client.cancel(&callback_session.0)" in turn,
    "runtime_allowlist_wired": "crate::VIBECODER_BRIDGED_FILE_TOOLS" in runtime,
    "runtime_policy_failure_checked": "safety_state.tool_policy_failure()" in runtime,
    "verification_checkpoint_reason": "AgentChangeVerification" in checkpoint,
    "rollback_keeps_pending_until_success": (
        lambda recovery: recovery.index("rollback_project_checkpoint_for_pending_conversation(")
        < recovery.index("cleared.turn_pending = false;")
    )(core[core.index("async fn recover_failed_persisted_agent_action("):]),
    "pending_recovery_is_scope_bound": "checkpoint_rollback_permitted_pending_conversation_missing" in core,
    "mutation_required": "agent_action_no_successful_mutation" in core,
    "tree_change_required": "agent_action_workspace_unchanged" in core,
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit("Part 34.8 agent-action regression FAILED: " + ",".join(failed))

expected_tools = ["read", "write", "edit", "multiedit", "apply_patch", "patch", "agentgrep", "ls"]
for tool in expected_tools:
    if f'"{tool}"' not in config or f'"{tool}"' not in core:
        raise SystemExit(f"Part 34.8 agent-action regression FAILED: missing file tool:{tool}")
if '"bash"' in core[core.index("const AGENT_ACTION_FILE_TOOLS"):core.index("const AGENT_ACTION_MUTATION_TOOLS")]:
    raise SystemExit("Part 34.8 agent-action regression FAILED: bash entered action allowlist")
if "command_tools: true" in runtime:
    raise SystemExit("Part 34.8 agent-action regression FAILED: command capability overclaim")
if "MAX_AGENT_ACTION_TOOL_CALLS: usize = 32" not in core or "VIBECODER_BRIDGED_MAX_TOOL_CALLS_PER_TURN: u32 = 32" not in config:
    raise SystemExit("Part 34.8 agent-action regression FAILED: tool limits drifted")

print("Part 34.8 agent-action regression PASSED")
