#!/usr/bin/env python3
from pathlib import Path
import json

ROOT=Path(__file__).resolve().parents[1]
def read(path): return (ROOT/path).read_text()
def need(text, token, label):
    if token not in text: raise SystemExit(f"Part 34.10 missing {label}: {token}")

activity=read(Path('android/app/src/main/java/com/vibecoder/shell/MainActivity.java'))
ui=read(Path('android/app/src/main/java/com/vibecoder/shell/AlphaWorkspaceUi.java'))
bridge=read(Path('android/app/src/main/java/com/vibecoder/shell/NativeBridge.java'))
native=read(Path('android/app/src/main/cpp/native_bridge.c'))
host=read(Path('crates/vibecoder-android-host/src/app_controller_ffi.rs'))
core=read(Path('crates/vibecoder-core/src/lib.rs'))
strings=read(Path('android/app/src/main/res/values/strings.xml'))
manifest=read(Path('android/app/src/main/AndroidManifest.xml'))
workflow=read(Path('.github/workflows/android-diagnostic-apk.yml'))
strict_java=read(Path('scripts/part34_10_strict_java_compile.sh'))
core_manifest=read(Path('crates/vibecoder-core/Cargo.toml'))
cargo_lock=read(Path('Cargo.lock'))
node_provision=read(Path('scripts/provision_node_android.sh'))
node_split=read(Path('scripts/verify_node_android_toolchain_split.py'))
node_sanitize=read(Path('scripts/sanitize_node_android_host_makefiles.py'))
node_cpufeatures_patch=read(Path('scripts/patch_node_android_zlib_cpufeatures.py'))
node_cpufeatures_graph=read(Path('scripts/verify_node_android_cpufeatures_integration.py'))
process_local=read(Path('crates/vibecoder-process-local/src/lib.rs'))
gateway_chat=read(Path('crates/vibecoder-gateway-omniroute/src/chat.rs'))
omni_service=read(Path('crates/vibecoder-android-host/src/omniroute_service.rs'))
compile_repairs=read(Path('scripts/test_part34_10_compile_repairs.py'))
alpha_build=read(Path('scripts/part34_alpha_build_and_verify.sh'))
alpha_evidence=read(Path('scripts/write_alpha_build_evidence.py'))
omni_fetch=read(Path('scripts/fetch_omniroute_reviewed_archive.sh'))
apk_verify=read(Path('scripts/verify_android_diagnostic_apk.sh'))
device_harness=read(Path('scripts/test_android_diagnostic_device.sh'))
alpha_package_regression=read(Path('scripts/test_part34_10_alpha_package_tools.py'))
provenance=json.loads(read(Path('third_party/provenance/omniroute-3.8.50-reviewed.json')))
provisioning=json.loads(read(Path('config/android-payload-provisioning.json')))
compile_audit=json.loads(read(Path('docs/evidence/part34_10_6_compile_log_audit.json')))
latest_compile_audit=json.loads(read(Path('docs/evidence/part34_10_7_compile_log_audit.json')))
deep_audit=json.loads(read(Path('docs/evidence/part34_10_8_deep_source_audit.json')))
project=json.loads(read(Path('PROJECT_STATE.json')))['part34_10_alpha_mobile_ui']
state=json.loads(read(Path('PART34_STATE.json')))['alpha_mobile_ui']

for token in (
    'AlphaWorkspaceUi', 'showDiagnosticsDialog()', 'startChatRuntime();',
    'OmniRouteAssetInstaller.ensureInstalled', 'NativeBridge.nativeOmniRouteStart(',
    'runtime_profile_round_trip_proven', 'hidden_model_reroutes_disabled',
    'NativeBridge.nativeAppControllerInit(', 'for (int attempt = 0; attempt < 4; attempt++)',
    'Thread.sleep(250L)', 'catalogMayStillBeWarming', 'NativeBridge.nativeChatCreate()',
    'NativeBridge.nativeChatSend(', 'NativeBridge.nativeChatCancel(',
    'createNewChat()', 'sendChatPrompt(prompt)', 'stopActiveChatTurn()',
    'chatStopRequested', 'cancelExecutor'):
    need(activity, token, 'automatic runtime / real chat bridge')
