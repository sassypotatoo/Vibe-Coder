package com.vibecoder.shell;

import android.app.Activity;
import android.app.PendingIntent;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;

import dalvik.system.BaseDexClassLoader;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

final class NodeRuntimeDeliveryManager implements AutoCloseable {
    static final String NODE_FILE_NAME = "libvibecoder_node_exec.so";
    static final String NODE_VERSION = "24.19.0";
    static final String RUNTIME_SPLIT_NAME = "node_runtime";
    static final String RUNTIME_RELEASE_TAG = "vibecoder-node-runtime-24.19.0-v31";
    static final String RUNTIME_APK_NAME = "vibecoder-node-runtime-arm64-v31.apk";
    static final String RUNTIME_URL = "https://github.com/sassypotatoo/Vibe-Coder/releases/download/"
            + RUNTIME_RELEASE_TAG + "/" + RUNTIME_APK_NAME;

    private static final String NODE_LIBRARY_NAME = "vibecoder_node_exec";
    private static final String ACTION_INSTALL_RESULT = "com.vibecoder.shell.NODE_RUNTIME_INSTALL_RESULT";
    private static final int UNKNOWN_SOURCES_REQUEST_CODE = 24020;
    private static final int BUFFER_SIZE = 256 * 1024;

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
            this.percent = totalBytes > 0L
                    ? (int) Math.min(100L, (this.downloadedBytes * 100L) / totalBytes)
                    : 0;
            this.error = error;
            this.executableDirectory = executableDirectory;
        }

        boolean ready() {
            return "ready".equals(status) && executableDirectory != null;
        }
    }

    private final Activity activity;
    private final Listener listener;
    private final ExecutorService downloadExecutor = Executors.newSingleThreadExecutor();
    private final AtomicBoolean downloadActive = new AtomicBoolean(false);
    private final AtomicBoolean cancelRequested = new AtomicBoolean(false);
    private volatile File downloadedRuntimeApk;
    private volatile int installSessionId = -1;
    private volatile boolean closed;

    NodeRuntimeDeliveryManager(Activity activity, Listener listener) {
        this.activity = activity;
        this.listener = listener;
    }

    State currentState() {
        File directory = resolveInstalledNodeDirectory();
        if (directory != null) {
            return new State("ready", 0L, 0L, null, directory);
        }
        if (isRuntimeSplitInstalled()) {
            return new State("restart_required", 0L, 0L, null, null);
        }
        if (downloadActive.get()) {
            return new State("downloading", 0L, 0L, null, null);
        }
        return new State("not_installed", 0L, 0L, null, null);
    }

    void restoreActiveInstall() {
        State state = currentState();
        listener.onNodeRuntimeState(state);
        if ("restart_required".equals(state.status)) {
            restartApplication();
        }
    }

    void startInstall() {
        if (closed) return;
        State current = currentState();
        if (current.ready()) {
            listener.onNodeRuntimeState(current);
            return;
        }
        if ("restart_required".equals(current.status)) {
            restartApplication();
            return;
        }
        if (!downloadActive.compareAndSet(false, true)) return;
        cancelRequested.set(false);
        listener.onNodeRuntimeState(new State("preparing", 0L, 0L, null, null));
        downloadExecutor.execute(this::downloadRuntime);
    }

    void cancelInstall() {
        cancelRequested.set(true);
        int sessionId = installSessionId;
        if (sessionId >= 0) {
            try {
                activity.getPackageManager().getPackageInstaller().abandonSession(sessionId);
            } catch (Exception ignored) {
                // Session may already be committed or removed by the package installer.
            }
        }
        listener.onNodeRuntimeState(new State("cancelling", 0L, 0L, null, null));
    }

    boolean onActivityResult(int requestCode) {
        if (requestCode != UNKNOWN_SOURCES_REQUEST_CODE) return false;
        File runtimeApk = downloadedRuntimeApk;
        if (runtimeApk == null || !runtimeApk.isFile()) {
            emitTerminal(new State("failed", 0L, 0L, "node_runtime_download_missing_after_permission", null));
            return true;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                && !activity.getPackageManager().canRequestPackageInstalls()) {
            emitTerminal(new State("failed", 0L, 0L, "node_runtime_install_permission_denied", null));
            return true;
        }
        installRuntime(runtimeApk);
        return true;
    }

    boolean handleInstallResultIntent(Intent intent) {
        if (intent == null || !ACTION_INSTALL_RESULT.equals(intent.getAction())) return false;
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirmation = extractConfirmationIntent(intent);
            if (confirmation == null) {
                emitTerminal(new State("failed", 0L, 0L, "node_runtime_install_confirmation_missing", null));
                return true;
            }
            listener.onNodeRuntimeState(new State("awaiting_confirmation", 0L, 0L, null, null));
            activity.startActivity(confirmation);
            return true;
        }
        if (status == PackageInstaller.STATUS_SUCCESS) {
            cleanupDownload();
            downloadActive.set(false);
            File directory = resolveInstalledNodeDirectory();
            if (directory != null) {
                listener.onNodeRuntimeState(new State("ready", 0L, 0L, null, directory));
            } else {
                listener.onNodeRuntimeState(new State("restart_required", 0L, 0L, null, null));
                restartApplication();
            }
            return true;
        }
        String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
        emitTerminal(new State(
                "failed",
                0L,
                0L,
                "node_runtime_package_install_failed_" + status + safeSuffix(message),
                null));
        return true;
    }

    private void downloadRuntime() {
        HttpURLConnection connection = null;
        File temp = null;
        try {
            File runtimeDir = new File(activity.getCacheDir(), "node-runtime");
            if (!runtimeDir.exists() && !runtimeDir.mkdirs()) {
                throw new IllegalStateException("runtime_cache_directory_create_failed");
            }
            temp = new File(runtimeDir, RUNTIME_APK_NAME + ".part");
            File target = new File(runtimeDir, RUNTIME_APK_NAME);
            if (temp.exists() && !temp.delete()) {
                throw new IllegalStateException("runtime_partial_delete_failed");
            }
            if (target.exists() && !target.delete()) {
                throw new IllegalStateException("runtime_previous_download_delete_failed");
            }

            connection = (HttpURLConnection) new URL(RUNTIME_URL).openConnection();
            connection.setInstanceFollowRedirects(true);
            connection.setConnectTimeout(20_000);
            connection.setReadTimeout(45_000);
            connection.setRequestProperty("Accept", "application/vnd.android.package-archive,application/octet-stream,*/*");
            connection.setRequestProperty("User-Agent", "VibeCoder-Android-Node-Setup/1");
            int status = connection.getResponseCode();
            if (status < 200 || status >= 300) {
                throw new IllegalStateException("runtime_download_http_" + status);
            }
            long total = connection.getContentLengthLong();
            long downloaded = 0L;
            long lastEmitAt = 0L;
            try (InputStream input = new BufferedInputStream(connection.getInputStream(), BUFFER_SIZE);
                 OutputStream output = new BufferedOutputStream(new FileOutputStream(temp), BUFFER_SIZE)) {
                byte[] buffer = new byte[BUFFER_SIZE];
                while (true) {
                    if (cancelRequested.get()) throw new CancelledException();
                    int read = input.read(buffer);
                    if (read < 0) break;
                    output.write(buffer, 0, read);
                    downloaded += read;
                    long now = android.os.SystemClock.elapsedRealtime();
                    if (now - lastEmitAt >= 250L || (total > 0L && downloaded >= total)) {
                        lastEmitAt = now;
                        listener.onNodeRuntimeState(new State("downloading", downloaded, total, null, null));
                    }
                }
                output.flush();
            }
            if (downloaded <= 0L) throw new IllegalStateException("runtime_download_empty");
            if (total > 0L && downloaded != total) {
                throw new IllegalStateException("runtime_download_size_mismatch");
            }
            if (!temp.renameTo(target)) {
                copyAndReplace(temp, target);
            }
            validateRuntimeApk(target);
            downloadedRuntimeApk = target;
            listener.onNodeRuntimeState(new State("downloaded", downloaded, downloaded, null, null));
            activity.runOnUiThread(() -> continueInstallAfterDownload(target));
        } catch (CancelledException cancelled) {
            if (temp != null) temp.delete();
            emitTerminal(new State("cancelled", 0L, 0L, null, null));
        } catch (Exception error) {
            if (temp != null) temp.delete();
            emitTerminal(new State("failed", 0L, 0L, safeError(error), null));
        } finally {
            if (connection != null) connection.disconnect();
        }
    }

    private void continueInstallAfterDownload(File runtimeApk) {
        if (closed || cancelRequested.get()) {
            emitTerminal(new State("cancelled", 0L, 0L, null, null));
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                && !activity.getPackageManager().canRequestPackageInstalls()) {
            listener.onNodeRuntimeState(new State("awaiting_permission", 0L, 0L, null, null));
            Intent permission = new Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:" + activity.getPackageName()));
            activity.startActivityForResult(permission, UNKNOWN_SOURCES_REQUEST_CODE);
            return;
        }
        installRuntime(runtimeApk);
    }

    private void installRuntime(File runtimeApk) {
        if (closed) return;
        try {
            validateRuntimeApk(runtimeApk);
            PackageInstaller installer = activity.getPackageManager().getPackageInstaller();
            PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(
                    PackageInstaller.SessionParams.MODE_INHERIT_EXISTING);
            params.setAppPackageName(activity.getPackageName());
            installSessionId = installer.createSession(params);
            listener.onNodeRuntimeState(new State("installing", runtimeApk.length(), runtimeApk.length(), null, null));
            try (PackageInstaller.Session session = installer.openSession(installSessionId)) {
                try (InputStream input = new BufferedInputStream(new FileInputStream(runtimeApk), BUFFER_SIZE);
                     OutputStream output = session.openWrite(RUNTIME_APK_NAME, 0L, runtimeApk.length())) {
                    byte[] buffer = new byte[BUFFER_SIZE];
                    while (true) {
                        if (cancelRequested.get()) throw new CancelledException();
                        int read = input.read(buffer);
                        if (read < 0) break;
                        output.write(buffer, 0, read);
                    }
                    session.fsync(output);
                }
                Intent result = new Intent(activity, MainActivity.class)
                        .setAction(ACTION_INSTALL_RESULT)
                        .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                PendingIntent pending = PendingIntent.getActivity(
                        activity,
                        installSessionId,
                        result,
                        PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
                session.commit(pending.getIntentSender());
            }
        } catch (CancelledException cancelled) {
            emitTerminal(new State("cancelled", 0L, 0L, null, null));
        } catch (Exception error) {
            emitTerminal(new State("failed", 0L, 0L, safeError(error), null));
        }
    }

    private void validateRuntimeApk(File runtimeApk) throws Exception {
        if (!runtimeApk.isFile() || runtimeApk.length() <= 0L) {
            throw new IllegalStateException("node_runtime_apk_missing_or_empty");
        }
        try (ZipFile zip = new ZipFile(runtimeApk)) {
            ZipEntry node = zip.getEntry("lib/arm64-v8a/" + NODE_FILE_NAME);
            if (node == null || node.getSize() == 0L) {
                throw new IllegalStateException("node_runtime_apk_payload_missing");
            }
        }

        PackageManager packageManager = activity.getPackageManager();
        PackageInfo archive = packageManager.getPackageArchiveInfo(
                runtimeApk.getAbsolutePath(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (archive == null || !activity.getPackageName().equals(archive.packageName)) {
            throw new IllegalStateException("node_runtime_apk_package_mismatch");
        }
        PackageInfo installed = packageManager.getPackageInfo(
                activity.getPackageName(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (archive.getLongVersionCode() != installed.getLongVersionCode()) {
            throw new IllegalStateException("node_runtime_apk_version_mismatch");
        }
        if (!Arrays.deepEquals(signingDigests(archive), signingDigests(installed))) {
            throw new IllegalStateException("node_runtime_apk_signing_mismatch");
        }
    }

    private File resolveInstalledNodeDirectory() {
        try {
            ClassLoader loader = activity.getClassLoader();
            if (loader instanceof BaseDexClassLoader) {
                String absolutePath = ((BaseDexClassLoader) loader).findLibrary(NODE_LIBRARY_NAME);
                File resolved = validatedNodeExecutable(absolutePath == null ? null : new File(absolutePath));
                if (resolved != null) return resolved.getParentFile();
            }
            ApplicationInfo info = activity.getPackageManager().getApplicationInfo(activity.getPackageName(), 0);
            if (info.nativeLibraryDir != null && !info.nativeLibraryDir.isEmpty()) {
                File resolved = validatedNodeExecutable(new File(info.nativeLibraryDir, NODE_FILE_NAME));
                if (resolved != null) return resolved.getParentFile();
            }
            return null;
        } catch (Exception ignored) {
            return null;
        }
    }

    private boolean isRuntimeSplitInstalled() {
        try {
            PackageInfo info = activity.getPackageManager().getPackageInfo(activity.getPackageName(), 0);
            if (info.splitNames == null) return false;
            for (String split : info.splitNames) {
                if (RUNTIME_SPLIT_NAME.equals(split)) return true;
            }
        } catch (Exception ignored) {
            // Treat lookup failure as not installed and let setup retry.
        }
        return false;
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

    @SuppressWarnings("deprecation")
    private static Intent extractConfirmationIntent(Intent result) {
        if (Build.VERSION.SDK_INT >= 33) {
            return result.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class);
        }
        return result.getParcelableExtra(Intent.EXTRA_INTENT);
    }

    private static byte[][] signingDigests(PackageInfo info) throws Exception {
        if (info.signingInfo == null) return new byte[0][];
        Signature[] signers = info.signingInfo.getApkContentsSigners();
        byte[][] digests = new byte[signers.length][];
        for (int i = 0; i < signers.length; i++) {
            digests[i] = MessageDigest.getInstance("SHA-256").digest(signers[i].toByteArray());
        }
        Arrays.sort(digests, (left, right) -> {
            int count = Math.min(left.length, right.length);
            for (int i = 0; i < count; i++) {
                int a = left[i] & 0xff;
                int b = right[i] & 0xff;
                if (a != b) return Integer.compare(a, b);
            }
            return Integer.compare(left.length, right.length);
        });
        return digests;
    }

    private void restartApplication() {
        activity.runOnUiThread(() -> {
            Intent launch = activity.getPackageManager().getLaunchIntentForPackage(activity.getPackageName());
            if (launch == null) {
                emitTerminal(new State("failed", 0L, 0L, "node_runtime_restart_intent_missing", null));
                return;
            }
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK);
            activity.startActivity(launch);
            activity.finishAffinity();
        });
    }

    private void emitTerminal(State state) {
        downloadActive.set(false);
        installSessionId = -1;
        listener.onNodeRuntimeState(state);
    }

    private void cleanupDownload() {
        File file = downloadedRuntimeApk;
        downloadedRuntimeApk = null;
        if (file != null) file.delete();
    }

    private static void copyAndReplace(File source, File target) throws Exception {
        try (InputStream input = new FileInputStream(source);
             OutputStream output = new FileOutputStream(target)) {
            byte[] buffer = new byte[BUFFER_SIZE];
            for (int read; (read = input.read(buffer)) >= 0; ) {
                if (read > 0) output.write(buffer, 0, read);
            }
        }
        if (!source.delete()) source.deleteOnExit();
    }

    private static String safeSuffix(String message) {
        if (message == null || message.isEmpty()) return "";
        String clean = message.replaceAll("[^A-Za-z0-9._-]+", "_");
        return clean.isEmpty() ? "" : "_" + clean.substring(0, Math.min(clean.length(), 80));
    }

    private static String safeError(Exception error) {
        if (error instanceof CancelledException) return "node_runtime_setup_cancelled";
        String message = error.getMessage();
        if (message == null || message.isEmpty()) return "node_runtime_setup_failed";
        if (message.matches("[A-Za-z0-9._:-]{1,160}")) return message;
        return error.getClass().getSimpleName() + "_" + safeSuffix(message);
    }

    @Override
    public void close() {
        closed = true;
        cancelRequested.set(true);
        downloadExecutor.shutdownNow();
    }

    private static final class CancelledException extends Exception {
        private static final long serialVersionUID = 1L;
    }
}
