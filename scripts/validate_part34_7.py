#!/usr/bin/env python3
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
problems = []

def read(rel):
    path = ROOT / rel
    if not path.is_file():
        problems.append(f'missing:{rel}')
        return ''
    return path.read_text(encoding='utf-8')

def need(text, token, label):
    if token not in text:
        problems.append(f'missing_contract:{label}:{token}')

config = read('crates/vibecoder-agent-jcode/src/config.rs')
lifecycle = read('crates/vibecoder-agent-jcode/src/lifecycle.rs')
runtime = read('crates/vibecoder-agent-jcode/src/runtime.rs')
model = read('crates/vibecoder-agent-jcode/src/model.rs')
core = read('crates/vibecoder-core/src/lib.rs')
task = read('crates/vibecoder-task-orchestration/src/lib.rs')
host = read('crates/vibecoder-android-host/src/lib.rs')
workflow = read('.github/workflows/android-diagnostic-apk.yml')
doc = read('docs/PART34_7_JCODE_MODEL_TOOL_BRIDGE.md')
turn = read('crates/vibecoder-agent-jcode/src/turn.rs')

for token in (
    'pub enum JcodeModelGatewayBridge',
    'VibeCoderOmniRouteLoopbackV1',
    'http://127.0.0.1:20128/v1',
    'Jcode model gateway bridge requires a private runtime',
    'VIBECODER_BRIDGED_MAX_TOOL_CALLS_PER_TURN: u32 = 32',
): need(config, token, 'fixed_bridge_config')

for token in (
    '("JCODE_OPENAI_COMPAT_API_BASE", bridge.api_base())',
    '("JCODE_OPENAI_COMPAT_LOCAL_ENABLED", "1")',
    '("JCODE_OPENROUTER_ALLOW_NO_AUTH", "1")',
    '("JCODE_RUNTIME_PROVIDER", "openai-compatible")',
    '("JCODE_INITIAL_PROVIDER_EXPLICIT", "1")',
    '("JCODE_OPENROUTER_NO_FALLBACK", "1")',
    '("JCODE_AUTO_POKE", "off")',
    '("JCODE_RUN_AUTO_POKE", "0")',
    '("JCODE_AUTOREVIEW_ENABLED", "false")',
    '("JCODE_AUTOJUDGE_ENABLED", "false")',
    '("JCODE_TOOL_PROFILE", "minimal")',
    '("JCODE_DISABLED_TOOLS", "bash")',
    '("JCODE_NO_TELEMETRY", "1")',
): need(lifecycle, token, 'jcode_launch_policy')

for token in (
    'file_tools: self.connection.config().model_gateway_bridge.is_some()',
    'value == "session_files"',
    'command_tools: false',
    'fn model_gateway_bridge_identity(&self) -> Option<ModelGatewayBridgeIdentity>',
): need(runtime, token, 'runtime_capability_gate')

for token in (
    'model_gateway_bridge: Some(',
    'JcodeModelGatewayBridge::VibeCoderOmniRouteLoopbackV1',
): need(host, token, 'android_packaged_bridge')

for token in (
    'let agent_gateway_bridge = self.agent.model_gateway_bridge_identity();',
    'require_agent_gateway_bridge_matches_profile(bridge, &profile)?;',
    'task.corroborate_bridged_agent_catalog(',
    'agent_model_gateway_bridge_profile_mismatch',
): need(core, token, 'core_bridge_authority')

for token in (
    'pub fn corroborate_bridged_agent_catalog(',
    'agent_bridge_transport_provider_mismatch',
    'bridged_catalog_uses_transport_provider_but_preserves_gateway_upstream_identity',
    'bridged_catalog_rejects_wrong_transport_provider',
): need(task, token, 'bridge_catalog_corroboration')

for token in (
    'bridge_transport_provider: Option<&str>',
    'Jcode bridged model route does not report the expected transport provider',
    'Jcode fresh active-provider probe does not match the attested gateway transport provider',
): need(model, token, 'jcode_model_identity')

for token in (
    'auto_approve: false',
    'AgentEvent::ToolStarted',
    'AgentEvent::ToolFinished',
    'max_tool_calls: Option<u32>',
    'observed > limit',
    'safety_client.cancel(&callback_session.0)',
): need(turn, token, 'tool_observation_permission_boundary')

