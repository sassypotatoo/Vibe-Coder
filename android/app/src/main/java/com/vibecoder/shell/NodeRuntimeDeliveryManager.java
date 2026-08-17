package com.vibecoder.shell;

import android.app.Activity;
import android.content.Context;
import android.content.pm.ApplicationInfo;

import dalvik.system.BaseDexClassLoader;

import com.google.android.play.core.splitcompat.SplitCompat;
import com.google.android.play.core.splitinstall.SplitInstallException;
import com.google.android.play.core.splitinstall.SplitInstallManager;
import com.google.android.play.core.splitinstall.SplitInstallManagerFactory;
import com.google.android.play.core.splitinstall.SplitInstallRequest;
import com.google.android.play.core.splitinstall.SplitInstallSessionState;
import com.google.android.play.core.splitinstall.SplitInstallStateUpdatedListener;
import com.google.android.play.core.splitinstall.model.SplitInstallSessionStatus;

import java.io.File;
import java.util.Set;

final class NodeRuntimeDeliveryManager implements AutoCloseable {
    static final String MODULE_NAME = "node_runtime";
    static final String NODE_FILE_NAME = "libvibecoder_node_exec.so";
    private static final String NODE_LIBRARY_NAME = "vibecoder_node_exec";
    static final String NODE_VERSION = "24.19.0";
    private static final int CONFIRMATION_REQUEST_CODE = 24019;

    interface Listener {
        void onNodeRuntimeState(State state);
    }

    static final class State {
        final String status;
        final long downloadedBytes;
        final long totalBytes;
        final int percent;
        final String error;
        final File executableDirectory;

        State(String status, long downloadedBytes, long totalBytes, String error, File executableDirectory) {
            this.status = status;
            this.downloadedBytes = Math.max(0L, downloadedBytes);
            this.totalBytes = Math.max(0L, totalBytes);
            this.percent = totalBytes > 0L ? (int) Math.min(100L, (downloadedBytes * 100L) / totalBytes) : 0;
            this.error = error;
            this.executableDirectory = executableDirectory;
        }

        boolean ready() { return "ready".equals(status) && executableDirectory != null; }
    }

    private final Activity activity;
    private final SplitInstallManager splitInstallManager;
    private final Listener listener;
    private final SplitInstallStateUpdatedListener stateListener = this::onStateUpdate;
    private int activeSessionId;
    private boolean registered;

    NodeRuntimeDeliveryManager(Activity activity, Listener listener) {
        this.activity = activity;
        this.listener = listener;
        this.splitInstallManager = SplitInstallManagerFactory.create(activity.getApplicationContext());
    }

    State currentState() {
        File directory = resolvePackagedNodeDirectory();
        if (directory != null) return new State("ready", 0L, 0L, null, directory);
        Set<String> modules = splitInstallManager.getInstalledModules();
        if (modules.contains(MODULE_NAME)) {
            return new State("failed", 0L, 0L, "node_split_installed_but_executable_missing", null);
        }
        return new State("not_installed", 0L, 0L, null, null);
    }

    void restoreActiveInstall() {
        State current = currentState();
        if (current.ready()) {
            listener.onNodeRuntimeState(current);
            return;
        }
        register();
        splitInstallManager.getSessionStates()
                .addOnSuccessListener(states -> {
                    for (SplitInstallSessionState state : states) {
                        if (!state.moduleNames().contains(MODULE_NAME)) continue;
                        int status = state.status();
                        if (status == SplitInstallSessionStatus.FAILED
                                || status == SplitInstallSessionStatus.CANCELED) {
                            continue;
                        }
                        activeSessionId = state.sessionId();
                        onStateUpdate(state);
                        return;
                    }
                    unregister();
                })
                .addOnFailureListener(error -> unregister());
    }

    void startInstall() {
        State current = currentState();
        if (current.ready()) {
            listener.onNodeRuntimeState(current);
            return;
        }
        register();
        SplitInstallRequest request = SplitInstallRequest.newBuilder().addModule(MODULE_NAME).build();
        splitInstallManager.startInstall(request)
                .addOnSuccessListener(sessionId -> {
                    activeSessionId = sessionId;
                    if (sessionId == 0) {
                        State installed = currentState();
                        emitTerminal(installed.ready() ? installed : new State(
                                "failed", 0L, 0L, "node_runtime_already_installed_payload_unavailable", null));
                    } else {
                        listener.onNodeRuntimeState(new State("pending", 0L, 0L, null, null));
                    }
                })
                .addOnFailureListener(error -> emitTerminal(
                        new State("failed", 0L, 0L, splitInstallFailureCode(error), null)));
    }

    void cancelInstall() {
        if (activeSessionId <= 0) return;
        splitInstallManager.cancelInstall(activeSessionId);
    }

