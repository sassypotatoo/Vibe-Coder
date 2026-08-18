package com.vibecoder.shell;

import android.app.Activity;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInstaller;
import android.net.Uri;
import android.provider.Settings;

import dalvik.system.BaseDexClassLoader;

import org.json.JSONObject;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

final class JcodeRuntimeDeliveryManager implements AutoCloseable {
    static final String MODULE_NAME = "jcode_runtime";
    static final String JCODE_FILE_NAME = "libvibecoder_jcode_exec.so";
    static final String JCODE_VERSION = "0.73.0";
    private static final String JCODE_LIBRARY_NAME = "vibecoder_jcode_exec";
    private static final String DESCRIPTOR_ASSET = "runtime/jcode-runtime-download.json";
    private static final int INSTALL_PERMISSION_REQUEST = 7300;

    interface Listener { void onJcodeRuntimeState(State state); }

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

    private static final class Descriptor {
        final String url;
        final long maxBytes;
        final int versionCode;
        Descriptor(String url, long maxBytes, int versionCode) {
            this.url = url;
            this.maxBytes = maxBytes;
            this.versionCode = versionCode;
        }
    }

    private final Activity activity;
    private final Listener listener;
    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final AtomicBoolean cancelled = new AtomicBoolean(false);
    private volatile int activeSessionId;
    private volatile boolean waitingForInstallPermission;

    JcodeRuntimeDeliveryManager(Activity activity, Listener listener) {
        this.activity = activity;
        this.listener = listener;
    }

    State currentState() {
        File directory = resolveInstalledJcodeDirectory();
        if (directory != null) return new State("ready", 0L, 0L, null, directory);
        String persisted = activity.getSharedPreferences(
                JcodeRuntimeInstallReceiver.PREFS, Context.MODE_PRIVATE)
                .getString(JcodeRuntimeInstallReceiver.KEY_STATUS, "");
        String message = activity.getSharedPreferences(
                JcodeRuntimeInstallReceiver.PREFS, Context.MODE_PRIVATE)
                .getString(JcodeRuntimeInstallReceiver.KEY_MESSAGE, "");
        if (persisted.startsWith("failed_")) {
            return new State("failed", 0L, 0L,
                    message.isEmpty() ? "jcode_runtime_split_install_failed" : message, null);
        }
        if ("awaiting_user_confirmation".equals(persisted)) {
            return new State("installing", 0L, 0L, null, null);
        }
        if ("installed".equals(persisted)) {
            // PackageInstaller succeeded, but the current process may still hold the pre-split
            // class-loader/native-library path. Do not download the runtime again. A clean process
            // restart lets Android rebuild the package split search path.
            return new State("restart_required", 0L, 0L, null, null);
        }
        return new State("not_installed", 0L, 0L, null, null);
    }

    @SuppressWarnings("deprecation")
    void startInstall() {
        State state = currentState();
        if (state.ready()) {
            listener.onJcodeRuntimeState(state);
            return;
        }
        if ("restart_required".equals(state.status)) {
            activity.finishAffinity();
            return;
        }
        if (!activity.getPackageManager().canRequestPackageInstalls()) {
            waitingForInstallPermission = true;
            listener.onJcodeRuntimeState(new State("permission_required", 0L, 0L, null, null));
            Intent settings = new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:" + activity.getPackageName()));
            activity.startActivityForResult(settings, INSTALL_PERMISSION_REQUEST);
            return;
        }
        beginDownload();
    }

    void refreshAfterResume() {
        State state = currentState();
        if (state.ready()) {
            listener.onJcodeRuntimeState(state);
            return;
        }
        if (waitingForInstallPermission && activity.getPackageManager().canRequestPackageInstalls()) {
            waitingForInstallPermission = false;
            beginDownload();
            return;
        }
        listener.onJcodeRuntimeState(state);
    }

    void cancelInstall() {
        cancelled.set(true);
        int sessionId = activeSessionId;
        if (sessionId > 0) {
            try { activity.getPackageManager().getPackageInstaller().abandonSession(sessionId); }
            catch (Exception ignored) { }
        }
        activeSessionId = 0;
        listener.onJcodeRuntimeState(new State("cancelled", 0L, 0L, null, null));
    }

