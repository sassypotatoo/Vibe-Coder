package com.vibecoder.shell;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;

public final class JcodeRuntimeInstallReceiver extends BroadcastReceiver {
    static final String ACTION = "com.vibecoder.shell.JCODE_RUNTIME_INSTALL_STATUS";
    static final String PREFS = "vibecoder_jcode_runtime_install";
    static final String KEY_STATUS = "status";
    static final String KEY_MESSAGE = "message";

    @Override
    @SuppressWarnings("deprecation")
    public void onReceive(Context context, Intent intent) {
        if (intent == null || !ACTION.equals(intent.getAction())) return;
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            Intent confirmation;
            if (android.os.Build.VERSION.SDK_INT >= 33) {
                confirmation = intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class);
            } else {
                confirmation = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            }
            persist(context, "awaiting_user_confirmation", message);
            if (confirmation != null) {
                confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                context.startActivity(confirmation);
            }
            return;
        }
        if (status == PackageInstaller.STATUS_SUCCESS) {
            persist(context, "installed", message);
        } else {
            persist(context, "failed_" + status, message);
        }
    }

    private static void persist(Context context, String status, String message) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit()
                .putString(KEY_STATUS, status)
                .putString(KEY_MESSAGE, message == null ? "" : message)
                .apply();
    }
}