if 'Chat controller UI bridge is not connected yet; draft was not sent.' in activity:
    raise SystemExit('Part 34.10 stale disconnected Send boundary remains')
need(activity, 'if (shouldAutoRunDiagnostics())', 'diagnostics explicit test gate')
need(activity, 'getBooleanExtra("vibecoder_diagnostic_test", false)', 'diagnostics test intent extra')
if 'startChatRuntime();\n        runDiagnostics();' in activity:
    raise SystemExit('Part 34.10 normal app startup still duplicates heavyweight diagnostics')
need(device_harness, '--ez vibecoder_diagnostic_test true', 'physical diagnostic harness explicit intent')
need(device_harness, '|| "$MODE" == "alpha"', 'combined Alpha physical acceptance mode')

for token in (
    '"Old Chats"','"◯  Chat"','"▣  Preview"','"＋  New Chat"',
    'vibecoder/state/conversations','isSafeConversationFile','Files.isSymbolicLink',
    'MAX_CHAT_FILE_BYTES','"Preview not active yet"','callbacks.onSendRequested(prompt)',
    'callbacks.onStopRequested()','installSystemUiInsets()','WindowInsets.Type.ime()',
    'Api33Back.register(activity, this::closeDrawer)','MAX_RENDERED_TEXT_BYTES',
    'Executors.newSingleThreadExecutor()','conversationJsonIsValidForDisplay',
    'preparingChat', 'conversationBlocked', 'callbacks.onConversationSelectionCleared()',
    'This saved chat needs recovery before another message can be sent.',
    'truncateUtf8ForDisplay(text, MAX_RENDERED_SINGLE_MESSAGE_BYTES)'):
    need(ui, token, 'mobile UI contract')
for forbidden in ('Deployed to staging','Build successful','Firebase','https://startup-',
                  'MCP connected','bash ready','Browser automation'):
    if forbidden in ui: raise SystemExit(f"Part 34.10 fake/unsupported UI token present: {forbidden}")

for token in ('nativeAppControllerInit(', 'nativeChatCreate()', 'nativeChatSend(', 'nativeChatCancel('):
    need(bridge, token, 'Java JNI declarations')
for token in (
    'vibecoder_android_host_app_controller_init_json',
    'vibecoder_android_host_chat_create_json',
    'vibecoder_android_host_chat_send_json',
    'vibecoder_android_host_chat_cancel_json',
    'utf8_bytes_to_jstring', 'CHAT_JSON_CAPACITY'):
    need(native, token, 'JNI native bridge')
for token in (
    'vibecoder_android_host_app_controller_init_json',
    'vibecoder_android_host_chat_create_json',
    'vibecoder_android_host_chat_send_json',
    'vibecoder_android_host_chat_cancel_json',
    'list_gateway_models(GatewayCredential::Anonymous)',
    'models.retain(|model| android_chat_model_id_usable(&model.id))',
    'fn android_chat_model_id_usable(value: &str) -> bool',
    'value.len() <= 512', 'byte.is_ascii_graphic()',
    'models.sort_by(|left, right| left.id.cmp(&right.id))',
    'run_persisted_model_conversation_turn_cancellable'):
    need(host, token, 'Rust Android app controller')
for token in (
    'ConversationModelTurnCancellation',
    'run_persisted_model_conversation_turn_cancellable',
    'await_conversation_model_inference_or_cancel',
    'tokio::time::timeout(Duration::from_millis(50)'):
    need(core, token, 'cancellable durable model turn')
if 'tokio::select!' in core:
    raise SystemExit('Part 34.10 cancellable model turn unexpectedly requires tokio macros')

need(strings, '<string name="app_name">VibeCoder</string>', 'app label')
need(manifest, 'android:windowSoftInputMode="adjustResize"', 'IME resize boundary')
need(workflow, 'bash scripts/part34_10_strict_java_compile.sh', 'strict Java CI gate')
need(strict_java, 'javac --release 17 -Xlint:all -Werror', 'strict Java warnings-as-errors')

