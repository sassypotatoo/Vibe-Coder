package com.vibecoder.shell;

import android.app.Activity;
import android.os.Build;
import android.os.Bundle;
import android.content.pm.ApplicationInfo;
import android.graphics.Typeface;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

public final class MainActivity extends Activity {
    private static final String DIAGNOSTIC_INFERENCE_PROMPT =
            "Reply with one short sentence confirming that this VibeCoder model request was received.";
    private final ExecutorService diagnosticsExecutor = Executors.newSingleThreadExecutor();
    private final ExecutorService chatExecutor = Executors.newSingleThreadExecutor();
    private final ExecutorService cancelExecutor = Executors.newSingleThreadExecutor();
    private final AtomicBoolean diagnosticRunning = new AtomicBoolean(false);
    private final AtomicBoolean chatBootstrapRunning = new AtomicBoolean(false);
    private final AtomicBoolean chatControllerReady = new AtomicBoolean(false);
    private final AtomicBoolean chatTurnRunning = new AtomicBoolean(false);
    private final AtomicBoolean chatStopRequested = new AtomicBoolean(false);
    private TextView summary;
    private TextView details;
    private Button rerun;
    private volatile boolean omniRouteServiceStartedForDiagnostic = false;
    private volatile String activeProjectId;
    private volatile String activeConversationId;
    private AlphaWorkspaceUi alphaWorkspaceUi;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(buildUi());
        if (shouldAutoRunDiagnostics()) {
            runDiagnostics();
        } else {
            startChatRuntime();
        }
    }

    private boolean shouldAutoRunDiagnostics() {
        android.content.Intent intent = getIntent();
        return intent != null && intent.getBooleanExtra("vibecoder_diagnostic_test", false);
    }

    // Kept as a compatibility marker for the physical-device diagnostic harness.
    private static final String DIAGNOSTIC_UI_COMPATIBILITY_LABEL =
            "Part 31 first-APK shell · build and device proof";

    private View buildUi() {
        // These holders keep the existing diagnostic runner alive without forcing diagnostics to be
        // the primary user interface. Runtime proof remains available from the gear button.
        summary = new TextView(this);
        details = new TextView(this);
        rerun = new Button(this);
        rerun.setOnClickListener(v -> runDiagnostics());

        alphaWorkspaceUi = new AlphaWorkspaceUi(this, new AlphaWorkspaceUi.Callbacks() {
            @Override
            public void onOpenDiagnostics() {
                showDiagnosticsDialog();
            }

            @Override
            public void onNewChatRequested() {
                createNewChat();
            }

            @Override
            public void onConversationSelectionCleared() {
                activeProjectId = null;
                activeConversationId = null;
            }

            @Override
            public void onConversationSelected(String projectId, String conversationId) {
                if (isCanonicalUuid(projectId) && isCanonicalUuid(conversationId)) {
                    activeProjectId = projectId;
                    activeConversationId = conversationId;
                }
            }

            @Override
            public void onSendRequested(String prompt) {
                sendChatPrompt(prompt);
            }

            @Override
            public void onStopRequested() {
                stopActiveChatTurn();
            }
        });
        return alphaWorkspaceUi.root();
    }

    private void startChatRuntime() {
        if (!chatBootstrapRunning.compareAndSet(false, true)) return;
        chatControllerReady.set(false);
        if (alphaWorkspaceUi != null) {
            alphaWorkspaceUi.setBackendState(false, "Starting local AI runtime…");
        }
        chatExecutor.execute(() -> {
            RuntimeBootstrapResult result = bootstrapChatRuntime();
            runOnUiThread(() -> {
                chatBootstrapRunning.set(false);
                if (isDestroyed() || alphaWorkspaceUi == null) return;
                chatControllerReady.set(result.ready);
                alphaWorkspaceUi.setBackendState(result.ready, result.status);
            });
        });
    }

    private RuntimeBootstrapResult bootstrapChatRuntime() {
        try {
            byte[] inventory = readAsset("runtime/android-runtime-inventory.json");
            ApplicationInfo info = getApplicationInfo();
            if (info.nativeLibraryDir == null || info.nativeLibraryDir.isEmpty()) {
                return RuntimeBootstrapResult.failed("AI runtime unavailable · native library directory missing");
            }
            File nativeRoot = new File(info.nativeLibraryDir).getCanonicalFile();
            OmniRouteAssetInstaller.Result asset = OmniRouteAssetInstaller.ensureInstalled(
                    getAssets(), getFilesDir());
            if (!asset.packaged) {
                return RuntimeBootstrapResult.failed("AI runtime unavailable · OmniRoute bundle not packaged");
            }
            if (!asset.verified || !asset.manifestSha256.matches("[0-9a-f]{64}")) {
                return RuntimeBootstrapResult.failed("AI runtime unavailable · OmniRoute verification failed");
            }

            JSONObject service = new JSONObject(NativeBridge.nativeOmniRouteStart(
                    getFilesDir().getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    inventory,
                    asset.manifestSha256));
            if (!service.optBoolean("active", false)
                    || !service.optBoolean("ready", false)
                    || !service.optBoolean("runtime_profile_round_trip_proven", false)
                    || !service.optBoolean("exact_model_only", false)
                    || !service.optBoolean("hidden_model_reroutes_disabled", false)) {
                return RuntimeBootstrapResult.failed(
                        "AI runtime unavailable · " + safeNativeCode(service.optString("error", service.optString("status", "omniroute_not_ready"))));
            }

            JSONObject controller = null;
            for (int attempt = 0; attempt < 4; attempt++) {
                controller = new JSONObject(NativeBridge.nativeAppControllerInit(
                        getFilesDir().getCanonicalPath(),
                        nativeRoot.getCanonicalPath(),
                        nativeRoot.getCanonicalPath(),
                        inventory));
                if (controller.optBoolean("chat_ready", false)
                        && controller.optBoolean("runtime_profile_verified", false)
                        && "ready".equals(controller.optString("status", ""))) {
                    String model = controller.optString("selected_model_id", "");
                    return RuntimeBootstrapResult.ready(
                            model.isEmpty() ? "AI ready" : "AI ready · " + truncateLabel(model, 42));
                }
                String controllerStatus = controller.optString("status", "");
                boolean catalogMayStillBeWarming = "gateway_not_ready".equals(controllerStatus)
                        || "provider_setup_required".equals(controllerStatus)
                        || controller.optBoolean("provider_setup_required", false);
                if (!catalogMayStillBeWarming || attempt == 3) break;
                try {
                    Thread.sleep(250L);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    return RuntimeBootstrapResult.failed("AI runtime unavailable · startup_interrupted");
                }
            }
            if (controller != null
                    && (controller.optBoolean("provider_setup_required", false)
                    || "provider_setup_required".equals(controller.optString("status", "")))) {
                return RuntimeBootstrapResult.failed(
                        "OmniRoute ready · no usable model/provider available");
            }
            return RuntimeBootstrapResult.failed(
                    "AI runtime unavailable · " + safeNativeCode(controller == null
                            ? "controller_not_ready"
                            : controller.optString("error", controller.optString("status", "controller_not_ready"))));
        } catch (Throwable error) {
            return RuntimeBootstrapResult.failed(
                    "AI runtime unavailable · " + safeNativeCode(safeMessage(error)));
        }
    }

    private void createNewChat() {
        activeProjectId = null;
        activeConversationId = null;
        if (!chatControllerReady.get()) {
            if (alphaWorkspaceUi != null) {
                alphaWorkspaceUi.showCreateChatFailure("AI runtime is not ready yet.");
            }
            return;
        }
        if (chatTurnRunning.get()) return;
        chatExecutor.execute(() -> {
            try {
                ChatIdentity identity = createConversationOnWorker();
                activeProjectId = identity.projectId;
                activeConversationId = identity.conversationId;
                runOnUiThread(() -> {
                    if (isDestroyed() || alphaWorkspaceUi == null) return;
                    alphaWorkspaceUi.showNewChatReady();
                });
            } catch (Throwable error) {
                runOnUiThread(() -> {
                    if (isDestroyed() || alphaWorkspaceUi == null) return;
                    alphaWorkspaceUi.showCreateChatFailure(
                            "New chat could not be created · " + safeNativeCode(safeMessage(error)));
                });
            }
        });
    }

    private ChatIdentity createConversationOnWorker() throws Exception {
        JSONObject created = new JSONObject(NativeBridge.nativeChatCreate());
        if (!"created".equals(created.optString("status", ""))) {
            throw new IllegalStateException(created.optString("error", "chat_create_failed"));
        }
        String projectId = created.optString("project_id", "");
        String conversationId = created.optString("conversation_id", "");
        if (!isCanonicalUuid(projectId) || !isCanonicalUuid(conversationId)) {
            throw new IllegalStateException("chat_create_identity_invalid");
        }
        return new ChatIdentity(projectId, conversationId);
    }

    private void sendChatPrompt(String prompt) {
        if (!chatControllerReady.get()) {
            android.widget.Toast.makeText(
                    this, "AI runtime is not ready yet.", android.widget.Toast.LENGTH_SHORT).show();
            return;
        }
        byte[] promptUtf8 = prompt == null ? new byte[0] : prompt.getBytes(StandardCharsets.UTF_8);
        if (promptUtf8.length == 0 || promptUtf8.length > 128 * 1024) {
            android.widget.Toast.makeText(
                    this, "Message is empty or too large.", android.widget.Toast.LENGTH_SHORT).show();
            return;
        }
        if (!chatTurnRunning.compareAndSet(false, true)) return;
        chatStopRequested.set(false);
        if (alphaWorkspaceUi != null) alphaWorkspaceUi.setTurnRunning(true);

        chatExecutor.execute(() -> {
            try {
                ChatIdentity identity = activeIdentity();
                boolean createdNow = false;
                if (identity == null) {
                    identity = createConversationOnWorker();
                    activeProjectId = identity.projectId;
                    activeConversationId = identity.conversationId;
                    createdNow = true;
                }
                if (chatStopRequested.get()) {
                    postStoppedBeforeSend(createdNow);
                    return;
                }

                final boolean createdForUi = createdNow;
                runOnUiThread(() -> {
                    if (isDestroyed() || alphaWorkspaceUi == null) return;
                    if (createdForUi) alphaWorkspaceUi.showNewChatReady();
                    alphaWorkspaceUi.appendUserMessage(prompt);
                });

                // Once the user bubble is visible, always enter the Rust turn so that the same
                // message is durably persisted. A racing Stop is handled by native cancellation
                // instead of leaving a UI-only message that disappears from saved history.
                JSONObject response = new JSONObject(NativeBridge.nativeChatSend(
                        identity.projectId,
                        identity.conversationId,
                        promptUtf8));
                String status = response.optString("status", "failed");
                if ("completed".equals(status)) {
                    String assistant = response.optString("assistant_text", "");
                    if (assistant.isEmpty()) throw new IllegalStateException("chat_assistant_text_missing");
                    runOnUiThread(() -> {
                        if (isDestroyed() || alphaWorkspaceUi == null) return;
                        alphaWorkspaceUi.appendAssistantMessage(assistant);
                    });
                } else if ("cancelled".equals(status)) {
                    postTurnNotice("Stopped.");
                } else {
                    postTurnNotice(friendlyChatFailure(response.optString("error", status)));
                }
            } catch (Throwable error) {
                postTurnNotice(friendlyChatFailure(safeMessage(error)));
            } finally {
                chatTurnRunning.set(false);
                chatStopRequested.set(false);
                runOnUiThread(() -> {
                    if (isDestroyed() || alphaWorkspaceUi == null) return;
                    alphaWorkspaceUi.setTurnRunning(false);
                });
            }
        });
    }

    private void postStoppedBeforeSend(boolean createdNow) {
        runOnUiThread(() -> {
            if (isDestroyed() || alphaWorkspaceUi == null) return;
            if (createdNow) alphaWorkspaceUi.showNewChatReady();
            alphaWorkspaceUi.showTurnNotice("Stopped before the message was sent.");
        });
    }

    private void stopActiveChatTurn() {
        if (!chatTurnRunning.get()) return;
        chatStopRequested.set(true);
        cancelExecutor.execute(() -> {
            // A Stop tap can race the native send entry point. Retry briefly until the Rust turn
            // registration becomes visible, or until the worker finishes on its own.
            for (int attempt = 0; attempt < 40 && chatTurnRunning.get() && chatStopRequested.get(); attempt++) {
                ChatIdentity identity = activeIdentity();
                if (identity != null) {
                    try {
                        JSONObject cancelled = new JSONObject(NativeBridge.nativeChatCancel(
                                identity.projectId, identity.conversationId));
                        if (cancelled.optBoolean("cancel_requested", false)) return;
                    } catch (Throwable ignored) {
                        // Retry while the owning send thread is still active; no state is fabricated.
                    }
                }
                try {
                    Thread.sleep(50L);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    return;
                }
            }
        });
    }

    private ChatIdentity activeIdentity() {
        String projectId = activeProjectId;
        String conversationId = activeConversationId;
        if (!isCanonicalUuid(projectId) || !isCanonicalUuid(conversationId)) return null;
        return new ChatIdentity(projectId, conversationId);
    }

    private void postTurnNotice(String message) {
        runOnUiThread(() -> {
            if (isDestroyed() || alphaWorkspaceUi == null) return;
            alphaWorkspaceUi.showTurnNotice(message);
        });
    }

    private static String friendlyChatFailure(String code) {
        String safe = safeNativeCode(code);
        if ("cancelled".equals(safe)) return "Stopped.";
        if (safe.contains("no_usable_models") || safe.contains("provider_setup")) {
            return "No usable AI model/provider is available.";
        }
        if (safe.contains("gateway") && safe.contains("auth")) {
            return "The selected AI provider needs authentication.";
        }
        return "Message failed · " + safe;
    }

    private static boolean isCanonicalUuid(String value) {
        if (value == null || value.length() != 36) return false;
        try {
            return java.util.UUID.fromString(value).toString().equals(value);
        } catch (IllegalArgumentException ignored) {
            return false;
        }
    }

    private static String safeNativeCode(String value) {
        if (value == null || value.isEmpty()) return "unknown_error";
        StringBuilder output = new StringBuilder();
        for (int index = 0; index < value.length() && output.length() < 160; index++) {
            char character = value.charAt(index);
            output.append((character >= 'a' && character <= 'z')
                    || (character >= 'A' && character <= 'Z')
                    || (character >= '0' && character <= '9')
                    || character == '_' || character == '-' || character == '.'
                    ? character : '_');
        }
        return output.length() == 0 ? "unknown_error" : output.toString();
    }

    private static String truncateLabel(String value, int maxCodePoints) {
        if (value == null) return "";
        int count = value.codePointCount(0, value.length());
        if (count <= maxCodePoints) return value;
        int end = value.offsetByCodePoints(0, maxCodePoints);
        return value.substring(0, end) + "…";
    }

    private void showDiagnosticsDialog() {
        String summaryText = summary == null ? "Diagnostics not started" : summary.getText().toString();
        String detailsText = details == null ? "" : details.getText().toString();
        String message = summaryText + (detailsText.isEmpty() ? "" : "\n\n" + detailsText);
        new android.app.AlertDialog.Builder(this)
                .setTitle("Runtime diagnostics")
                .setMessage(message)
                .setPositiveButton("Close", null)
                .setNeutralButton("Run checks", (dialog, which) -> runDiagnostics())
                .show();
    }

    private void runDiagnostics() {
        if (!diagnosticRunning.compareAndSet(false, true)) return;
        summary.setText("Running…");
        rerun.setEnabled(false);
        diagnosticsExecutor.execute(() -> {
            DiagnosticResult result = collectDiagnostics();
            runOnUiThread(() -> {
                diagnosticRunning.set(false);
                if (isDestroyed()) return;
                summary.setText(result.summary);
                details.setText(result.details);
                rerun.setEnabled(true);
            });
        });
    }

    private DiagnosticResult collectDiagnostics() {
        boolean arm64 = Arrays.asList(Build.SUPPORTED_ABIS).contains("arm64-v8a");
        try {
            byte[] inventory = readAsset("runtime/android-runtime-inventory.json");
            ApplicationInfo info = getApplicationInfo();
            if (info.nativeLibraryDir == null || info.nativeLibraryDir.isEmpty()) {
                throw new IllegalStateException("native_library_dir_missing");
            }
            File nativeRoot = new File(info.nativeLibraryDir);
            byte[] assetEvidence = buildApkAssetEvidence(inventory);
            JSONObject omniRouteAssetState = collectOmniRouteAssetState();
            // The same package-owned directory is intentionally supplied for JNI libraries and
            // child-process executables only because build.gradle.kts enforces
            // packaging.jniLibs.useLegacyPackaging = true. If that packaging contract changes,
            // these roots must be resolved independently instead of silently reusing this path.
            String snapshot = NativeBridge.nativeProbeSnapshot(
                    getFilesDir().getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    inventory,
                    assetEvidence);

            JSONObject json = new JSONObject(snapshot);
            JSONObject omniRouteServiceState = collectOmniRouteServiceState(
                    inventory,
                    nativeRoot,
                    omniRouteAssetState);
            JSONObject omniRouteGatewayState = omniRouteServiceState == null
                    ? null
                    : omniRouteServiceState.optJSONObject("gateway_transport");
            JSONObject omniRouteInferenceState = omniRouteServiceState == null
                    ? null
                    : omniRouteServiceState.optJSONObject("inference");
            persistDiagnosticReport(buildDiagnosticReport(
                    arm64, json, omniRouteAssetState, omniRouteServiceState, omniRouteGatewayState,
                    omniRouteInferenceState, null));
            boolean nativeLoaded = json.optBoolean("native_loaded", false);
            boolean probeOk = json.optBoolean("probe_ok", false);
            JSONObject readiness = json.optJSONObject("readiness");

            StringBuilder status = new StringBuilder();
            status.append(line("Device ARM64", arm64));
            status.append(line("JNI bridge", true));
            status.append(line("Rust host", nativeLoaded));
            status.append(line("Host probe", probeOk));
            status.append(line("Omni asset", omniRouteAssetState.optBoolean("verified", false)));
            if (omniRouteServiceState != null) {
                status.append(line("Omni service",
                        omniRouteServiceState.optBoolean("ready", false)
                                && omniRouteServiceState.optBoolean("active", false)));
            }
            if (omniRouteGatewayState != null) {
                status.append(line("Omni transport",
                        omniRouteGatewayState.optBoolean("local_transport_round_trip_proven", false)
                                && omniRouteGatewayState.optBoolean("catalog_round_trip_reached", false)));
            }
            if (omniRouteInferenceState != null) {
                status.append(line("First AI reply",
                        omniRouteInferenceState.optBoolean("first_model_request_proven", false)));
            }
            if (readiness != null) {
                status.append(line("Core", readiness.optBoolean("core_ready", false)));
                status.append(line("Jcode agent", readiness.optBoolean("agent_ready", false)));
                status.append(line("OmniRoute", readiness.optBoolean("gateway_ready", false)));
                status.append(line("Website build", readiness.optBoolean("website_build_ready", false)));
                status.append(line("Android build", readiness.optBoolean("android_build_ready", false)));
            }

            String diagnosticDetails =
                    "SDK: " + Build.VERSION.SDK_INT + "\n" +
                    "ABIs: " + Arrays.toString(Build.SUPPORTED_ABIS) + "\n" +
                    "filesDir: " + getFilesDir() + "\n" +
                    "nativeLibraryDir: " + nativeRoot + "\n\n" +
                    expectedPayloads(nativeRoot) +
                    "\n\nOmniRoute asset state:\n" + omniRouteAssetState.toString(2) +
                    (omniRouteServiceState == null
                            ? ""
                            : "\n\nOmniRoute service state:\n" + omniRouteServiceState.toString(2)) +
                    (omniRouteGatewayState == null
                            ? ""
                            : "\n\nOmniRoute gateway transport:\n" + omniRouteGatewayState.toString(2)) +
                    (omniRouteInferenceState == null
                            ? ""
                            : "\n\nOmniRoute first inference proof:\n" + omniRouteInferenceState.toString(2)) +
                    "\n\nProbe snapshot:\n" + pretty(snapshot);
            return new DiagnosticResult(status.toString(), diagnosticDetails);
        } catch (Throwable error) {
            try {
                persistDiagnosticReport(buildDiagnosticReport(arm64, null, null, null, null, null, error));
            } catch (Throwable ignored) {
                // The UI still surfaces the primary diagnostic error. Device automation will time out
                // if app-private report persistence is also unavailable.
            }
            return new DiagnosticResult(
                    "❌ Diagnostic shell error",
                    error.getClass().getSimpleName() + ": " + safeMessage(error));
        }
    }

    @Override
    protected void onDestroy() {
        if (alphaWorkspaceUi != null) {
            alphaWorkspaceUi.destroy();
            alphaWorkspaceUi = null;
        }
        if (chatTurnRunning.get()) {
            stopActiveChatTurn();
        }
        diagnosticsExecutor.shutdownNow();
        // Let an in-flight send observe the real cancellation request and finish its durable cleanup.
        // Interrupting a Java thread that is inside JNI does not cancel the native Rust future.
        chatExecutor.shutdown();
        cancelExecutor.shutdown();
        if (omniRouteServiceStartedForDiagnostic) {
            try {
                NativeBridge.nativeOmniRouteStop();
            } catch (Throwable ignored) {
                // Android process death still activates the Rust child PDEATHSIG guard.
            } finally {
                omniRouteServiceStartedForDiagnostic = false;
            }
        }
        super.onDestroy();
    }

    private String expectedPayloads(File root) {
        String[] names = {
                "libvibecoder_android_host.so",
                "libvibecoder_jcode_exec.so",
                "libvibecoder_node_exec.so",
                "libvibecoder_java_exec.so",
                "libvibecoder_aapt2_exec.so",
                "libvibecoder_zipalign_exec.so"
        };
        StringBuilder out = new StringBuilder("Packaged native payloads:\n");
        for (String name : names) {
            File candidate = new File(root, name);
            out.append(candidate.isFile() ? "✅ " : "❌ ").append(name).append('\n');
        }
        return out.toString();
    }

    private JSONObject collectOmniRouteAssetState() {
        try {
            return OmniRouteAssetInstaller.ensureInstalled(getAssets(), getFilesDir()).toJson();
        } catch (Throwable error) {
            JSONObject failed = new JSONObject();
            try {
                failed.put("schema", 1);
                failed.put("component_id", "omniroute");
                failed.put("packaged", assetPathExists("omniroute/bundle/" + OmniRouteAssetInstaller.MANIFEST_NAME));
                failed.put("verified", false);
                failed.put("installed_now", false);
                failed.put("status", "install_failed");
                failed.put("error", safeMessage(error));
                failed.put("service_round_trip_proven", false);
            } catch (Exception ignored) {
                // JSONObject writes above use only in-memory primitive values.
            }
            return failed;
        }
    }


    private JSONObject collectOmniRouteServiceState(
            byte[] inventory,
            File nativeRoot,
            JSONObject assetState) {
        if (!getIntent().getBooleanExtra("vibecoder_omniroute_service_test", false)) {
            return null;
        }
        JSONObject failed = new JSONObject();
        try {
            failed.put("schema", 1);
            failed.put("component_id", "omniroute");
            failed.put("active", false);
            failed.put("ready", false);
            failed.put("runtime_profile_round_trip_proven", false);
            if (!assetState.optBoolean("verified", false)) {
                failed.put("error", "omniroute_asset_not_verified");
                return failed;
            }
            String manifestSha256 = assetState.optString("manifest_sha256", "");
            if (!manifestSha256.matches("[0-9a-f]{64}")) {
                failed.put("error", "omniroute_asset_manifest_sha_invalid");
                return failed;
            }
            String response = NativeBridge.nativeOmniRouteStart(
                    getFilesDir().getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    nativeRoot.getCanonicalPath(),
                    inventory,
                    manifestSha256);
            JSONObject state = new JSONObject(response);
            boolean startedReady = state.optBoolean("active", false) && state.optBoolean("ready", false);
            if (startedReady && "started_ready".equals(state.optString("status", ""))) {
                omniRouteServiceStartedForDiagnostic = true;
            }
            if (startedReady
                    && getIntent().getBooleanExtra("vibecoder_omniroute_gateway_test", false)) {
                // Part 34.4 diagnostics use anonymous access only. Real bearer credentials must come
                // from the future secure-store boundary and are never persisted in this report.
                JSONObject gatewayTransport = new JSONObject(
                        NativeBridge.nativeOmniRouteGatewayProbe(new byte[0]));
                state.put("gateway_transport", gatewayTransport);
            }
            if (startedReady
                    && getIntent().getBooleanExtra("vibecoder_omniroute_inference_test", false)) {
                String modelId = getIntent().getStringExtra("vibecoder_omniroute_model");
                if (modelId == null || modelId.isEmpty() || modelId.length() > 512) {
                    throw new IllegalStateException("omniroute_inference_model_missing_or_invalid");
                }
                // Part 34.5 diagnostics remain anonymous to the local gateway. Provider accounts
                // are owned by OmniRoute; no API key or prompt/response text is persisted here.
                JSONObject inference = new JSONObject(NativeBridge.nativeOmniRouteInferenceProbe(
                        new byte[0],
                        modelId.getBytes(StandardCharsets.UTF_8),
                        DIAGNOSTIC_INFERENCE_PROMPT.getBytes(StandardCharsets.UTF_8)));
                state.put("inference", inference);
            }
            if (startedReady
                    && getIntent().getBooleanExtra("vibecoder_omniroute_service_stop_after_probe", false)) {
                JSONObject liveStatus = new JSONObject(NativeBridge.nativeOmniRouteStatus());
                JSONObject stopResult = new JSONObject(NativeBridge.nativeOmniRouteStop());
                boolean stopped = !stopResult.optBoolean("active", true)
                        && !stopResult.optBoolean("ready", true)
                        && "stopped".equals(stopResult.optString("status", ""));
                state.put("live_status", liveStatus);
                state.put("stop_result", stopResult);
                state.put("service_round_trip_proven",
                        liveStatus.optBoolean("active", false)
                                && liveStatus.optBoolean("ready", false)
                                && liveStatus.optBoolean("runtime_profile_round_trip_proven", false)
                                && stopped);
                if (stopped) {
                    omniRouteServiceStartedForDiagnostic = false;
                }
            } else {
                state.put("service_round_trip_proven", false);
            }
            return state;
        } catch (Throwable error) {
            try {
                failed.put("error", safeMessage(error));
            } catch (Exception ignored) {
                // Primitive in-memory fallback only.
            }
            return failed;
        }
    }

    private JSONObject buildDiagnosticReport(
            boolean arm64,
            JSONObject snapshot,
            JSONObject omniRouteAssetState,
            JSONObject omniRouteServiceState,
            JSONObject omniRouteGatewayState,
            JSONObject omniRouteInferenceState,
            Throwable error) throws Exception {
        JSONObject report = new JSONObject();
        report.put("schema", 1);
        report.put("part", 31);
        report.put("package", getPackageName());
        report.put("sdk_int", Build.VERSION.SDK_INT);
        report.put("device_arm64", arm64);
        if (omniRouteAssetState != null) {
            report.put("omniroute_asset_installation", omniRouteAssetState);
        }
        if (omniRouteServiceState != null) {
            report.put("omniroute_service", omniRouteServiceState);
        }
        if (omniRouteGatewayState != null) {
            report.put("omniroute_gateway_transport", omniRouteGatewayState);
        }
        if (omniRouteInferenceState != null) {
            report.put("omniroute_inference", omniRouteInferenceState);
        }
        if (snapshot != null) {
            report.put("probe_snapshot", snapshot);
        } else {
            JSONObject failed = new JSONObject();
            failed.put("schema", 1);
            failed.put("native_loaded", false);
            failed.put("probe_ok", false);
            failed.put("error", error == null ? "diagnostic_failed" : safeMessage(error));
            report.put("probe_snapshot", failed);
        }
        return report;
    }

    private void persistDiagnosticReport(JSONObject report) throws Exception {
        byte[] bytes = report.toString().getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 1024 * 1024) {
            throw new IllegalStateException("diagnostic_report_too_large");
        }
        File target = new File(getFilesDir(), "vibecoder-diagnostic-result.json");
        File temp = new File(getFilesDir(), "vibecoder-diagnostic-result.json.tmp");
        try (FileOutputStream output = new FileOutputStream(temp, false)) {
            output.write(bytes);
            output.flush();
            output.getFD().sync();
        }
        if (!temp.renameTo(target)) {
            if (target.exists() && !target.delete()) {
                throw new IllegalStateException("diagnostic_report_replace_failed");
            }
            if (!temp.renameTo(target)) {
                throw new IllegalStateException("diagnostic_report_commit_failed");
            }
        }
    }

    private static String line(String label, boolean ok) {
        return String.format(Locale.ROOT, "%s %-16s %s%n", ok ? "✅" : "❌", label, ok ? "READY" : "NOT READY");
    }

    private byte[] buildApkAssetEvidence(byte[] inventoryBytes) throws Exception {
        JSONObject inventory = new JSONObject(new String(inventoryBytes, StandardCharsets.UTF_8));
        JSONArray components = inventory.getJSONArray("components");
        JSONArray evidence = new JSONArray();
        for (int index = 0; index < components.length(); index++) {
            JSONObject component = components.getJSONObject(index);
            if (!"apk_asset".equals(component.optString("placement"))) continue;
            String componentId = component.getString("component_id");
            String relativePath = component.optString("relative_path", "");
            JSONObject item = new JSONObject();
            item.put("component_id", componentId);
            item.put("package_presence", assetPathExists(relativePath) ? "passed" : "failed");
            evidence.put(item);
        }
        return evidence.toString().getBytes(StandardCharsets.UTF_8);
    }

    private boolean assetPathExists(String path) {
        if (path == null || path.isEmpty() || path.startsWith("/") || path.contains("..")) {
            return false;
        }
        try (InputStream input = getAssets().open(path)) {
            // Referencing the stream also keeps strict Java lint from treating this resource as
            // an unused AutoCloseable while preserving a no-content-read existence probe.
            return input.available() >= 0;
        } catch (Exception ignored) {
            try {
                String[] children = getAssets().list(path);
                return children != null && children.length > 0;
            } catch (Exception ignoredAgain) {
                return false;
            }
        }
    }

    private byte[] readAsset(String name) throws Exception {
        try (InputStream input = getAssets().open(name);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[4096];
            int read;
            while ((read = input.read(buffer)) != -1) {
                if (output.size() + read > 1024 * 1024) {
                    throw new IllegalStateException("asset_too_large");
                }
                output.write(buffer, 0, read);
            }
            return output.toByteArray();
        }
    }

    private static String pretty(String raw) {
        try {
            return new JSONObject(raw).toString(2);
        } catch (Exception ignored) {
            return raw;
        }
    }

    private static String safeMessage(Throwable error) {
        String message = error.getMessage();
        if (message == null || message.isEmpty()) return "no_message";
        return message.length() > 500 ? message.substring(0, 500) : message;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
    private static final class ChatIdentity {
        final String projectId;
        final String conversationId;

        ChatIdentity(String projectId, String conversationId) {
            this.projectId = projectId;
            this.conversationId = conversationId;
        }
    }

    private static final class RuntimeBootstrapResult {
        final boolean ready;
        final String status;

        private RuntimeBootstrapResult(boolean ready, String status) {
            this.ready = ready;
            this.status = status;
        }

        static RuntimeBootstrapResult ready(String status) {
            return new RuntimeBootstrapResult(true, status);
        }

        static RuntimeBootstrapResult failed(String status) {
            return new RuntimeBootstrapResult(false, status);
        }
    }

    private static final class DiagnosticResult {
        final String summary;
        final String details;

        DiagnosticResult(String summary, String details) {
            this.summary = summary;
            this.details = details;
        }
    }

}
