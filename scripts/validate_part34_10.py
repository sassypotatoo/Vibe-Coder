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
node_runtime_workflow=read(Path('.github/workflows/node-runtime-proof.yml'))
strict_java=read(Path('scripts/part34_10_strict_java_compile.sh'))
core_manifest=read(Path('crates/vibecoder-core/Cargo.toml'))
host_manifest=read(Path('crates/vibecoder-android-host/Cargo.toml'))
cargo_lock=read(Path('Cargo.lock'))
node_provision=read(Path('scripts/provision_node_android.sh'))
node_split=read(Path('scripts/verify_node_android_toolchain_split.py'))
node_sanitize=read(Path('scripts/sanitize_node_android_host_makefiles.py'))
node_cpufeatures_patch=read(Path('scripts/patch_node_android_zlib_cpufeatures.py'))
node_cpufeatures_graph=read(Path('scripts/verify_node_android_cpufeatures_integration.py'))
node_configure_verify=read(Path('scripts/verify_node_android_configure_output.py'))
node_host_arch_graph=read(Path('scripts/verify_node_android_host_arch_graph.py'))
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
graph_repair=json.loads(read(Path('docs/evidence/part34_10_9_node_gyp_graph_repair.json')))
latest_repair=json.loads(read(Path('docs/evidence/part34_10_10_node_relative_cpufeatures_agent_routing.json')))
latest_compile_repair=json.loads(read(Path('docs/evidence/part34_10_11_android_libc_node_timeout_repair.json')))
latest_host_arch_repair=json.loads(read(Path('docs/evidence/part34_10_12_node_configure_host_arch_repair.json')))
latest_sanitizer_repair=json.loads(read(Path('docs/evidence/part34_10_13_node_clean_host_sanitizer_repair.json')))
latest_ci_throughput_repair=json.loads(read(Path('docs/evidence/part34_10_14_node_ci_throughput_timeout_repair.json')))
checkpoint_local=read(Path('crates/vibecoder-checkpoint-local/src/lib.rs'))
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

for token in ('pub async fn run_persisted_conversation_turn(',
              'pub async fn run_persisted_agent_action_turn(',
              'pub async fn run_persisted_agent_action_turn_resolved<',
              '.chat_completion(gateway_credential, &request)'):
    need(core, token, 'real conversation / coding-agent source capability')

for token in ('fn classify_chat_route(prompt: &str) -> ChatRoute',
              'ChatRoute::ModelChat', 'ChatRoute::AgentAction',
              'run_persisted_agent_action_turn(', 'turn_kind: "agent_action"',
              '.with_checkpoint_store(checkpoint_store)',
              'cancel_persisted_conversation_turn(project_id, conversation_id)'):
    need(host, token, 'automatic normal-chat / coding-agent routing')
for token in ('vibecoder-checkpoint-local', 'vibecoder-routing'):
    need(host_manifest, token, 'Android host agent-routing dependency')
if 'run_explicit_agent_loop' in host or 'run_persisted_explicit' in host:
    raise SystemExit('Part 34.10 automatic chat routing must not auto-enable explicit outer loop')

need(strings, '<string name="app_name">VibeCoder</string>', 'app label')
need(manifest, 'android:windowSoftInputMode="adjustResize"', 'IME resize boundary')
need(workflow, 'bash scripts/part34_10_strict_java_compile.sh', 'strict Java CI gate')
need(strict_java, ':app:compileDebugJavaWithJavac', 'strict Java Gradle compile task')
app_gradle=read(Path('android/app/build.gradle.kts'))
need(app_gradle, 'options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))', 'strict Java warnings-as-errors')
need(app_gradle, 'androidResources.ignoreAssetsPattern = "__vibecoder_aapt_ignore_none__"', 'AAPT transparent OmniRoute asset sentinel')
need(read(Path('scripts/part34_alpha_build_and_verify.sh')), 'omniroute-aapt-policy', 'pre-Gradle OmniRoute AAPT transparency gate')
need(read(Path('scripts/verify_omniroute_aapt_asset_policy.py')), 'omniroute_aapt_policy_would_drop_runtime_entries', 'AAPT runtime collision failure token')
need(read(Path('scripts/verify_omniroute_aapt_asset_policy.py')), 'omniroute_gradle_asset_metadata_would_be_dropped', 'Gradle default-exclude metadata preflight token')
need(read(Path('scripts/omniroute_android_packaging_metadata_policy.py')), '".gitattributes"', 'Gradle default-exclude metadata policy')
need(apk_verify, 'vibecoder-part34-apk-asset-diff.json', 'APK OmniRoute mismatch diagnostics artifact')