for token in (
    'full-alpha-package:',
    'needs: [jcode-android-proof-build, node-android-proof-build]',
    'actions/setup-node@v6.4.0',
    'node-version: "24.19.0"',
    'package-manager-cache: false',
    'actions/download-artifact@v8.0.1',
    'bash scripts/fetch_omniroute_reviewed_archive.sh',
    'bash scripts/part34_alpha_build_and_verify.sh',
    'vibecoder-part34-full-alpha-apk',
    'python3 scripts/test_part34_10_alpha_package_tools.py'):
    need(workflow, token, 'full Alpha one-APK CI lane')
for token in (
    'libvibecoder_jcode_exec.so', 'libvibecoder_node_exec.so',
    'build_omniroute_android_bundle.py', 'stage_omniroute_android_asset.py',
    'build_android_host.sh', 'build_android_shell.sh',
    'verify_android_diagnostic_apk.sh" "$APK" alpha',
    'write_alpha_build_evidence.py'):
    need(alpha_build, token, 'full Alpha build script')
for token in (
    'MODE" == "jcode" || "$MODE" == "alpha"',
    'MODE" == "node" || "$MODE" == "omniroute_service"',
    'MODE" == "omniroute_asset" || "$MODE" == "omniroute_service"'):
    need(apk_verify, token, 'full Alpha APK payload verifier')
for token in (
    'SOURCE_REF="release/v3.8.50"',
    'REVIEWED_COMMIT="ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"',
    'EXPECTED_SHA256="1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7"',
    'archive/refs/heads/${SOURCE_REF}.zip',
    'comment != commit'):
    need(omni_fetch, token, 'exact reviewed OmniRoute archive fetch')
for token in ('jcode_build_evidence_payload_mismatch', 'payload_bound_to_proof_evidence',
              'payload_bound_to_cross_build_evidence'):
    need(alpha_package_regression, token, 'Alpha package evidence regression')
for token in ('full_alpha_apk_package_evidence_only_not_device_execution',
              "'device_execution_proven': False",
              "'device_service_round_trip_proven': False"):
    need(alpha_evidence, token, 'non-overclaiming Alpha build evidence')
if provenance.get('source_ref') != 'release/v3.8.50':
    raise SystemExit('Part 34.10 OmniRoute provenance source_ref mismatch')
if provenance.get('reviewed_git_commit') != 'ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f':
    raise SystemExit('Part 34.10 OmniRoute provenance commit mismatch')
omni_payload=next((item for item in provisioning.get('payloads', []) if item.get('component_id') == 'omniroute'), None)
if omni_payload is None or omni_payload.get('source_ref') != 'release/v3.8.50':
    raise SystemExit('Part 34.10 payload provisioning OmniRoute source_ref mismatch')
if compile_audit.get('part') != '34.10.6' or compile_audit.get('external_recompile_required_for_zero_error_claim') is not True:
    raise SystemExit('Part 34.10 compile-log audit identity/claim mismatch')
observed=compile_audit.get('current_ci_observations', {})
if observed.get('strict_java_17_compile_passed') is not True or observed.get('jcode_0_73_0_release_build_passed') is not True:
    raise SystemExit('Part 34.10 compile-log audit lost positive CI evidence')
for overclaim in ('vibecoder_android_host_compile_passed','node_android_binary_built','full_alpha_apk_built'):
    if observed.get(overclaim) is not False:
        raise SystemExit(f'Part 34.10 compile-log audit overclaim: {overclaim}')

if latest_compile_audit.get('step') != '34.10.7-latest-compile-repair':
    raise SystemExit('Part 34.10 latest compile audit step mismatch')
if latest_compile_audit.get('observed',{}).get('node_linker_failure') != 'undefined symbol android_getCpuFeatures':
    raise SystemExit('Part 34.10 latest Node linker failure evidence mismatch')
if latest_compile_audit.get('repairs',{}).get('node_zlib_ndk_cpufeatures_source_linkage_patch') is not True:
    raise SystemExit('Part 34.10 latest Node cpufeatures repair evidence missing')
if latest_compile_audit.get('post_fix_compile_claim') is not False:
    raise SystemExit('Part 34.10 latest compile audit overclaims post-fix compile')

