#!/usr/bin/env python3
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
ui=(ROOT/'android/app/src/main/java/com/vibecoder/shell/AlphaWorkspaceUi.java').read_text()
activity=(ROOT/'android/app/src/main/java/com/vibecoder/shell/MainActivity.java').read_text()
bridge=(ROOT/'android/app/src/main/java/com/vibecoder/shell/NativeBridge.java').read_text()
native=(ROOT/'android/app/src/main/cpp/native_bridge.c').read_text()
host=(ROOT/'crates/vibecoder-android-host/src/app_controller_ffi.rs').read_text()
core=(ROOT/'crates/vibecoder-core/src/lib.rs').read_text()
installer=(ROOT/'android/app/src/main/java/com/vibecoder/shell/OmniRouteAssetInstaller.java').read_text()
manifest=(ROOT/'android/app/src/main/AndroidManifest.xml').read_text()
workflow=(ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
strict=(ROOT/'scripts/part34_10_strict_java_compile.sh').read_text()
alpha_build=(ROOT/'scripts/part34_alpha_build_and_verify.sh').read_text()
alpha_evidence=(ROOT/'scripts/write_alpha_build_evidence.py').read_text()
omni_fetch=(ROOT/'scripts/fetch_omniroute_reviewed_archive.sh').read_text()
apk_verify=(ROOT/'scripts/verify_android_diagnostic_apk.sh').read_text()
device_harness=(ROOT/'scripts/test_android_diagnostic_device.sh').read_text()

# Read-only app-private restored-chat surface and identity/path hardening.
assert 'file.isFile()' in ui and 'isSymlink(file)' in ui
assert 'canonical.getParentFile().equals(canonicalRoot)' in ui
assert 'UUID.fromString(projectText)' in ui and 'UUID.fromString(conversationText)' in ui
assert 'conversationJsonIsValidForDisplay' in ui
assert 'projectText.equals(json.optString("project_id", ""))' in ui
assert 'conversationText.equals(json.optString("conversation_id", ""))' in ui
assert 'sessionCreationPending != agentSessionMissing' in ui

# Persistence/UI bounds must not silently discard a Rust-valid 16 MiB conversation.
assert 'MAX_CHAT_FILE_BYTES = 16 * 1024 * 1024' in ui
assert 'output.size() + read > MAX_CHAT_FILE_BYTES' in ui
assert ui.index('candidates.sort(Comparator.comparingLong(File::lastModified).reversed())') < ui.index('if (entries.size() >= MAX_CHAT_FILES) break;')
assert 'MAX_RENDERED_TEXT_BYTES = 512 * 1024' in ui
assert 'MAX_RENDERED_SINGLE_MESSAGE_BYTES = 64 * 1024' in ui
assert 'truncateUtf8ForDisplay' in ui
assert 'Persisted history was not modified.' in ui
assert 'truncateUtf8ForDisplay(text, MAX_RENDERED_SINGLE_MESSAGE_BYTES)' in ui

# Disk/JSON work stays off the main thread; active turns block chat switching.
assert 'Executors.newSingleThreadExecutor()' in ui
assert 'chatIoExecutor.execute' in ui and 'activity.runOnUiThread' in ui
assert 'turnRunning || preparingChat' in ui
assert 'conversationBlocked' in ui
assert 'callbacks.onConversationSelectionCleared()' in ui
assert 'This saved chat needs recovery before another message can be sent.' in ui
assert 'alphaWorkspaceUi.destroy();' in activity

# App-open automatic OmniRoute install/start + attestation + Rust controller bootstrap.
assert 'startChatRuntime();' in activity
assert 'OmniRouteAssetInstaller.ensureInstalled' in activity
assert 'NativeBridge.nativeOmniRouteStart(' in activity
assert 'runtime_profile_round_trip_proven' in activity
assert 'exact_model_only' in activity and 'hidden_model_reroutes_disabled' in activity
assert 'NativeBridge.nativeAppControllerInit(' in activity
assert 'for (int attempt = 0; attempt < 4; attempt++)' in activity
assert 'Thread.sleep(250L)' in activity
assert 'catalogMayStillBeWarming' in activity
assert 'no usable model/provider available' in activity
assert 'if (shouldAutoRunDiagnostics())' in activity
assert 'getBooleanExtra("vibecoder_diagnostic_test", false)' in activity
assert 'startChatRuntime();\n        runDiagnostics();' not in activity
assert '--ez vibecoder_diagnostic_test true' in device_harness
assert '|| "$MODE" == "alpha"' in device_harness
assert "if mode in ('jcode', 'alpha'):" in device_harness
assert "if mode == 'node':" in device_harness
assert "if mode in ('omniroute_asset', 'alpha'):" in device_harness
assert "if mode in ('omniroute_service', 'omniroute_gateway', 'omniroute_inference'):" in device_harness

# New Chat / Send / Stop are real JNI calls, not UI fabrications.
assert 'NativeBridge.nativeChatCreate()' in activity
assert 'NativeBridge.nativeChatSend(' in activity
assert 'NativeBridge.nativeChatCancel(' in activity
assert 'chatTurnRunning.compareAndSet(false, true)' in activity
assert 'chatStopRequested.set(true)' in activity
assert 'cancelExecutor' in activity
assert 'Chat controller UI bridge is not connected yet' not in activity
assert 'nativeChatCreate()' in bridge and 'nativeChatSend(' in bridge and 'nativeChatCancel(' in bridge

# Rust owns durable conversation + exact-model selection; no silent fallback/autoloop is introduced.
assert 'models.retain(|model| android_chat_model_id_usable(&model.id))' in host
assert 'fn android_chat_model_id_usable(value: &str) -> bool' in host
assert 'value.len() <= 512' in host and 'byte.is_ascii_graphic()' in host
assert 'models.sort_by(|left, right| left.id.cmp(&right.id))' in host
assert 'run_persisted_model_conversation_turn_cancellable' in host
assert 'ConversationModelTurnCancellation' in core
assert 'await_conversation_model_inference_or_cancel' in core
assert 'tokio::time::timeout(Duration::from_millis(50)' in core
assert 'tokio::select!' not in core

# Standard UTF-8 model output must not be passed to JNI NewStringUTF as arbitrary bytes.
assert 'utf8_bytes_to_jstring' in native
chat_tail=native[native.index('static jstring chat_call_simple'):]
assert 'utf8_bytes_to_jstring(env, buffer, (size_t) written)' in chat_tail
assert 'NewStringUTF(env, (const char *) buffer)' not in chat_tail
assert 'CHAT_JSON_CAPACITY (2u * 1024u * 1024u)' in native

# targetSdk 36 system UI safety + predictive Back for the custom drawer.
assert 'installSystemUiInsets()' in ui
assert 'WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout()' in ui
assert 'WindowInsets.Type.ime()' in ui
assert 'Api33Back.register(activity, this::closeDrawer)' in ui
assert 'OnBackInvokedDispatcher.PRIORITY_OVERLAY' in ui
assert 'android:windowSoftInputMode="adjustResize"' in manifest

# Preview remains honest and unsupported feature labels stay absent.
assert 'Preview not active yet' in ui
for forbidden in ('Deployed to staging','Build successful','Firebase','MCP connected','bash ready','Browser automation'):
    assert forbidden not in ui

# Strict source gate must catch warnings that Gradle's ordinary compile can otherwise tolerate.
assert 'FileLock installLock = lockChannel.lock()' in installer
assert 'installLock.isValid()' in installer
assert ':app:compileDebugJavaWithJavac' in strict
assert 'options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))' in (ROOT/'android/app/build.gradle.kts').read_text()
assert 'platforms/android-36/android.jar' in strict
assert 'bash scripts/part34_10_strict_java_compile.sh' in workflow

# Development Alpha is directly installable: Jcode + OmniRoute + proven Node are packaged together.
assert 'node-android-proof-build:' in workflow
assert 'full-alpha-package:' in workflow
assert 'needs: [jcode-android-proof-build, node-android-proof-build]' in workflow
assert 'Development Alpha APK (Jcode + OmniRoute + Node packaged)' in workflow
assert 'vibecoder-part34-development-alpha-apk' in workflow
assert 'actions/setup-node@v6.4.0' in workflow
assert 'package-manager-cache: false' in workflow
assert workflow.count('actions/download-artifact@v8.0.1') >= 2
assert 'bash scripts/fetch_omniroute_reviewed_archive.sh' in workflow
assert 'bash scripts/part34_alpha_build_and_verify.sh' in workflow
for token in ('libvibecoder_jcode_exec.so','libvibecoder_node_exec.so',
              'verify_node_cross_build_evidence.py',
              'build_omniroute_android_bundle.py','stage_omniroute_android_asset.py',
              'verify_android_diagnostic_apk.sh" "$APK" sideload_alpha','write_alpha_build_evidence.py'):
    assert token in alpha_build
assert 'MODE" == "jcode" || "$MODE" == "alpha"' in apk_verify
assert 'MODE" == "node" || "$MODE" == "omniroute_service"' in apk_verify
assert 'SOURCE_REF="release/v3.8.50"' in omni_fetch
assert 'REVIEWED_COMMIT="ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"' in omni_fetch
assert '1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7' in omni_fetch
assert 'URL="${REPO}/archive/${REVIEWED_COMMIT}.zip"' in omni_fetch
assert 'comment != commit' in omni_fetch
assert 'development_alpha_apk_with_packaged_node_no_play_dependency_not_device_execution' in alpha_evidence
assert "'google_play_required_for_development_apk': False" in alpha_evidence
assert "'device_execution_proven': False" in alpha_evidence
assert "'device_service_round_trip_proven': False" in alpha_evidence
delivery=(ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
feature=(ROOT/'android/node_runtime/src/main/AndroidManifest.xml').read_text()
assert 'SplitInstallManagerFactory.create' in delivery
assert '.addModule(MODULE_NAME)' in delivery
assert 'bytesDownloaded()' in delivery and 'totalBytesToDownload()' in delivery
assert 'startConfirmationDialogForResult' in delivery and 'cancelInstall' in delivery
assert '<dist:on-demand />' in feature and 'dist:fusing dist:include="false"' in feature
print('Part 34.10.15 UI/runtime/on-demand-node pre-compile regression PASSED')