for token in (
    'full-alpha-package:',
    'needs: [jcode-android-proof-build, node-runtime-ready]',
    'actions/setup-node@v6.4.0',
    'node-version: "24.19.0"',
    'package-manager-cache: false',
    'actions/download-artifact@v8.0.1',
    'bash scripts/fetch_omniroute_reviewed_archive.sh',
    'bash scripts/part34_alpha_build_and_verify.sh',
    'vibecoder-part34-full-alpha-apk',
    'python3 scripts/test_part34_10_alpha_package_tools.py'):
    need(workflow, token, 'base Alpha CI lane with Node setup download')
for token in (
    'libvibecoder_jcode_exec.so',
    'build_omniroute_android_bundle.py', 'stage_omniroute_android_asset.py',
    'build_android_host.sh', 'build_android_shell.sh',
    'verify_android_diagnostic_apk.sh" "$APK" alpha',
    'write_alpha_build_evidence.py'):
    need(alpha_build, token, 'base Alpha build script')
if 'node_payload_not_staged' in alpha_build or 'libvibecoder_node_exec.so' in alpha_build:
    raise SystemExit('Part 34.10.15 base Alpha must not require bundled Node payload')
for token in (
    'MODE" == "jcode" || "$MODE" == "alpha"',
    'MODE" == "node" || "$MODE" == "omniroute_service"',
    'MODE" == "omniroute_asset" || "$MODE" == "omniroute_service"'):
    need(apk_verify, token, 'full Alpha APK payload verifier')
for token in (
    'SOURCE_REF="release/v3.8.50"',
    'REVIEWED_COMMIT="ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"',
    'EXPECTED_SHA256="1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7"',
    'URL="${REPO}/archive/${REVIEWED_COMMIT}.zip"',
    'comment != commit'):
    need(omni_fetch, token, 'exact reviewed OmniRoute archive fetch')
for token in ('jcode_build_evidence_payload_mismatch', 'payload_bound_to_proof_evidence',
              'github_release_packageinstaller_split'):
    need(alpha_package_regression, token, 'Alpha package evidence regression')
