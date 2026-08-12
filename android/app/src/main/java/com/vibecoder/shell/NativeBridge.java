package com.vibecoder.shell;

final class NativeBridge {
    static {
        System.loadLibrary("vibecoder_shell_jni");
    }

    private NativeBridge() {}

    static native String nativeProbeSnapshot(
            String appPrivateDir,
            String nativeLibraryDir,
            String packagedExecutableDir,
            byte[] inventoryJson,
            byte[] additionalEvidenceJson);
}
