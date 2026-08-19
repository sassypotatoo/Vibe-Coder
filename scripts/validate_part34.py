#!/usr/bin/env python3
"""Source-contract validator for the VibeCoder Part 34 conversation slice."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ERRORS: list[str] = []


def text(path: str) -> str:
    target = ROOT / path
    if not target.is_file():
        ERRORS.append(f"missing {path}")
        return ""
    return target.read_text(encoding="utf-8", errors="replace")


def require(haystack: str, token: str, label: str) -> None:
    if token not in haystack:
        ERRORS.append(f"{label} missing: {token}")


def forbid(haystack: str, pattern: str, label: str) -> None:
    if re.search(pattern, haystack, re.IGNORECASE | re.MULTILINE):
        ERRORS.append(f"{label} forbidden pattern present: {pattern}")


def main() -> int:
    domain = text("crates/vibecoder-domain/src/lib.rs")
    contract = text("crates/vibecoder-persistence-contract/src/lib.rs")
    local = text("crates/vibecoder-persistence-local/src/lib.rs")
    unix = text("crates/vibecoder-persistence-local/src/unix_store.rs")
    core = text("crates/vibecoder-core/src/lib.rs")
    doc = text("docs/PART34_ALPHA_INTERACTION.md")
    ledger = text("docs/PROGRESS_LEDGER.md")
    process_local = text("crates/vibecoder-process-local/src/lib.rs")
    android_host = text("crates/vibecoder-android-host/src/lib.rs")
    device_test = text("scripts/test_android_diagnostic_device.sh")

    for token in ("pub struct ConversationId", "impl ConversationId"):
        require(domain, token, "domain")
    for token in (
        "pub trait ConversationStore",
        "pub struct PersistedConversation",
        "pub enum ConversationRole",
        "session_creation_pending",
        "turn_pending",
        "conversation_creation_pending_payload_invalid",
        "MAX_PERSISTED_CONVERSATIONS_PER_PROJECT: usize = 512",
        "MAX_CONVERSATION_MESSAGES: usize = 4096",
    ):
        require(contract, token, "conversation contract")
    for token in (
        "impl ConversationStore for LocalProjectStateStore",
        "conversation_state_root",
        "MAX_PERSISTED_CONVERSATIONS_PER_PROJECT",
    ):
        require(local, token, "local conversation store")
    for token in (
        "save_conversation",
        "load_conversation",
        "list_conversation_ids",
        "remove_project_conversations",
        "CONVERSATION_TEMP_PREFIX",
        "O_NOFOLLOW",
        "verify_private_regular_stat",
        "parse_conversation_file_name",
    ):
        require(unix, token, "secure Unix conversation store")
    for token in (
        "with_conversation_store",
        "create_persisted_conversation",
        "resume_persisted_conversation",
        "run_persisted_conversation_turn",
        "cancel_persisted_conversation_turn",
        "conversation_turn_recovery_required",
        "conversation.append_message(ConversationRole::User",
        "conversation.append_message(ConversationRole::Assistant",
        "conversation.turn_pending = true",
        "conversation.turn_pending = false",
        "small handoff window",
        "checkpoint_rollback_conversation_turn_pending",
        "checkpoint_rollback_committed_conversation_refresh_failed",
        "refresh_session_after_workspace_replacement(&reopened, &session_id)",
    ):
        require(core, token, "Core conversation controller")

    # Part 34 must use the existing one-task executor and must not grow a hidden self-reprompt loop.
    require(core, ".run_backend_task(", "Core single-turn delegation")
    controller = core[core.find("pub async fn run_persisted_conversation_turn"):core.find(
        "pub async fn cancel_persisted_conversation_turn"
    )]
    forbid(controller, r"\bloop\s*\{", "conversation controller")
    forbid(controller, r"while\s+", "conversation controller")
    forbid(controller, r"echo|dummy assistant|fake response", "conversation controller")

    require(doc, "There is no automatic re-prompt", "Part 34 documentation")
    require(doc, "No dummy assistant, echo response, or hard-coded success path", "Part 34 documentation")
    require(doc, "Uninstall is not persistence", "Part 34 documentation")
    require(ledger, "## Part 34 — Durable multi-chat + single-turn Alpha controller", "progress ledger")

    for token in (
        "ActiveProcessScope::RuntimeService",
        "pub fn start_runtime_service",
        "pub fn active_runtime_service",
        "pub fn start_persistent_runtime_service",
        "process_runtime_service_already_active",
        "runtime_service_private_directory",
        "timeout: None",
        ".env_clear()",
        "PR_SET_PDEATHSIG",
        "MAX_RUNTIME_SERVICE_ARGS",
    ):
        require(process_local, token, "Part 34.2 Node runtime service supervisor")
    for token in (
        "pub fn start_node_runtime",
        "pub fn node_runtime_active",
        "pub fn cancel_node_runtime",
        "NODE_RUNTIME_SERVICE_ID",
    ):
        require(android_host, token, "Part 34.2 Android Node host lifecycle")
    for token in (
        '"node"',
        "node_device_proof_incomplete",
        "node_device_version_mismatch",
        "page_size_16k_compatibility",
    ):
        require(device_test, token, "Part 34.2 Node device acceptance")
    forbid(process_local[process_local.find("pub fn start_runtime_service"):process_local.find("fn prepare_command")],
           r"\b(sh|bash|zsh|cmd|powershell)\b",
           "Part 34.2 runtime-service launch path")

    try:
        state = json.loads(text("PART34_STATE.json"))
    except Exception as exc:
        ERRORS.append(f"invalid PART34_STATE.json: {exc}")
        state = {}
    if state.get("checkpoint") != "part-34-source-slice":
        ERRORS.append("Part 34 state checkpoint mismatch")
    conversation = state.get("conversation", {})
    for key in (
        "multi_chat_per_project",
        "one_jcode_session_per_chat",
        "single_turn_by_default",
        "session_creation_crash_marker",
        "turn_crash_marker",
        "lifecycle_gated_turn_handoff",
        "cas_revision",
    ):
        if conversation.get(key) is not True:
            ERRORS.append(f"Part 34 conversation state missing true marker: {key}")
    if conversation.get("automatic_general_loop") is not False:
        ERRORS.append("Part 34 must keep automatic_general_loop false")
    node_runtime = state.get("node_runtime_audit", {})
    for key in (
        "trusted_runtime_service_supervisor",
        "node_supervised_launch_api",
        "node_bounded_output_capture",
        "node_timeout_and_cancel",
        "node_process_group_termination",
        "node_android_parent_death_cleanup",
        "node_single_active_service_guard",
        "node_shell_path_lookup_forbidden",
        "node_device_acceptance_contract",
    ):
        if node_runtime.get(key) is not True:
            ERRORS.append(f"Part 34.2 Node lifecycle state missing true marker: {key}")
    if node_runtime.get("node_payload_built") is not False or node_runtime.get("node_device_execution_proven") is not False:
        ERRORS.append("Part 34.2 Node binary/device proof must remain false until real evidence exists")

    if state.get("validation", {}).get("node_lifecycle_source_contract_verified") is not True:
        ERRORS.append("Part 34.2 Node lifecycle validation marker must be true")

    blockers = state.get("blockers", {})
    for key in (
        "packaged_node_android",
        "reviewed_omniroute_bundle_present",
        "omniroute_service_round_trip",
        "android_alpha_ui_wired",
        "uninstall_safe_backup_restore",
    ):
        if blockers.get(key) is not False:
            ERRORS.append(f"Part 34 blocker must remain false until proven: {key}")

    if ERRORS:
        print(f"Part 34 source validation FAILED ({len(ERRORS)} problem(s))")
        for index, error in enumerate(ERRORS, 1):
            print(f"{index}. {error}")
        return 1
    print("Part 34 source validation PASSED")
    print("Scope: durable multi-chat + single-turn controller; runtime Alpha proof intentionally not claimed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