for token in (
    '- "crates/vibecoder-agent-contract/**"',
    '- "crates/vibecoder-agent-jcode/**"',
    '- "crates/vibecoder-task-orchestration/**"',
    'python3 scripts/validate_part34_7.py',
    'python3 scripts/test_part34_7_jcode_bridge_tools.py',
): need(workflow, token, 'ci_bridge_coverage')

for token in (
    'Source lane complete',
    'JCODE_TOOL_PROFILE=minimal',
    'JCODE_DISABLED_TOOLS=bash',
    '`command_tools=false`',
    'real Android model-driven Jcode tool turn',
): need(doc, token, 'part34_7_docs')

try:
    sources = json.loads(read('third_party/SOURCES.lock.json'))
    jcode = next(item for item in sources['sources'] if item['name'] == 'jcode')
    expected = {
        'version': '0.73.0',
        'sha256': 'dd6efc76c253a4a5d9ea35ec640f80980b898f1f98a6db0671d0efefa8b141f2',
        'reviewed_openai_compatible_local_endpoint_supported': True,
        'reviewed_openai_compatible_no_auth_local_supported': True,
        'vibecoder_part34_7_tool_profile': 'minimal_with_bash_disabled',
        'vibecoder_part34_7_command_tools_enabled': False,
    }
    for key, value in expected.items():
        if jcode.get(key) != value:
            problems.append(f'jcode_provenance_mismatch:{key}:{jcode.get(key)!r}')
    if jcode.get('reviewed_minimal_tool_profile') != ['bash','read','write','edit','multiedit','apply_patch','patch','agentgrep','ls']:
        problems.append('jcode_reviewed_minimal_tool_profile_mismatch')
except Exception as exc:
    problems.append(f'jcode_provenance_invalid:{exc}')

try:
    state = json.loads(read('PART34_STATE.json'))['jcode_model_tool_bridge']
    expected = {
        'step':'34.7-jcode-model-tool-bridge',
        'status':'source_lane_complete_real_android_agent_tool_turn_pending',
        'android_private_bridge_enabled':True,
        'exact_model_id_passthrough':True,
        'gateway_profile_match_required':True,
        'bridged_catalog_transport_corroboration':True,
        'reviewed_session_files_capability_required_for_file_tools':True,
        'jcode_tool_profile':'minimal',
        'bash_disabled':True,
        'command_tools_enabled':False,
        'hidden_provider_fallback_disabled':True,
        'auto_poke_disabled':True,
        'autoreview_disabled':True,
        'autojudge_disabled':True,
        'sdk_auto_approve_permissions':False,
        'tool_events_observable':True,
        'max_tool_calls_per_turn':32,
        'tool_limit_cancels_turn':True,
        'real_android_agent_tool_turn_proven':False,
        'fresh_rust_compile':False,
    }
    for key, value in expected.items():
        if state.get(key) != value:
            problems.append(f'part34_state_mismatch:{key}:{state.get(key)!r}')
except Exception as exc:
    problems.append(f'part34_state_invalid:{exc}')

try:
    state = json.loads(read('PROJECT_STATE.json'))['part34_7_jcode_model_tool_bridge']
    for key in (
        'source_bridge_connected','omniroute_loopback_only','exact_model_passthrough',
        'upstream_provider_separated_from_transport_provider','android_packaged_jcode_bridge_enabled',
        'file_tools_enabled_under_reviewed_bridge','minimal_tool_profile','bash_disabled',
        'tool_call_limit_fail_closed',
    ):
        if state.get(key) is not True:
            problems.append(f'project_state_missing_true:{key}')
    if state.get('max_tool_calls_per_normal_turn') != 32:
        problems.append(f"project_state_tool_limit_mismatch:{state.get('max_tool_calls_per_normal_turn')!r}")
    for key in (
        'command_tools_enabled','automatic_jcode_provider_fallback','automatic_jcode_poke',
        'automatic_jcode_review','automatic_jcode_judge','blanket_permission_auto_approve',
        'real_android_agent_tool_turn_proven','fresh_rust_compile_for_34_7',
    ):
        if state.get(key) is not False:
            problems.append(f'project_state_overclaim:{key}')
except Exception as exc:
    problems.append(f'project_state_invalid:{exc}')

if problems:
    print(f'Part 34.7 source validation FAILED ({len(problems)} problem(s))')
    for i, problem in enumerate(problems,1): print(f'{i}. {problem}')
    sys.exit(1)
print('Part 34.7 source validation PASSED')
print('Scope: exact OmniRoute model -> private Jcode OpenAI-compatible bridge -> reviewed file-tool surface; command tools disabled')
