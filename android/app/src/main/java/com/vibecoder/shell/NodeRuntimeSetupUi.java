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

final class NodeRuntimeSetupUi {
    interface Callbacks {
        void onStartSetup();
        void onCancelSetup();
    }

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

    NodeRuntimeSetupUi(Activity activity, Callbacks callbacks) {
        root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER_HORIZONTAL);
        root.setPadding(48, 72, 48, 48);
        root.setBackgroundColor(0xff111318);

        TextView title = text(activity, "Set up VibeCoder", 26, true);
        root.addView(title, matchWrap());

        TextView intro = text(activity,
                "One runtime is downloaded once before local AI starts.", 16, false);
        intro.setPadding(0, 18, 0, 30);
        root.addView(intro, matchWrap());

        TextView requirements = text(activity,
                "Already inside the app\n✓ Jcode\n✓ OmniRoute\n\nFirst-time setup\n⬇ Node.js Android Runtime "
                        + NodeRuntimeDeliveryManager.NODE_VERSION,
                16,
                false);
        root.addView(requirements, matchWrap());

        status = text(activity, "Node.js runtime is ready to download.", 15, true);
        status.setPadding(0, 36, 0, 12);
        root.addView(status, matchWrap());

        progress = new ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        progress.setProgress(0);
        root.addView(progress, new LinearLayout.LayoutParams(-1, 28));

        progressText = text(activity, "0%", 15, true);
        progressText.setPadding(0, 12, 0, 4);
        root.addView(progressText, matchWrap());

        bytesText = text(activity,
                "The runtime is downloaded directly from the VibeCoder GitHub release.", 13, false);
        root.addView(bytesText, matchWrap());

        primary = new Button(activity);
        primary.setText("Download & Set Up Node.js");
        primary.setOnClickListener(v -> callbacks.onStartSetup());
        LinearLayout.LayoutParams buttonParams = matchWrap();
        buttonParams.topMargin = 32;
        root.addView(primary, buttonParams);

        cancel = new Button(activity);
        cancel.setText("Cancel");
        cancel.setVisibility(View.GONE);
        cancel.setOnClickListener(v -> callbacks.onCancelSetup());
        root.addView(cancel, matchWrap());
    }

    View root() { return root; }
    void show() { root.setVisibility(View.VISIBLE); }
    void hide() { root.setVisibility(View.GONE); }

    void render(NodeRuntimeDeliveryManager.State state) {
        if (state.ready()) {
            status.setText("Node.js runtime ready ✓");
            progress.setProgress(100);
            progressText.setText("100%");
            bytesText.setText("Setup complete. Starting VibeCoder…");
            primary.setEnabled(false);
            cancel.setVisibility(View.GONE);
            return;
        }

        boolean active = "preparing".equals(state.status)
                || "waiting_for_release".equals(state.status)
                || "downloading".equals(state.status)
                || "downloaded".equals(state.status)
                || "awaiting_permission".equals(state.status)
                || "awaiting_confirmation".equals(state.status)
                || "installing".equals(state.status)
                || "restart_required".equals(state.status)
                || "cancelling".equals(state.status);
        primary.setEnabled(!active);
        primary.setText("failed".equals(state.status) || "cancelled".equals(state.status)
                ? "Retry Node.js Setup"
                : "Download & Set Up Node.js");
        cancel.setVisibility(("downloading".equals(state.status)
                || "waiting_for_release".equals(state.status)
                || "preparing".equals(state.status))
                ? View.VISIBLE : View.GONE);

        int percent = state.totalBytes > 0L ? state.percent : 0;
        progress.setProgress(percent);
        progressText.setText(state.totalBytes > 0L ? percent + "%" : statusProgressText(state.status));

        if (state.totalBytes > 0L) {
            long remaining = Math.max(0L, state.totalBytes - state.downloadedBytes);
            updateRateSample(state);
            String rateAndEta = formatRateAndEta(remaining);
            bytesText.setText(String.format(Locale.ROOT, "%s / %s · %s remaining%s",
                    formatBytes(state.downloadedBytes),
                    formatBytes(state.totalBytes),
                    formatBytes(remaining),
                    rateAndEta));
        } else {
            resetRateSample();
            bytesText.setText(descriptionFor(state.status));
        }

        switch (state.status) {
            case "preparing": status.setText("Preparing Node.js download…"); break;
            case "waiting_for_release": status.setText("Node.js release is still becoming available. Retrying…"); break;
            case "downloading": status.setText("Downloading Node.js runtime…"); break;
            case "downloaded": status.setText("Download complete. Verifying runtime…"); break;
            case "awaiting_permission": status.setText("Allow VibeCoder to install its Node.js runtime once."); break;
            case "awaiting_confirmation": status.setText("Confirm the Node.js runtime installation."); break;
            case "installing": status.setText("Installing Node.js runtime…"); break;
            case "restart_required": status.setText("Node.js installed. Restarting VibeCoder…"); break;
            case "cancelling": status.setText("Cancelling setup…"); break;
            case "cancelled": status.setText("Node.js setup cancelled."); break;
            case "failed": status.setText("Setup failed: " + safe(state.error)); break;
            default: status.setText("Node.js runtime is required before local AI startup.");
        }
    }

    private static String statusProgressText(String state) {
        if ("waiting_for_release".equals(state)) return "Retrying…";
        if ("installing".equals(state) || "awaiting_confirmation".equals(state)) return "Installing…";
        if ("awaiting_permission".equals(state)) return "Permission required";
        if ("restart_required".equals(state)) return "Restarting…";
        return "0%";
    }

    private static String descriptionFor(String state) {
        if ("waiting_for_release".equals(state)) {
            return "GitHub has not exposed the fixed Node.js release asset yet; VibeCoder is retrying the exact URL.";
        }
        if ("awaiting_permission".equals(state)) {
            return "Android may ask once for permission to install the downloaded VibeCoder runtime.";
        }
        if ("awaiting_confirmation".equals(state)) {
            return "Approve the Android installer prompt; setup continues automatically afterwards.";
        }
        if ("installing".equals(state)) return "Adding Node.js to this VibeCoder installation.";
        if ("restart_required".equals(state)) return "The runtime is installed and will be visible after restart.";
        return "The runtime is downloaded directly from the VibeCoder GitHub release.";
    }

    private void updateRateSample(NodeRuntimeDeliveryManager.State state) {
        long now = SystemClock.elapsedRealtime();
        if (!"downloading".equals(state.status)) {
            if (state.downloadedBytes == 0L) resetRateSample();
            return;
        }
        if (lastSampleBytes >= 0L && state.downloadedBytes >= lastSampleBytes && lastSampleAtMs >= 0L) {
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
        return String.format(Locale.ROOT, "\n%s/s · ~%s remaining",
                formatBytes((long) smoothedBytesPerSecond), eta);
    }

    private void resetRateSample() {
        lastSampleBytes = -1L;
        lastSampleAtMs = -1L;
        smoothedBytesPerSecond = 0.0;
    }

    private static String safe(String value) {
        return value == null || value.isEmpty() ? "unknown_error" : value;
    }

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

    private static LinearLayout.LayoutParams matchWrap() {
        return new LinearLayout.LayoutParams(-1, -2);
    }
}
