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
    private double smoothedBytesPerSecond = 0.0;

    NodeRuntimeSetupUi(Activity activity, Callbacks callbacks) {
        root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER_HORIZONTAL);
        root.setPadding(48, 72, 48, 48);
        root.setBackgroundColor(0xff111318);

        TextView title = text(activity, "Welcome to VibeCoder", 26, true);
        root.addView(title, matchWrap());
        TextView intro = text(activity, "Preparing local coding environment…", 16, false);
        intro.setPadding(0, 18, 0, 32);
        root.addView(intro, matchWrap());

        TextView requirements = text(activity,
                "Required runtime\n✓ Jcode\n✓ OmniRoute\n⬇ Node.js Android Runtime 24.19.0", 16, false);
        root.addView(requirements, matchWrap());

        status = text(activity, "Node.js runtime is not installed.", 15, true);
        status.setPadding(0, 36, 0, 12);
        root.addView(status, matchWrap());

        progress = new ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        progress.setProgress(0);
        root.addView(progress, new LinearLayout.LayoutParams(-1, 28));

        progressText = text(activity, "0%", 15, true);
        progressText.setPadding(0, 12, 0, 4);
        root.addView(progressText, matchWrap());
        bytesText = text(activity, "Download size is provided by Google Play when setup starts.", 13, false);
        root.addView(bytesText, matchWrap());

        primary = new Button(activity);
        primary.setText("Start Setup");
        primary.setOnClickListener(v -> callbacks.onStartSetup());
        LinearLayout.LayoutParams buttonParams = matchWrap();
        buttonParams.topMargin = 32;
        root.addView(primary, buttonParams);

        cancel = new Button(activity);
        cancel.setText("Cancel Download");
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
            primary.setEnabled(false);
            cancel.setVisibility(View.GONE);
            return;
        }
        boolean active = state.status.equals("pending") || state.status.equals("downloading")
                || state.status.equals("installing") || state.status.equals("cancelling");
        primary.setEnabled(!active);
        primary.setText(state.status.equals("failed") || state.status.equals("cancelled") ? "Retry Setup" : "Start Setup");
        cancel.setVisibility(active ? View.VISIBLE : View.GONE);
        int percent = state.totalBytes > 0L ? state.percent : 0;
        progress.setProgress(percent);
        progressText.setText(state.totalBytes > 0L ? percent + "%" : "Waiting for Google Play…");
        if (state.totalBytes > 0L) {
            long remaining = Math.max(0L, state.totalBytes - state.downloadedBytes);
            updateRateSample(state);
            String rateAndEta = formatRateAndEta(remaining);
            bytesText.setText(String.format(Locale.ROOT, "%s / %s · %s remaining%s",
                    formatBytes(state.downloadedBytes), formatBytes(state.totalBytes), formatBytes(remaining), rateAndEta));
        } else {
            resetRateSample();
            bytesText.setText("Download size is provided by Google Play when setup starts.");
        }
        switch (state.status) {
            case "pending": status.setText("Preparing Node.js runtime download…"); break;
            case "downloading": status.setText("Downloading Node.js runtime…"); break;
            case "installing": status.setText("Installing verified Play-delivered runtime…"); break;
            case "cancelling": status.setText("Cancelling setup…"); break;
            case "cancelled": status.setText("Node.js runtime setup cancelled."); break;
            case "failed":
                if ("node_runtime_play_app_not_owned_use_sideload_alpha".equals(state.error)) {
                    status.setText("This base APK cannot download Node from Google Play.");
                    progressText.setText("Use the Sideload Alpha APK");
                    bytesText.setText("The sideload build contains the verified Node.js runtime locally and does not require Play ownership.");
                    primary.setEnabled(false);
                } else if ("node_runtime_play_store_unavailable".equals(state.error)) {
                    status.setText("Google Play is unavailable for Node.js delivery.");
                    bytesText.setText("Install the Sideload Alpha APK for local testing, or install the Play build through Google Play.");
                } else {
                    status.setText("Setup failed: " + safe(state.error));
                }
                break;
            default: status.setText("Node.js runtime is required before local AI startup.");
        }
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

    private static String safe(String value) { return value == null || value.isEmpty() ? "unknown_error" : value; }
    private static String formatBytes(long bytes) {
        double mib = bytes / (1024.0 * 1024.0);
        return String.format(Locale.ROOT, mib >= 10.0 ? "%.0f MB" : "%.1f MB", mib);
    }
    private static TextView text(Activity activity, String value, int sp, boolean bold) {
        TextView view = new TextView(activity);
        view.setText(value); view.setTextSize(sp); view.setTextColor(0xfff4f5f7);
        if (bold) view.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        return view;
    }
    private static LinearLayout.LayoutParams matchWrap() { return new LinearLayout.LayoutParams(-1, -2); }
}