    private void beginDownload() {
        cancelled.set(false);
        activity.getSharedPreferences(JcodeRuntimeInstallReceiver.PREFS, Context.MODE_PRIVATE)
                .edit().clear().apply();
        listener.onJcodeRuntimeState(new State("pending", 0L, 0L, null, null));
        executor.execute(() -> {
            File downloaded = null;
            try {
                Descriptor descriptor = readDescriptor();
                if (descriptor.versionCode != getInstalledVersionCode()) {
                    throw new IllegalStateException("jcode_runtime_base_version_mismatch");
                }
                downloaded = downloadSplit(descriptor);
                if (cancelled.get()) return;
                commitSplit(downloaded);
            } catch (Throwable error) {
                if (!cancelled.get()) {
                    listener.onJcodeRuntimeState(new State(
                            "failed", 0L, 0L, safeError(error), null));
                }
            } finally {
                if (downloaded != null && downloaded.exists()) downloaded.delete();
            }
        });
    }

    private Descriptor readDescriptor() throws Exception {
        byte[] data;
        try (InputStream input = activity.getAssets().open(DESCRIPTOR_ASSET)) {
            data = readAll(input, 32 * 1024L);
        }
        JSONObject json = new JSONObject(new String(data, StandardCharsets.UTF_8));
        if (json.optInt("schema", 0) != 1
                || !"jcode".equals(json.optString("component_id", ""))
                || !JCODE_VERSION.equals(json.optString("version", ""))
                || !activity.getPackageName().equals(json.optString("application_id", ""))
                || !MODULE_NAME.equals(json.optString("split_name", ""))
                || !"arm64-v8a".equals(json.optString("abi", ""))) {
            throw new IllegalStateException("jcode_runtime_descriptor_invalid");
        }
        String url = json.optString("download_url", "");
        if (!url.startsWith("https://github.com/sassypotatoo/Vibe-Coder/releases/download/")) {
            throw new IllegalStateException("jcode_runtime_download_url_untrusted");
        }
        long maxBytes = json.optLong("max_download_bytes", 0L);
        if (maxBytes < 1024L || maxBytes > 256L * 1024L * 1024L) {
            throw new IllegalStateException("jcode_runtime_max_download_invalid");
        }
        return new Descriptor(url, maxBytes, json.optInt("base_version_code", -1));
    }

    private File downloadSplit(Descriptor descriptor) throws Exception {
        File target = new File(activity.getCacheDir(), "jcode-runtime-download.apk");
        if (target.exists() && !target.delete()) {
            throw new IllegalStateException("jcode_runtime_stale_download_delete_failed");
        }
        HttpURLConnection connection = (HttpURLConnection) new URL(descriptor.url).openConnection();
        connection.setConnectTimeout(20_000);
        connection.setReadTimeout(30_000);
        connection.setInstanceFollowRedirects(true);
        connection.setRequestProperty("User-Agent", "VibeCoder-Development-Runtime/1");
        connection.connect();
        int code = connection.getResponseCode();
        if (code < 200 || code >= 300) {
            connection.disconnect();
            throw new IllegalStateException("jcode_runtime_download_http_" + code);
        }
        long total = connection.getContentLengthLong();
        if (total > descriptor.maxBytes) {
            connection.disconnect();
            throw new IllegalStateException("jcode_runtime_download_too_large");
        }
        long downloaded = 0L;
        byte[] buffer = new byte[128 * 1024];
        try (InputStream input = new BufferedInputStream(connection.getInputStream());
             FileOutputStream output = new FileOutputStream(target)) {
            while (true) {
                if (cancelled.get()) throw new InterruptedException("jcode_runtime_download_cancelled");
                int read = input.read(buffer);
                if (read < 0) break;
                if (read == 0) continue;
                downloaded += read;
                if (downloaded > descriptor.maxBytes) {
                    throw new IllegalStateException("jcode_runtime_download_too_large");
                }
                output.write(buffer, 0, read);
                listener.onJcodeRuntimeState(new State("downloading", downloaded, total, null, null));
            }
            output.getFD().sync();
        } finally {
            connection.disconnect();
        }
        if (downloaded <= 0L) throw new IllegalStateException("jcode_runtime_download_empty");
        return target;
    }