expected={
 'portrait_mobile_shell':True,'chat_tab':True,'preview_tab':True,'old_chat_drawer':True,
 'fake_sample_chats':False,'send_controller_jni_wired':True,'stop_controller_jni_wired':True,
 'new_chat_controller_jni_wired':True,'normal_conversation_send_wired':True,
 'coding_agent_send_ui_wired':False,'omniroute_auto_install_on_app_open_wired':True,
 'omniroute_auto_start_on_app_open_wired':True,'omniroute_manual_runtime_setup_required':False,
 'provider_setup_ui_wired':False,'selected_model_from_fresh_exact_catalog':True,
 'selected_model_ascii_safe_filtered':True,'bootstrap_catalog_readiness_retry_bounded':True,
 'send_unicode_safe_jni_decode':True,'cancellable_model_turn_wired':True,
 'live_preview_runtime_wired':False,'diagnostic_harness_preserved':True,
 'physical_ui_acceptance_proven':False,'drawer_io_off_main_thread':True,
 'drawer_latest_first_before_limit':True,'drawer_identity_validation_hardened':True,
 'system_bar_and_ime_insets_handled':True,'predictive_back_drawer_handled':True,
 'chat_render_memory_bounded':True,'strict_java_ci_gate':True,
 'diagnostics_auto_run_only_for_test_intent':True,'normal_app_start_does_not_duplicate_diagnostics':True,
 'full_alpha_build_lane_ready':True,'full_alpha_requires_jcode_node_omniroute_same_apk':True,
 'exact_reviewed_omniroute_archive_fetch_fail_closed':True,'full_alpha_apk_compiled':False,
 'full_alpha_package_verified':False,'physical_alpha_acceptance_proven':False,
 'physical_alpha_acceptance_mode_defined':True}
for key,value in expected.items():
    if project.get(key)!=value: raise SystemExit(f"Part 34.10 PROJECT_STATE mismatch: {key}")
for key in ('compile_log_repair_applied','vibecoder_core_tokio_direct_dependency_fixed',
            'node_android_host_target_toolchain_split_enforced','node_android_split_regression_guard_added',
            'second_compile_attempt_analyzed','second_compile_failure_repaired_not_recompiled',
            'omniroute_process_runtime_trait_import_fixed','omniroute_last_profile_warning_removed',
            'node_generated_makefile_materialization_fixed','node_host_flag_sanitizer_after_gyp',
            'omniroute_runtime_profile_hash_authority_repaired','part34_3_service_regression_in_ci',
            'latest_compile_attempt_analyzed','process_runtime_control_predicate_compile_fixed',
            'gateway_chat_test_only_import_warning_fixed','node_android_zlib_cpufeatures_linkage_fixed'):
    if project.get(key) is not True:
        raise SystemExit(f"Part 34.10 PROJECT_STATE compile repair flag missing: {key}")
if project.get('first_compile_failure_repaired_not_recompiled') is not False:
    raise SystemExit('Part 34.10 PROJECT_STATE stale first-compile pending flag')
if project.get('node_proven_host_flag_guard') != '-mbranch-protection=standard':
    raise SystemExit('Part 34.10 PROJECT_STATE Node proven host flag guard mismatch')
if project.get('status') != 'deep_source_audit_clean_recompile_pending':
    raise SystemExit('Part 34.10.8 PROJECT_STATE audit status mismatch')
if project.get('step') != '34.10.8-deep-source-audit':
    raise SystemExit('Part 34.10.8 PROJECT_STATE audit step mismatch')
