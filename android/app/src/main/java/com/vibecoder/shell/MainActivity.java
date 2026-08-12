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
    private final ExecutorService diagnosticsExecutor = Executors.newSingleThreadExecutor();
    private final AtomicBoolean diagnosticRunning = new AtomicBoolean(false);
    private TextView summary;
    private TextView details;
    private Button rerun;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(buildUi());
        runDiagnostics();
    }

    private View buildUi() {
        int pad = dp(18);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(pad, pad, pad, pad);

        TextView title = new TextView(this);
        title.setText("VibeCoder Runtime Test");
        title.setTextSize(28f);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        root.addView(title);

        TextView subtitle = new TextView(this);
        subtitle.setText("Part 31 first-APK shell · build and device proof");
        subtitle.setTextSize(15f);
        subtitle.setPadding(0, dp(4), 0, dp(18));
        root.addView(subtitle);

        summary = new TextView(this);
        summary.setTextSize(17f);
        summary.setTypeface(Typeface.MONOSPACE);
        summary.setPadding(0, 0, 0, dp(16));
        root.addView(summary);

        rerun = new Button(this);
        rerun.setText("Run runtime checks");
        rerun.setAllCaps(false);
        rerun.setOnClickListener(v -> runDiagnostics());
        root.addView(rerun);

        details = new TextView(this);
        details.setTextSize(12f);
        details.setTypeface(Typeface.MONOSPACE);
        details.setTextIsSelectable(true);
        details.setPadding(0, dp(14), 0, dp(30));
        root.addView(details);

        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.addView(root);
        return scroll;
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
            persistDiagnosticReport(buildDiagnosticReport(arm64, json, null));
            boolean nativeLoaded = json.optBoolean("native_loaded", false);
            boolean probeOk = json.optBoolean("probe_ok", false);
            JSONObject readiness = json.optJSONObject("readiness");

            StringBuilder status = new StringBuilder();
            status.append(line("Device ARM64", arm64));
            status.append(line("JNI bridge", true));
            status.append(line("Rust host", nativeLoaded));
            status.append(line("Host probe", probeOk));
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
                    expectedPayloads(nativeRoot) + "\n\nProbe snapshot:\n" + pretty(snapshot);
            return new DiagnosticResult(status.toString(), diagnosticDetails);
        } catch (Throwable error) {
            try {
                persistDiagnosticReport(buildDiagnosticReport(arm64, null, error));
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
        diagnosticsExecutor.shutdownNow();
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

    private JSONObject buildDiagnosticReport(boolean arm64, JSONObject snapshot, Throwable error) throws Exception {
        JSONObject report = new JSONObject();
        report.put("schema", 1);
        report.put("part", 31);
        report.put("package", getPackageName());
        report.put("sdk_int", Build.VERSION.SDK_INT);
        report.put("device_arm64", arm64);
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
    private static final class DiagnosticResult {
        final String summary;
        final String details;

        DiagnosticResult(String summary, String details) {
            this.summary = summary;
            this.details = details;
        }
    }

}