    private void commitSplit(File splitApk) throws Exception {
        PackageInstaller installer = activity.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(
                PackageInstaller.SessionParams.MODE_INHERIT_EXISTING);
        params.setAppPackageName(activity.getPackageName());
        params.setInstallReason(android.content.pm.PackageManager.INSTALL_REASON_USER);
        params.setSize(splitApk.length());
        int sessionId = installer.createSession(params);
        activeSessionId = sessionId;
        try (PackageInstaller.Session session = installer.openSession(sessionId);
             OutputStream output = session.openWrite("jcode_runtime.apk", 0L, splitApk.length());
             InputStream input = new BufferedInputStream(new java.io.FileInputStream(splitApk))) {
            byte[] buffer = new byte[128 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) {
                if (read > 0) output.write(buffer, 0, read);
            }
            session.fsync(output);
            Intent callback = new Intent(activity, JcodeRuntimeInstallReceiver.class)
                    .setAction(JcodeRuntimeInstallReceiver.ACTION);
            PendingIntent pending = PendingIntent.getBroadcast(
                    activity, sessionId, callback,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            listener.onJcodeRuntimeState(new State("installing", splitApk.length(), splitApk.length(), null, null));
            session.commit(pending.getIntentSender());
        } catch (Throwable error) {
            try { installer.abandonSession(sessionId); } catch (Exception ignored) { }
            activeSessionId = 0;
            throw error;
        }
    }

    private File resolveInstalledJcodeDirectory() {
        try {
            Context fresh = activity.createPackageContext(activity.getPackageName(), 0);
            ClassLoader loader = fresh.getClassLoader();
            if (loader instanceof BaseDexClassLoader) {
                String absolutePath = ((BaseDexClassLoader) loader).findLibrary(JCODE_LIBRARY_NAME);
                File resolved = validatedJcodeExecutable(absolutePath == null ? null : new File(absolutePath));
                if (resolved != null) return resolved.getParentFile();
            }
            ApplicationInfo info = fresh.getApplicationInfo();
            if (info.splitSourceDirs == null) return null;
            return null;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static File validatedJcodeExecutable(File candidate) {
        if (candidate == null) return null;
        try {
            File file = candidate.getCanonicalFile();
            File parent = file.getParentFile();
            if (parent == null || !JCODE_FILE_NAME.equals(file.getName())
                    || !file.isFile() || !file.canExecute()) return null;
            return file;
        } catch (Exception ignored) {
            return null;
        }
    }

    @SuppressWarnings("deprecation")
    private int getInstalledVersionCode() throws Exception {
        android.content.pm.PackageInfo info = activity.getPackageManager()
                .getPackageInfo(activity.getPackageName(), 0);
        if (android.os.Build.VERSION.SDK_INT >= 28) {
            long value = info.getLongVersionCode();
            if (value > Integer.MAX_VALUE) throw new IllegalStateException("version_code_too_large");
            return (int) value;
        }
        return info.versionCode;
    }

    private static byte[] readAll(InputStream input, long maxBytes) throws Exception {
        java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        long total = 0L;
        int read;
        while ((read = input.read(buffer)) >= 0) {
            if (read == 0) continue;
            total += read;
            if (total > maxBytes) throw new IllegalStateException("descriptor_too_large");
            out.write(buffer, 0, read);
        }
        return out.toByteArray();
    }

    private static String safeError(Throwable error) {
        String message = error.getMessage();
        return message == null || message.isEmpty()
                ? "jcode_runtime_setup_failed"
                : message.replaceAll("[^A-Za-z0-9_:. -]", "_");
    }

    @Override
    public void close() {
        cancelled.set(true);
        executor.shutdownNow();
    }
}