expected_state={
 'portrait_layout':True,'drawer_old_chats':True,'drawer_reads_persisted_conversations_only':True,
 'drawer_mutates_conversation_store':False,'preview_placeholder_only':True,
 'fake_model_replies':False,'send_bridge_wired':True,'stop_bridge_wired':True,
 'new_chat_bridge_wired':True,'normal_conversation_send_wired':True,
 'coding_agent_send_ui_wired':False,'omniroute_auto_install_wired':True,
 'omniroute_auto_start_wired':True,'omniroute_manual_runtime_setup_required':False,
 'provider_setup_ui_wired':False,'selected_model_from_fresh_exact_catalog':True,
 'selected_model_ascii_safe_filtered':True,'bootstrap_catalog_readiness_retry_bounded':True,
 'send_unicode_safe_jni_decode':True,'cancellable_model_turn_wired':True,
 'preview_runtime_wired':False,'diagnostics_available_from_settings':True,'real_android_ui_proven':False,
 'drawer_io_off_main_thread':True,'rust_conversation_size_limit_aligned':True,
 'drawer_latest_first_before_limit':True,'drawer_identity_validation_hardened':True,
 'system_bar_and_ime_insets_handled':True,'predictive_back_drawer_handled':True,
 'chat_render_memory_bounded':True,'strict_java_ci_gate':True,
 'strict_java_stub_compile_proven_in_audit_runner':True,
 'diagnostics_auto_run_only_for_test_intent':True,'normal_app_start_does_not_duplicate_diagnostics':True,
 'full_alpha_build_lane_ready':True,'full_alpha_requires_jcode_node_omniroute_same_apk':True,
 'exact_reviewed_omniroute_archive_fetch_fail_closed':True,'full_alpha_apk_compiled':False,
 'full_alpha_package_verified':False,'physical_alpha_acceptance_proven':False,
 'physical_alpha_acceptance_mode_defined':True}
for key,value in expected_state.items():
    if state.get(key)!=value: raise SystemExit(f"Part 34.10 PART34_STATE mismatch: {key}")
for key in ('compile_log_repair_applied','vibecoder_core_tokio_direct_dependency_fixed',
            'node_android_host_target_toolchain_split_enforced','node_android_split_regression_guard_added',
            'second_compile_attempt_analyzed','second_compile_failure_repaired_not_recompiled',
            'omniroute_process_runtime_trait_import_fixed','omniroute_last_profile_warning_removed',
            'node_generated_makefile_materialization_fixed','node_host_flag_sanitizer_after_gyp',
            'omniroute_runtime_profile_hash_authority_repaired','part34_3_service_regression_in_ci',
            'latest_compile_attempt_analyzed','process_runtime_control_predicate_compile_fixed',
            'gateway_chat_test_only_import_warning_fixed','node_android_zlib_cpufeatures_linkage_fixed'):
    if state.get(key) is not True:
        raise SystemExit(f"Part 34.10 PART34_STATE compile repair flag missing: {key}")
if state.get('first_compile_failure_repaired_not_recompiled') is not False:
    raise SystemExit('Part 34.10 PART34_STATE stale first-compile pending flag')
if state.get('node_proven_host_flag_guard') != '-mbranch-protection=standard':
    raise SystemExit('Part 34.10 PART34_STATE Node proven host flag guard mismatch')
if state.get('status') != 'deep_source_audit_clean_recompile_pending':
    raise SystemExit('Part 34.10.8 PART34_STATE audit status mismatch')
if state.get('step') != '34.10.8-deep-source-audit':
    raise SystemExit('Part 34.10.8 PART34_STATE audit step mismatch')

for container,label in ((project,'PROJECT_STATE'),(state,'PART34_STATE')):
    if container.get('bootstrap_catalog_retry_attempts') != 4:
        raise SystemExit(f'Part 34.10 {label} bootstrap_catalog_retry_attempts mismatch')
    if container.get('bootstrap_catalog_retry_delay_ms') != 250:
        raise SystemExit(f'Part 34.10 {label} bootstrap_catalog_retry_delay_ms mismatch')
    if container.get('reviewed_omniroute_source_ref') != 'release/v3.8.50':
        raise SystemExit(f'Part 34.10 {label} reviewed_omniroute_source_ref mismatch')
    if container.get('reviewed_omniroute_git_commit') != 'ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f':
        raise SystemExit(f'Part 34.10 {label} reviewed_omniroute_git_commit mismatch')

need(core_manifest, 'tokio.workspace = true', 'vibecoder-core Tokio direct dependency')
core_lock_start=cargo_lock.index('name = "vibecoder-core"')
core_lock_end=cargo_lock.index('\n[[package]]', core_lock_start)
if ' "tokio",' not in cargo_lock[core_lock_start:core_lock_end]:
    raise SystemExit('Part 34.10 vibecoder-core Cargo.lock dependency list missing tokio')