    boolean onActivityResult(int requestCode, int resultCode) {
        if (requestCode != CONFIRMATION_REQUEST_CODE) return false;
        if (resultCode != Activity.RESULT_OK) {
            emitTerminal(new State("cancelled", 0L, 0L, "user_confirmation_denied", null));
        }
        return true;
    }

    @SuppressWarnings("deprecation")
    private void onStateUpdate(SplitInstallSessionState state) {
        if (!state.moduleNames().contains(MODULE_NAME)) return;
        if (activeSessionId != 0 && state.sessionId() != activeSessionId) return;
        activeSessionId = state.sessionId();
        long downloaded = state.bytesDownloaded();
        long total = state.totalBytesToDownload();
        switch (state.status()) {
            case SplitInstallSessionStatus.PENDING:
                listener.onNodeRuntimeState(new State("pending", downloaded, total, null, null));
                break;
            case SplitInstallSessionStatus.DOWNLOADING:
                listener.onNodeRuntimeState(new State("downloading", downloaded, total, null, null));
                break;
            case SplitInstallSessionStatus.DOWNLOADED:
            case SplitInstallSessionStatus.INSTALLING:
                listener.onNodeRuntimeState(new State("installing", downloaded, total, null, null));
                break;
            case SplitInstallSessionStatus.REQUIRES_USER_CONFIRMATION:
                try {
                    boolean started = splitInstallManager.startConfirmationDialogForResult(
                            state, activity, CONFIRMATION_REQUEST_CODE);
                    if (!started) {
                        emitTerminal(new State(
                                "failed", downloaded, total, "node_runtime_confirmation_not_started", null));
                    }
                } catch (android.content.IntentSender.SendIntentException error) {
                    emitTerminal(new State(
                            "failed", downloaded, total, "node_runtime_confirmation_failed", null));
                }
                break;
            case SplitInstallSessionStatus.INSTALLED:
                SplitCompat.installActivity(activity);
                State installed = currentState();
                emitTerminal(installed.ready() ? installed : new State(
                        "failed", downloaded, total, "node_runtime_installed_payload_unavailable", null));
                break;
            case SplitInstallSessionStatus.CANCELING:
                listener.onNodeRuntimeState(new State("cancelling", downloaded, total, null, null));
                break;
            case SplitInstallSessionStatus.CANCELED:
                emitTerminal(new State("cancelled", downloaded, total, null, null));
                break;
            case SplitInstallSessionStatus.FAILED:
                emitTerminal(new State(
                        "failed", downloaded, total, "node_runtime_split_error_" + state.errorCode(), null));
                break;
            default:
                listener.onNodeRuntimeState(new State("pending", downloaded, total, null, null));
        }
    }

    private File resolvePackagedNodeDirectory() {
        try {
            // A newly installed optional split is not guaranteed to share the base APK's
            // nativeLibraryDir. Refresh split visibility, then ask Android's class loader for the
            // absolute native-library path. This also avoids guessing split installation paths.
            SplitCompat.installActivity(activity);
            Context freshContext = activity.createPackageContext(activity.getPackageName(), 0);
            ClassLoader loader = freshContext.getClassLoader();
            if (loader instanceof BaseDexClassLoader) {
                String absolutePath = ((BaseDexClassLoader) loader).findLibrary(NODE_LIBRARY_NAME);
                File resolved = validatedNodeExecutable(absolutePath == null ? null : new File(absolutePath));
                if (resolved != null) return resolved.getParentFile();
            }

            // Fallback keeps local/fused diagnostic packaging usable, while production AAB
            // verification separately forbids Node from leaking into the base module.
            ApplicationInfo info = freshContext.getApplicationInfo();
            if (info.nativeLibraryDir == null || info.nativeLibraryDir.isEmpty()) return null;
            File resolved = validatedNodeExecutable(new File(info.nativeLibraryDir, NODE_FILE_NAME));
            return resolved == null ? null : resolved.getParentFile();
        } catch (Exception ignored) {
            return null;
        }
    }

    private static File validatedNodeExecutable(File candidate) {
        if (candidate == null) return null;
        try {
            File node = candidate.getCanonicalFile();
            File parent = node.getParentFile();
            if (parent == null
                    || !NODE_FILE_NAME.equals(node.getName())
                    || !node.isFile()
                    || !node.canExecute()) {
                return null;
            }
            return node;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static String splitInstallFailureCode(Exception error) {
        if (error instanceof SplitInstallException) {
            return "node_runtime_split_error_" + ((SplitInstallException) error).getErrorCode();
        }
        return "node_runtime_install_request_failed";
    }

    private void register() {
        if (registered) return;
        splitInstallManager.registerListener(stateListener);
        registered = true;
    }

    private void unregister() {
        if (!registered) return;
        splitInstallManager.unregisterListener(stateListener);
        registered = false;
    }

    private void emitTerminal(State state) {
        activeSessionId = 0;
        unregister();
        listener.onNodeRuntimeState(state);
    }

    @Override
    public void close() {
        activeSessionId = 0;
        unregister();
    }
}