for token in ('base_alpha_apk_with_direct_node_setup_download_not_device_execution',
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
 'coding_agent_send_ui_wired':True,'omniroute_auto_install_on_app_open_wired':True,
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
 'full_alpha_build_lane_ready':True,'full_alpha_requires_jcode_node_omniroute_same_apk':False,
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
if project.get('status') != 'node_http_404_release_race_repaired_publication_proof_pending':
    raise SystemExit('Part 34.10.18 PROJECT_STATE status mismatch')
if project.get('step') != '34.10.18-alpha-node-publication-race-repair':
    raise SystemExit('Part 34.10.18 PROJECT_STATE step mismatch')
expected_state={
 'portrait_layout':True,'drawer_old_chats':True,'drawer_reads_persisted_conversations_only':True,
 'drawer_mutates_conversation_store':False,'preview_placeholder_only':True,
 'fake_model_replies':False,'send_bridge_wired':True,'stop_bridge_wired':True,
 'new_chat_bridge_wired':True,'normal_conversation_send_wired':True,
 'coding_agent_send_ui_wired':True,'omniroute_auto_install_wired':True,
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
 'full_alpha_build_lane_ready':True,'full_alpha_requires_jcode_node_omniroute_same_apk':False,
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
if state.get('status') != 'node_http_404_release_race_repaired_publication_proof_pending':
    raise SystemExit('Part 34.10.18 PART34_STATE status mismatch')
if state.get('step') != '34.10.18-alpha-node-publication-race-repair':
    raise SystemExit('Part 34.10.18 PART34_STATE step mismatch')

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
for token in ('-mbranch-protection=standard','target_makefiles_modified',
              'node_android_host_makefile_sanitize','sanitization_mode','already_clean','removed_proven_flags'):
    need(node_sanitize, token, 'Node Android idempotent proven host-only flag sanitizer')
for token in ('ProcessRuntime, ProcessTermination','self.process_runtime.cancel(process_id)'):
    need(omni_service, token, 'OmniRoute ProcessRuntime compile repair')
if 'last_profile' in omni_service:
    raise SystemExit('Part 34.10 OmniRoute known unused last_profile warning regressed')
if 'value.chars().any(char::is_control)' not in process_local or 'is_forbidden_control' in process_local:
    raise SystemExit('Part 34.10 runtime-service control predicate compile repair regressed')
if 'GatewayChatMessage' in gateway_chat.split('#[cfg(test)]',1)[0]:
    raise SystemExit('Part 34.10 gateway test-only import warning regressed')
for token in ('vibecoder-node-24.19.0-android-zlib-cpufeatures-v2',
              '<(ZLIB_ROOT)/vibecoder-android-cpufeatures/cpu-features.c',
              '<(ZLIB_ROOT)/vibecoder-android-cpufeatures',
              'node_android_cpufeatures_patch_already_applied'):
    need(node_cpufeatures_patch, token, 'Node Android zlib cpufeatures linkage patch')
for token in ('patch_node_android_zlib_cpufeatures.py', 'android_ndk_cpufeatures_source_missing:',
              'node_android_cpufeatures_patch_failed:'):
    need(node_provision, token, 'Node Android cpufeatures provision contract')
for token in ('node_android_cpufeatures_generated_graph', 'cpufeatures_object_missing_from_target_graph',
              'cpufeatures_object_leaked_into_host_graph', 'OBJECT_TOKEN', 'TARGET_RE'):
    need(node_cpufeatures_graph, token, 'Node Android generated cpufeatures graph verifier')
for token in ('verify_node_android_cpufeatures_integration.py', 'node_android_cpufeatures_generated_graph_invalid'):
    need(node_provision, token, 'Node Android generated cpufeatures graph provision guard')
for token in ('scripts/sanitize_node_android_host_makefiles.py',
              'python3 scripts/test_part34_3_service_tools.py',
              'python3 scripts/test_part34_10_compile_repairs.py'):
    need(workflow, token, 'deep-audit CI regression')

for record,label in ((project,'PROJECT_STATE'),(state,'PART34_STATE')):
    # Long-lived capability/repair flags through the 34.10.14 CI-only repair.
    for key in ('node_cpufeatures_generated_graph_guard_added','deep_source_compile_contract_audit_passed',
                'node_cpufeatures_generated_graph_false_negative_repaired',
                'latest_ci_minimal_apk_proven','latest_ci_jcode_apk_proven',
                'real_chat_interaction_source_wired','model_request_pipeline_source_wired',
                'agent_action_controller_source_implemented','node_cpufeatures_generated_graph_ci_proven',
                'node_cpufeatures_relative_staging_fix_applied',
                'node_cpufeatures_absolute_object_regression_guard_added',
                'latest_ci_node_absolute_cpufeatures_object_failure_observed',
                'coding_agent_send_ui_wired','agent_action_checkpoint_store_wired',
                'agent_action_auto_routing_conservative','agent_action_stop_cancellation_wired',
                'agent_action_route_exact_selected_model_only','android_renameat2_flags_type_fixed',
                'latest_ci_minimal_post_routing_compile_passed','latest_ci_jcode_post_routing_compile_passed',
                'latest_ci_node_relative_cpufeatures_graph_proven',
                'node_configure_host_toolchain_bound_before_configure',
                'node_configure_separate_host_toolset_required',
                'node_generated_host_arch_graph_guard_added','node_generated_arm64_push_register_host_rejected',
                'latest_ci_node_configure_host_arch_verified','latest_ci_node_host_arch_graph_verified',
                'latest_ci_node_cpufeatures_graph_verified','node_host_flag_sanitizer_clean_noop_allowed',
                'node_host_flag_sanitizer_idempotent','latest_ci_node_sanitizer_clean_noop_passed',
                'latest_ci_node_compile_started','latest_ci_node_canceled_by_240_minute_timeout',
                'latest_ci_node_active_compile_at_cancel'):
        if record.get(key) is not True:
            raise SystemExit(f'Part 34.10.14 {label} repair/evidence flag missing: {key}')
    if record.get('latest_ci_node_sanitizer_already_clean_rejected') is not False:
        raise SystemExit(f'Part 34.10.14 {label} stale sanitizer failure state')
    if record.get('agent_action_outer_loop_auto_enabled') is not False:
        raise SystemExit(f'Part 34.10.14 {label} must not auto-enable explicit outer loop')
    if record.get('jcode_diagnostic_apk_full_chat_expected') is not False:
        raise SystemExit(f'Part 34.10.14 {label} diagnostic APK/full Alpha boundary mismatch')
    if record.get('node_cpufeatures_generated_graph_expected_object') != 'deps/zlib/vibecoder-android-cpufeatures/cpu-features.o':
        raise SystemExit(f'Part 34.10.14 {label} generated cpufeatures object token mismatch')
    if record.get('latest_ci_run_id') != 31969047075 or record.get('latest_ci_commit') != 'b389e5d37535a75376fdf97047d0b14da45753c5':
        raise SystemExit(f'Part 34.10.14 {label} latest CI identity mismatch')
    if record.get('latest_ci_minimal_apk_sha256') != 'e6439a33cbafde7314075d9e3be3277338b1c4d403c94f5d9797f708777be3bf':
        raise SystemExit(f'Part 34.10.14 {label} latest Minimal APK evidence mismatch')
    if record.get('latest_ci_jcode_apk_sha256') != 'a99ee66ab6491199adbc8a00db00b4ced4b2dedc08cbafcd59410e17411ed633':
        raise SystemExit(f'Part 34.10.14 {label} latest Jcode APK evidence mismatch')
    if record.get('latest_ci_minimal_apk_artifact_id') != 9269309612 or record.get('latest_ci_jcode_apk_artifact_id') != 9269571797:
        raise SystemExit(f'Part 34.10.14 {label} latest APK artifact identity mismatch')
    if record.get('latest_ci_node_failure_artifact_id') is not None:
        raise SystemExit(f'Part 34.10.14 {label} cancelled Node run must not claim failure artifact')
    if record.get('latest_ci_node_failure') != 'github_job_timeout_during_active_node_compile':
        raise SystemExit(f'Part 34.10.14 {label} latest Node timeout identity mismatch')
    if record.get('latest_ci_node_compile_error_before_timeout') is not False:
        raise SystemExit(f'Part 34.10.14 {label} must not invent a compiler/linker failure')
    if record.get('latest_ci_node_host_arch_misdetected_as_arm64') is not False:
        raise SystemExit(f'Part 34.10.14 {label} repaired host architecture regressed')
    if record.get('latest_ci_node_host_compiler') != '/usr/bin/g++':
        raise SystemExit(f'Part 34.10.14 {label} host compiler evidence mismatch')
    if record.get('latest_ci_node_host_push_register_source') != 'deps/v8/src/heap/base/asm/x64/push_registers_asm.cc':
        raise SystemExit(f'Part 34.10.14 {label} x64 host push-register evidence mismatch')
    if record.get('node_configure_host_arch_expected') != 'x64' or record.get('node_configure_target_arch_expected') != 'arm64':
        raise SystemExit(f'Part 34.10.14 {label} configure arch contract mismatch')
    if record.get('node_generated_host_push_register_expected') != 'deps/v8/src/heap/base/asm/x64/push_registers_asm.o':
        raise SystemExit(f'Part 34.10.14 {label} generated host graph object contract mismatch')
    if record.get('node_ci_timeout_minutes') != 360 or record.get('node_ci_build_jobs') != 4:
        raise SystemExit(f'Part 34.10.14 {label} Node CI throughput/timeout contract mismatch')
    if record.get('node_runtime_http_404_bounded_retry') is not True:
        raise SystemExit(f'Part 34.10.17 {label} bounded HTTP 404 retry missing')
    if record.get('node_runtime_http_404_retry_attempts') != 6:
        raise SystemExit(f'Part 34.10.17 {label} HTTP 404 retry-attempt contract mismatch')
    if record.get('node_runtime_release_public_before_alpha_required') is not True:
        raise SystemExit(f'Part 34.10.17 {label} Alpha public-runtime gate missing')
    if record.get('node_runtime_post_publish_alpha_dispatch') is not True:
        raise SystemExit(f'Part 34.10.17 {label} post-publish Alpha dispatch missing')
    if record.get('alpha_node_release_race_repaired') is not True:
        raise SystemExit(f'Part 34.10.18 {label} Alpha/Node release race repair missing')
    if record.get('alpha_runtime_gate_nonfailing_until_public') is not True:
        raise SystemExit(f'Part 34.10.18 {label} non-failing publication gate missing')
    if record.get('alpha_runtime_gate_skips_packaging_when_unready') is not True:
        raise SystemExit(f'Part 34.10.18 {label} unready Alpha skip contract missing')
    if record.get('latest_alpha_publication_race_ci_commit') != 'ac92cf92bf5ab2345480a905d580a2b1b905db75':
        raise SystemExit(f'Part 34.10.18 {label} observed race commit mismatch')
    if record.get('latest_alpha_publication_race_http_status') != 404:
        raise SystemExit(f'Part 34.10.18 {label} observed HTTP status mismatch')
    if record.get('latest_alpha_publication_race_attempts') != 6:
        raise SystemExit(f'Part 34.10.18 {label} observed attempt count mismatch')
    if record.get('node_runtime_http_404_device_retest_pending') is not True:
        raise SystemExit(f'Part 34.10.17 {label} must preserve pending device proof')
    if record.get('status') != 'node_http_404_release_race_repaired_publication_proof_pending':
        raise SystemExit(f'Part 34.10.18 {label} status mismatch')
    if record.get('step') != '34.10.18-alpha-node-publication-race-repair':
        raise SystemExit(f'Part 34.10.18 {label} step mismatch')
    if record.get('fresh_android_compile') is not False or record.get('full_alpha_apk_compiled') is not False:
        raise SystemExit(f'Part 34.10.14 {label} overclaims post-repair compile/full Alpha proof')

# Historical evidence remains immutable and is validated independently of the current state.
if deep_audit.get('node_cpufeatures_generated_graph_guard_added') is not True or deep_audit.get('fresh_android_compile') is not False:
    raise SystemExit('Part 34.10.8 historical deep source audit evidence mismatch')
if graph_repair.get('step') != '34.10.9-node-gyp-graph-verifier-repair':
    raise SystemExit('Part 34.10.9 historical graph repair evidence identity mismatch')
if latest_repair.get('step') != '34.10.10-node-relative-cpufeatures-agent-routing':
    raise SystemExit('Part 34.10.10 latest repair evidence identity mismatch')
observed_3410=latest_repair.get('latest_ci_observations',{})
if observed_3410.get('minimal_apk_build_passed') is not True or observed_3410.get('jcode_apk_build_passed') is not True:
    raise SystemExit('Part 34.10.10 positive Minimal/Jcode CI evidence missing')
if observed_3410.get('node_generated_cpufeatures_graph_verifier_passed') is not True:
    raise SystemExit('Part 34.10.10 Node generated graph positive evidence missing')
if observed_3410.get('node_target_compile_count_before_failure') != 1121:
    raise SystemExit('Part 34.10.10 Node target compile-count evidence mismatch')
node_repair=latest_repair.get('node_repair',{})
if node_repair.get('expected_generated_object') != 'deps/zlib/vibecoder-android-cpufeatures/cpu-features.o':
    raise SystemExit('Part 34.10.10 relative cpufeatures object evidence mismatch')
if node_repair.get('absolute_ndk_object_graph_rejected') is not True:
    raise SystemExit('Part 34.10.10 absolute NDK object regression guard evidence missing')
routing=latest_repair.get('agent_action_routing',{})
for key in ('normal_model_chat_path_preserved','clear_coding_mutation_requests_route_to_single_agent_action_turn',
            'explanation_style_questions_remain_model_chat','checkpoint_store_attached_to_android_core',
            'agent_action_stop_cancellation_wired','exact_selected_model_policy_no_fallbacks','response_reports_turn_kind'):
    if routing.get(key) is not True:
        raise SystemExit(f'Part 34.10.10 agent-routing evidence missing: {key}')
if routing.get('explicit_outer_loop_auto_enabled') is not False:
    raise SystemExit('Part 34.10.10 agent routing overclaims/auto-enables explicit loop')
if latest_repair.get('post_fix_compile_claim') is not False or latest_repair.get('external_ci_recompile_required') is not True:
    raise SystemExit('Part 34.10.10 repair evidence overclaims external compile proof')

# Part 34.10.11 historical compiler evidence and ABI repair.
need(checkpoint_local, 'libc::RENAME_EXCHANGE as libc::c_uint', 'Android renameat2 flags type repair')
if 'timeout-minutes: 360' not in node_runtime_workflow:
    raise SystemExit('Part 34.10.14 historical Node CI timeout contract missing from dedicated runtime workflow')
if latest_compile_repair.get('step') != '34.10.11-android-libc-node-timeout-repair':
    raise SystemExit('Part 34.10.11 latest compile repair evidence identity mismatch')
obs=latest_compile_repair.get('latest_ci_observations',{})
if obs.get('node_generated_cpufeatures_graph_verified') is not True or obs.get('node_compile_or_link_error_before_cancel') is not False:
    raise SystemExit('Part 34.10.11 Node timeout evidence mismatch')
if obs.get('minimal_lane_result') != 'failed_before_apk' or obs.get('jcode_lane_result') != 'failed_before_apk':
    raise SystemExit('Part 34.10.11 Android lane failure evidence mismatch')
rep=latest_compile_repair.get('repairs',{})
if rep.get('renameat2_flags') != 'libc::RENAME_EXCHANGE as libc::c_uint' or rep.get('node_job_timeout_minutes_after') != 240:
    raise SystemExit('Part 34.10.11 repair evidence mismatch')
if latest_compile_repair.get('post_fix_compile_claim') is not False or latest_compile_repair.get('external_ci_recompile_required') is not True:
    raise SystemExit('Part 34.10.11 evidence overclaims post-fix compiler proof')

# Part 34.10.12: bind the host compiler before Node configure/GYP architecture detection, then
# reject a generated obj.host graph that selects ARM64 push-register assembly on the x86_64 runner.
for token in ('CC_host="$HOST_CC" CXX_host="$HOST_CXX" AR_host="$HOST_AR"',
              'verify_node_android_host_arch_graph.py', 'node_android_host_arch_graph_invalid'):
    need(node_provision, token, 'Node configure-time host architecture repair')
for token in ("host_arch') != 'x64'", "target_arch') != 'arm64'", "want_separate_host_toolset') != 1",
              'host_arch_mismatch', 'target_arch_mismatch', 'want_separate_host_toolset_mismatch'):
    need(node_configure_verify, token, 'Node configure output host/target architecture verifier')
for token in ('node_android_host_arch_graph', 'v8_base_without_compiler.host.mk',
              'deps/v8/src/heap/base/asm/x64/push_registers_asm.o',
              'arm64_push_register_leaked_into_host_graph', 'host_push_register_arch_mismatch'):
    need(node_host_arch_graph, token, 'Node generated V8 host architecture graph verifier')
for token in ('node_configure_time_host_toolchain_binding_missing',
              'node_configure_arm64_host_misdetection_not_rejected',
              'node_arm64_push_register_host_graph_not_rejected'):
    need(compile_repairs, token, 'Node configure host-arch regression coverage')

if latest_host_arch_repair.get('step') != '34.10.12-node-configure-host-arch-repair' or latest_host_arch_repair.get('status') != 'node_configure_host_arch_recompile_pending':
    raise SystemExit('Part 34.10.12 repair evidence identity mismatch')
obs12=latest_host_arch_repair.get('latest_ci_observations',{})
if obs12.get('minimal_lane_result') != 'passed' or obs12.get('jcode_lane_result') != 'passed':
    raise SystemExit('Part 34.10.12 positive Minimal/Jcode CI evidence missing')
if obs12.get('node_timed_out') is not False or obs12.get('node_failure_classification') != 'compiler_or_linker_failed':
    raise SystemExit('Part 34.10.12 Node failure classification mismatch')
if obs12.get('node_host_compiler') != '/usr/bin/g++' or obs12.get('node_host_source') != 'deps/v8/src/heap/base/asm/arm64/push_registers_asm.cc':
    raise SystemExit('Part 34.10.12 Node host architecture failure evidence mismatch')
if obs12.get('node_host_compile_count_before_failure') != 1795 or obs12.get('node_target_compile_count_before_failure') != 2125:
    raise SystemExit('Part 34.10.12 Node compile-count evidence mismatch')
if obs12.get('node_generated_cpufeatures_graph_verified') is not True:
    raise SystemExit('Part 34.10.12 cpufeatures graph regression evidence missing')
rep12=latest_host_arch_repair.get('repairs',{})
for key in ('configure_time_host_cc_bound','configure_time_host_cxx_bound','configure_time_host_ar_bound',
            'configure_output_requires_host_arch_x64','configure_output_requires_target_arch_arm64',
            'configure_output_requires_separate_host_toolset','generated_v8_host_graph_guard_added',
            'arm64_push_register_host_graph_rejected'):
    if rep12.get(key) is not True:
        raise SystemExit(f'Part 34.10.12 repair evidence missing: {key}')
if rep12.get('expected_host_push_register_object') != 'deps/v8/src/heap/base/asm/x64/push_registers_asm.o':
    raise SystemExit('Part 34.10.12 expected host push-register object mismatch')
for key in ('agent_action_routing_changed','jcode_build_script_changed','minimal_build_lane_changed',
            'node_cpufeatures_patch_changed','node_timeout_minutes_changed','vendored_jcode_changed'):
    if rep12.get(key) is not False:
        raise SystemExit(f'Part 34.10.12 protected path/change boundary mismatch: {key}')
if latest_host_arch_repair.get('post_fix_compile_claim') is not False or latest_host_arch_repair.get('external_ci_recompile_required') is not True:
    raise SystemExit('Part 34.10.12 evidence overclaims post-fix compiler proof')

# Part 34.10.13: after the host-architecture repair, an already-clean host graph is valid.
# The sanitizer must remain narrow, idempotent, and target-byte-preserving.
for token in ('sanitization_mode', 'already_clean', 'removed_proven_flags'):
    need(node_sanitize, token, 'Node clean-host sanitizer repair')
for token in ('host_flag_sanitizer_clean_graph_rejected', 'host_flag_sanitizer_clean_graph_evidence_invalid'):
    need(compile_repairs, token, 'Node clean-host sanitizer regression coverage')
if latest_sanitizer_repair.get('step') != '34.10.13-node-clean-host-sanitizer-repair' or latest_sanitizer_repair.get('status') != 'node_clean_host_sanitizer_recompile_pending':
    raise SystemExit('Part 34.10.13 repair evidence identity mismatch')
obs13=latest_sanitizer_repair.get('latest_ci_observations',{})
if obs13.get('run_id') != 31966423971 or obs13.get('commit') != '0499266c52585074495104fb8b2567cb65dc7fed':
    raise SystemExit('Part 34.10.13 latest CI identity mismatch')
if obs13.get('minimal_lane_result') != 'passed' or obs13.get('jcode_lane_result') != 'passed':
    raise SystemExit('Part 34.10.13 positive Minimal/Jcode CI evidence missing')
if obs13.get('node_configure_host_arch') != 'x64' or obs13.get('node_configure_target_arch') != 'arm64' or obs13.get('node_separate_host_toolset') is not True:
    raise SystemExit('Part 34.10.13 repaired configure architecture evidence mismatch')
if obs13.get('node_generated_host_arch_graph_verified') is not True or obs13.get('node_generated_host_push_register_arches') != ['x64']:
    raise SystemExit('Part 34.10.13 generated host graph evidence mismatch')
if obs13.get('node_generated_cpufeatures_graph_verified') is not True:
    raise SystemExit('Part 34.10.13 cpufeatures graph regression evidence missing')
if obs13.get('node_compile_started') is not False or obs13.get('node_failure_classification') != 'host_target_flag_sanitize_failed':
    raise SystemExit('Part 34.10.13 Node failure phase/classification mismatch')
if obs13.get('node_failure_detail') != 'proven_host_target_flag_not_found':
    raise SystemExit('Part 34.10.13 sanitizer false-failure detail mismatch')
rep13=latest_sanitizer_repair.get('repair',{})
for key in ('sanitizer_is_idempotent','clean_host_graph_is_valid_noop','proven_flag_removed_when_present',
            'post_scan_requires_proven_flag_absent_from_all_host_makefiles','target_makefiles_hashed_before_after',
            'target_makefiles_must_remain_byte_identical'):
    if rep13.get(key) is not True:
        raise SystemExit(f'Part 34.10.13 repair evidence missing: {key}')
if rep13.get('sanitization_modes') != ['removed_proven_flags','already_clean']:
    raise SystemExit('Part 34.10.13 sanitizer mode evidence mismatch')
for key in ('node_source_changed','node_cpufeatures_patch_changed','node_host_arch_repair_changed',
            'jcode_build_script_changed','minimal_build_lane_changed','agent_action_routing_changed',
            'vendored_jcode_changed','node_timeout_minutes_changed'):
    if rep13.get(key) is not False:
        raise SystemExit(f'Part 34.10.13 protected change boundary mismatch: {key}')
if latest_sanitizer_repair.get('post_fix_compile_claim') is not False or latest_sanitizer_repair.get('external_ci_recompile_required') is not True:
    raise SystemExit('Part 34.10.13 evidence overclaims post-fix compiler proof')

# Part 34.10.14: the 34.10.13 repair reached sustained real compilation and was cancelled only
# by the 240-minute job ceiling. Increase CI throughput/ceiling without touching runtime architecture.
if latest_ci_throughput_repair.get('step') != '34.10.14-node-ci-throughput-timeout-repair' or latest_ci_throughput_repair.get('status') != 'node_ci_throughput_timeout_recompile_pending':
    raise SystemExit('Part 34.10.14 repair evidence identity mismatch')
obs14=latest_ci_throughput_repair.get('latest_ci_observations',{})
if obs14.get('run_id') != 31969047075 or obs14.get('commit') != 'b389e5d37535a75376fdf97047d0b14da45753c5':
    raise SystemExit('Part 34.10.14 latest CI identity mismatch')
if obs14.get('minimal_lane_result') != 'passed' or obs14.get('jcode_lane_result') != 'passed':
    raise SystemExit('Part 34.10.14 positive Minimal/Jcode CI evidence missing')
if obs14.get('node_configure_host_arch') != 'x64' or obs14.get('node_configure_target_arch') != 'arm64' or obs14.get('node_separate_host_toolset') is not True:
    raise SystemExit('Part 34.10.14 configure host/target evidence mismatch')
if obs14.get('node_generated_host_arch_graph_verified') is not True or obs14.get('node_generated_host_push_register_arches') != ['x64']:
    raise SystemExit('Part 34.10.14 generated host graph evidence mismatch')
if obs14.get('node_generated_cpufeatures_graph_verified') is not True or obs14.get('node_sanitizer_clean_noop_passed') is not True:
    raise SystemExit('Part 34.10.14 precompile repair regression evidence missing')
if obs14.get('node_compile_started') is not True or obs14.get('node_compile_or_link_error_before_cancel') is not False:
    raise SystemExit('Part 34.10.14 sustained compile evidence mismatch')
if obs14.get('node_cancel_reason') != 'github_job_timeout_240_minutes' or obs14.get('node_active_compiler_process_at_cancel') is not True:
    raise SystemExit('Part 34.10.14 timeout classification mismatch')
rep14=latest_ci_throughput_repair.get('repair',{})
if rep14.get('scope') != 'ci_execution_only' or rep14.get('node_build_jobs_before') != 2 or rep14.get('node_build_jobs_after') != 4:
    raise SystemExit('Part 34.10.14 parallelism repair evidence mismatch')
if rep14.get('node_job_timeout_minutes_before') != 240 or rep14.get('node_job_timeout_minutes_after') != 360:
    raise SystemExit('Part 34.10.14 timeout repair evidence mismatch')
for key in ('node_source_changed','node_cpufeatures_patch_changed','node_host_arch_repair_changed','node_sanitizer_changed',
            'android_or_rust_app_source_changed','jcode_build_script_changed','vendored_jcode_changed','omniroute_changed',
            'agent_action_routing_changed','runtime_packaging_architecture_changed'):
    if rep14.get(key) is not False:
        raise SystemExit(f'Part 34.10.14 architecture/protected path changed unexpectedly: {key}')
if latest_ci_throughput_repair.get('post_fix_compile_claim') is not False or latest_ci_throughput_repair.get('external_ci_recompile_required') is not True:
    raise SystemExit('Part 34.10.14 evidence overclaims post-fix compiler proof')


feature_manifest=(ROOT/'android/node_runtime/src/main/AndroidManifest.xml').read_text()
delivery_manager=(ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
setup_ui=(ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeSetupUi.java').read_text()
base_gradle=(ROOT/'android/app/build.gradle.kts').read_text()
base_manifest=(ROOT/'android/app/src/main/AndroidManifest.xml').read_text()
settings_gradle=(ROOT/'android/settings.gradle.kts').read_text()
if '<dist:on-demand' in feature_manifest or '<dist:install-time>' not in feature_manifest:
    raise SystemExit('Part 34.10.15 direct-download runtime split manifest invalid')
for token in ('PackageInstaller.SessionParams.MODE_INHERIT_EXISTING', 'Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES',
              'RUNTIME_URL', 'getPackageArchiveInfo', 'PackageInstaller.STATUS_PENDING_USER_ACTION'):
    need(delivery_manager, token, 'Node direct-download PackageInstaller manager')
need(setup_ui, 'Download & Set Up Node.js', 'Node direct-download setup UI')
need(base_gradle, 'dynamicFeatures += setOf(":node_runtime")', 'downloadable node runtime split build registration')
need(settings_gradle, 'include(":node_runtime")', 'node runtime split module inclusion')
need(base_manifest, 'android.permission.REQUEST_INSTALL_PACKAGES', 'runtime split installer permission')
for forbidden in ('com.google.android.play:feature-delivery', 'SplitCompatApplication', 'SplitInstallManager'):
    if forbidden in base_gradle + base_manifest + delivery_manager:
        raise SystemExit('Part 34.10.15 Play runtime dependency survived: ' + forbidden)
if (ROOT/'.github/workflows/android-play-bundle.yml').exists():
    raise SystemExit('Part 34.10.15 Play bundle workflow must be absent during development')
print('Part 34.10.18 Alpha/Node publication-race recovery validation PASSED')
