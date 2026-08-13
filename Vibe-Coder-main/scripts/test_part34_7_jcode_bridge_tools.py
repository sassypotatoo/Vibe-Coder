#!/usr/bin/env python3
"""Fail-closed source regression checks for Part 34.7 OmniRoute -> Jcode tool bridge."""
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
read = lambda rel: (ROOT / rel).read_text(encoding='utf-8')

config = read('crates/vibecoder-agent-jcode/src/config.rs')
lifecycle = read('crates/vibecoder-agent-jcode/src/lifecycle.rs')
runtime = read('crates/vibecoder-agent-jcode/src/runtime.rs')
model = read('crates/vibecoder-agent-jcode/src/model.rs')
core = read('crates/vibecoder-core/src/lib.rs')
task = read('crates/vibecoder-task-orchestration/src/lib.rs')
host = read('crates/vibecoder-android-host/src/lib.rs')
contract = read('crates/vibecoder-agent-contract/src/lib.rs')
sources = json.loads(read('third_party/SOURCES.lock.json'))
jcode = next(source for source in sources['sources'] if source['name'] == 'jcode')

checks = {
    'exact_jcode_pin': jcode['version'] == '0.73.0' and jcode['sha256'] == 'dd6efc76c253a4a5d9ea35ec640f80980b898f1f98a6db0671d0efefa8b141f2',
    'reviewed_session_files': 'session_files' in jcode['reviewed_bridge_capabilities'],
    'reviewed_local_compat': jcode.get('reviewed_openai_compatible_local_endpoint_supported') is True and jcode.get('reviewed_openai_compatible_no_auth_local_supported') is True,
    'reviewed_minimal_profile': jcode.get('reviewed_minimal_tool_profile') == ['bash','read','write','edit','multiedit','apply_patch','patch','agentgrep','ls'],
    'fixed_api_base': 'http://127.0.0.1:20128/v1' in config,
    'fixed_gateway': 'VIBECODER_OMNIROUTE_JCODE_GATEWAY_ID: &str = "omniroute"' in config,
    'private_only_bridge': 'Jcode model gateway bridge requires a private runtime' in config,
    'bridge_identity_contract': 'pub struct ModelGatewayBridgeIdentity' in contract and 'exact_model_id_passthrough: bool' in contract,
    'local_compat_enabled': '("JCODE_OPENAI_COMPAT_LOCAL_ENABLED", "1")' in lifecycle,
    'local_no_auth_enabled': '("JCODE_OPENROUTER_ALLOW_NO_AUTH", "1")' in lifecycle,
    'provider_fallback_off': '("JCODE_OPENROUTER_NO_FALLBACK", "1")' in lifecycle,
    'auto_poke_off': '("JCODE_AUTO_POKE", "off")' in lifecycle and '("JCODE_RUN_AUTO_POKE", "0")' in lifecycle,
    'review_judge_off': '("JCODE_AUTOREVIEW_ENABLED", "false")' in lifecycle and '("JCODE_AUTOJUDGE_ENABLED", "false")' in lifecycle,
    'minimal_tools': '("JCODE_TOOL_PROFILE", "minimal")' in lifecycle,
    'bash_disabled': '("JCODE_DISABLED_TOOLS", "bash")' in lifecycle,
    'ambient_proxy_off': '("NO_PROXY", "127.0.0.1,localhost")' in lifecycle and '("no_proxy", "127.0.0.1,localhost")' in lifecycle,
    'file_capability_bridge_gated': 'file_tools: self.connection.config().model_gateway_bridge.is_some()' in runtime and 'value == "session_files"' in runtime,
    'command_capability_false': 'command_tools: false' in runtime,
    'tool_call_cap_constant': 'VIBECODER_BRIDGED_MAX_TOOL_CALLS_PER_TURN: u32 = 32' in config,
    'tool_call_cap_wired': 'tool_limit_exceeded' in runtime and 'max_tool_calls: Option<u32>' in read('crates/vibecoder-agent-jcode/src/turn.rs'),
    'tool_call_overflow_cancels': 'observed > limit' in read('crates/vibecoder-agent-jcode/src/turn.rs') and 'safety_client.cancel(&callback_session.0)' in read('crates/vibecoder-agent-jcode/src/turn.rs'),
    'android_bridge_enabled': 'model_gateway_bridge: Some(' in host and 'JcodeModelGatewayBridge::VibeCoderOmniRouteLoopbackV1' in host,
    'bridge_profile_checked': 'require_agent_gateway_bridge_matches_profile(bridge, &profile)?;' in core,
    'bridged_catalog_path': 'task.corroborate_bridged_agent_catalog(' in core,
    'transport_category_separated': 'corroborate_bridged_agent_catalog' in task and 'agent_bridge_transport_provider_mismatch' in task,
    'model_bridge_provider_verify': 'bridge_transport_provider: Option<&str>' in model and 'expected transport provider' in model,
    'sdk_blanket_autoapprove_off': 'auto_approve: false' in read('crates/vibecoder-agent-jcode/src/turn.rs'),
    'tool_events_observable': 'AgentEvent::ToolStarted' in read('crates/vibecoder-agent-jcode/src/turn.rs') and 'AgentEvent::ToolFinished' in read('crates/vibecoder-agent-jcode/src/turn.rs'),
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit('Part 34.7 Jcode bridge regression FAILED: ' + ','.join(failed))

# The bridge launch profile must never accidentally re-enable command execution in this slice.
start = lifecycle.index('fn model_gateway_bridge_launch_env')
bridge_env = lifecycle[start:]
if '("JCODE_TOOL_PROFILE", "minimal")' not in bridge_env or '("JCODE_DISABLED_TOOLS", "bash")' not in bridge_env:
    raise SystemExit('Part 34.7 Jcode bridge regression FAILED: unsafe tool profile')
if 'command_tools: true' in runtime:
    raise SystemExit('Part 34.7 Jcode bridge regression FAILED: command tools overclaimed')

print('Part 34.7 Jcode bridge regression PASSED')