if 'let (project, session_id, mut conversation, checkpoint) = {' in core:
    raise SystemExit('Part 34.10 known unused_mut warning regressed')
for token in ('HOST_CC=', 'HOST_CXX=', 'CC.host=$HOST_CC', 'CXX.host=$HOST_CXX',
              'CC.target=$NDK_CC', 'CXX.target=$NDK_CXX',
              'verify_node_android_toolchain_split.py', 'node_android_toolchain_split_log_invalid'):
    need(node_provision, token, 'Node Android host/target compiler split')
for token in ('host_compiler_must_not_be_from_android_ndk',
              'android_target_compiler_used_for_obj_host',
              'expected_android_compiler_not_observed_for_obj_target',
              '--require-observed'):
    need(node_split, token, 'Node Android toolchain-split verifier')
need(compile_repairs, 'old_android_compiler_for_obj_host_not_rejected', 'compile-log repair regression')
for token in ('-mbranch-protection=standard','proven_host_target_flag_not_found','target_makefiles_modified',
              'node_android_host_makefile_sanitize'):
    need(node_sanitize, token, 'Node Android proven host-only flag sanitizer')
for token in ('ProcessRuntime, ProcessTermination','self.process_runtime.cancel(process_id)'):
    need(omni_service, token, 'OmniRoute ProcessRuntime compile repair')
if 'last_profile' in omni_service:
    raise SystemExit('Part 34.10 OmniRoute known unused last_profile warning regressed')
if 'value.chars().any(char::is_control)' not in process_local or 'is_forbidden_control' in process_local:
    raise SystemExit('Part 34.10 runtime-service control predicate compile repair regressed')
if 'GatewayChatMessage' in gateway_chat.split('#[cfg(test)]',1)[0]:
    raise SystemExit('Part 34.10 gateway test-only import warning regressed')
for token in ('vibecoder-node-24.19.0-android-zlib-cpufeatures-v1',
              '<(android_ndk_path)/sources/android/cpufeatures/cpu-features.c',
              'node_android_cpufeatures_patch_already_applied'):
    need(node_cpufeatures_patch, token, 'Node Android zlib cpufeatures linkage patch')
for token in ('patch_node_android_zlib_cpufeatures.py', 'android_ndk_cpufeatures_source_missing:',
              'node_android_cpufeatures_patch_failed:'):
    need(node_provision, token, 'Node Android cpufeatures provision contract')
for token in ('node_android_cpufeatures_generated_graph', 'cpufeatures_source_missing_from_target_graph',
              'cpufeatures_source_leaked_into_host_graph', 'SOURCE_TOKEN'):
    need(node_cpufeatures_graph, token, 'Node Android generated cpufeatures graph verifier')
for token in ('verify_node_android_cpufeatures_integration.py', 'node_android_cpufeatures_generated_graph_invalid'):
    need(node_provision, token, 'Node Android generated cpufeatures graph provision guard')
for token in ('scripts/sanitize_node_android_host_makefiles.py',
              'python3 scripts/test_part34_3_service_tools.py',
              'python3 scripts/test_part34_10_compile_repairs.py'):
    need(workflow, token, 'deep-audit CI regression')

for record,label in ((project,'PROJECT_STATE'),(state,'PART34_STATE')):
    for key in ('node_cpufeatures_generated_graph_guard_added','deep_source_compile_contract_audit_passed'):
        if record.get(key) is not True:
            raise SystemExit(f'Part 34.10.8 {label} deep-audit flag missing: {key}')
    if record.get('node_cpufeatures_generated_graph_ci_proven') is not False:
        raise SystemExit(f'Part 34.10.8 {label} overclaims cpufeatures graph CI proof')
    if record.get('step') != '34.10.8-deep-source-audit':
        raise SystemExit(f'Part 34.10.8 {label} step mismatch')
if deep_audit.get('node_cpufeatures_generated_graph_guard_added') is not True or deep_audit.get('fresh_android_compile') is not False:
    raise SystemExit('Part 34.10.8 deep source audit evidence mismatch')

print('Part 34.10.8 deep source audit validation PASSED')
