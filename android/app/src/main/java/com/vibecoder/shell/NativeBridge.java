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

    static native String nativeOmniRouteStart(
            String appPrivateDir,
            String nativeLibraryDir,
            String packagedExecutableDir,
            byte[] inventoryJson,
            String expectedManifestSha256);

    static native String nativeOmniRouteStatus();

    static native String nativeOmniRouteGatewayProbe(byte[] credentialUtf8);

    static native String nativeOmniRouteInferenceProbe(
            byte[] credentialUtf8,
            byte[] modelUtf8,
            byte[] promptUtf8);

    static native String nativeOmniRouteStop();

    static native String nativeAppControllerInit(
            String appPrivateDir,
            String nativeLibraryDir,
            String packagedExecutableDir,
            byte[] inventoryJson);

    static native String nativeChatCreate();

    static native String nativeChatSend(
            String projectId,
            String conversationId,
            byte[] promptUtf8);

    static native String nativeChatCancel(String projectId, String conversationId);
}
