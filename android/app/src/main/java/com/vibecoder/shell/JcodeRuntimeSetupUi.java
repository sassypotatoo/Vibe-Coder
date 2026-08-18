package com.vibecoder.shell;

import android.app.Activity;
import android.graphics.Typeface;
import android.os.SystemClock;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

import java.util.Locale;

final class JcodeRuntimeSetupUi {
    interface Callbacks { void onStartSetup(); void onCancelSetup(); }

    private final LinearLayout root;
    private final TextView status;
    private final TextView progressText;
    private final TextView bytesText;
    private final ProgressBar progress;
    private final Button primary;
    private final Button cancel;
    private long lastSampleBytes = -1L;
    private long lastSampleAtMs = -1L;
    private double smoothedBytesPerSecond;

    JcodeRuntimeSetupUi(Activity activity, Callbacks callbacks) {
        root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER_HORIZONTAL);
        root.setPadding(48, 72, 48, 48);
        root.setBackgroundColor(0xff111318);

        root.addView(text(activity, "Welcome to VibeCoder", 26, true), matchWrap());
        TextView intro = text(activity, "Preparing local coding environment…", 16, false);
        intro.setPadding(0, 18, 0, 32);
        root.addView(intro, matchWrap());

        TextView requirements = text(activity,
                "Required runtime\n✓ Node.js Android Runtime 24.19.0\n✓ OmniRoute\n⬇ Jcode 0.73.0", 16, false);
        root.addView(requirements, matchWrap());

        status = text(activity, "Jcode runtime needs to be downloaded once.", 15, true);
        status.setPadding(0, 36, 0, 12);
        root.addView(status, matchWrap());

        progress = new ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress, new LinearLayout.LayoutParams(-1, 28));
        progressText = text(activity, "0%", 15, true);
        progressText.setPadding(0, 12, 0, 4);
        root.addView(progressText, matchWrap());
        bytesText = text(activity, "Jcode is downloaded from the VibeCoder GitHub runtime release.", 13, false);
        root.addView(bytesText, matchWrap());

        primary = new Button(activity);
        primary.setText("START SETUP");
        primary.setOnClickListener(v -> callbacks.onStartSetup());
        LinearLayout.LayoutParams buttonParams = matchWrap();
        buttonParams.topMargin = 32;
        root.addView(primary, buttonParams);

        cancel = new Button(activity);
        cancel.setText("CANCEL");
        cancel.setVisibility(View.GONE);
        cancel.setOnClickListener(v -> callbacks.onCancelSetup());
        root.addView(cancel, matchWrap());
    }

    View root() { return root; }
    void show() { root.setVisibility(View.VISIBLE); }
    void hide() { root.setVisibility(View.GONE); }

    void render(JcodeRuntimeDeliveryManager.State state) {
        if (state.ready()) {
            status.setText("Jcode runtime ready ✓");
            progress.setProgress(100);
            progressText.setText("100%");
            primary.setEnabled(false);
            cancel.setVisibility(View.GONE);
            return;
        }
        boolean active = "pending".equals(state.status) || "downloading".equals(state.status)
                || "installing".equals(state.status);
        primary.setEnabled(!active);
        primary.setText("permission_required".equals(state.status)
                ? "ALLOW RUNTIME INSTALL" : ("restart_required".equals(state.status)
                ? "CLOSE APP, THEN REOPEN" : ("failed".equals(state.status) || "cancelled".equals(state.status)
                ? "RETRY SETUP" : "START SETUP")));
        cancel.setVisibility(active ? View.VISIBLE : View.GONE);
        progress.setProgress(state.totalBytes > 0L ? state.percent : 0);
        progressText.setText(state.totalBytes > 0L ? state.percent + "%" : "Preparing…");
        if (state.totalBytes > 0L) {
            updateRateSample(state);
            long remaining = Math.max(0L, state.totalBytes - state.downloadedBytes);
            bytesText.setText(String.format(Locale.ROOT, "%s / %s%s",
                    formatBytes(state.downloadedBytes), formatBytes(state.totalBytes), formatRateAndEta(remaining)));
        } else {
            resetRateSample();
            bytesText.setText("Jcode is downloaded once and installed as a signed VibeCoder runtime split.");
        }
        switch (state.status) {
            case "pending": status.setText("Preparing Jcode runtime download…"); break;
            case "downloading": status.setText("Downloading Jcode runtime…"); break;
            case "installing": status.setText("Android is installing Jcode runtime… VibeCoder may restart once."); break;
            case "permission_required": status.setText("Allow VibeCoder to install its Jcode runtime, then return here."); break;
            case "restart_required": status.setText("Jcode installed ✓ Close VibeCoder once, then reopen it to activate the runtime."); break;
            case "cancelled": status.setText("Jcode runtime setup cancelled."); break;
            case "failed": status.setText("Jcode setup failed: " + safe(state.error)); break;
            default: status.setText("Jcode runtime needs to be downloaded once.");
        }
    }

    private void updateRateSample(JcodeRuntimeDeliveryManager.State state) {
        long now = SystemClock.elapsedRealtime();
        if (!"downloading".equals(state.status)) return;
        if (lastSampleBytes >= 0L && lastSampleAtMs >= 0L && state.downloadedBytes >= lastSampleBytes) {
            long elapsedMs = now - lastSampleAtMs;
            long deltaBytes = state.downloadedBytes - lastSampleBytes;
            if (elapsedMs >= 250L && deltaBytes > 0L) {
                double current = deltaBytes * 1000.0 / elapsedMs;
                smoothedBytesPerSecond = smoothedBytesPerSecond <= 0.0
                        ? current : (smoothedBytesPerSecond * 0.65) + (current * 0.35);
            }
        }
        lastSampleBytes = state.downloadedBytes;
        lastSampleAtMs = now;
    }

    private String formatRateAndEta(long remainingBytes) {
        if (smoothedBytesPerSecond < 1024.0) return "";
        long etaSeconds = (long) Math.ceil(remainingBytes / smoothedBytesPerSecond);
        String eta = etaSeconds < 60L ? etaSeconds + "s" : ((etaSeconds + 30L) / 60L) + "m";
        return String.format(Locale.ROOT, " · %s/s · ~%s", formatBytes((long) smoothedBytesPerSecond), eta);
    }

    private void resetRateSample() {
        lastSampleBytes = -1L;
        lastSampleAtMs = -1L;
        smoothedBytesPerSecond = 0.0;
    }

    private static String safe(String value) { return value == null || value.isEmpty() ? "unknown_error" : value; }
    private static String formatBytes(long bytes) {
        double mib = bytes / (1024.0 * 1024.0);
        return String.format(Locale.ROOT, mib >= 10.0 ? "%.0f MB" : "%.1f MB", mib);
    }
    private static TextView text(Activity activity, String value, int sp, boolean bold) {
        TextView view = new TextView(activity);
        view.setText(value);
        view.setTextSize(sp);
        view.setTextColor(0xfff4f5f7);
        if (bold) view.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        return view;
    }
    private static LinearLayout.LayoutParams matchWrap() { return new LinearLayout.LayoutParams(-1, -2); }
}
