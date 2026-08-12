#include <jni.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdlib.h>

// The Rust host is resolved once when the JNI bridge loads. We intentionally keep RTLD_LOCAL:
// VibeCoder does not make Rust host symbols part of the process-global plugin namespace.
typedef uint32_t (*abi_version_fn)(void);
typedef int64_t (*probe_snapshot_v2_fn)(
        const char *,
        const char *,
        const char *,
        const uint8_t *,
        size_t,
        const uint8_t *,
        size_t,
        uint8_t *,
        size_t);

static void *rust_host_handle = NULL;
static abi_version_fn rust_host_abi_version = NULL;
static probe_snapshot_v2_fn rust_host_probe_snapshot = NULL;

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void) vm;
    (void) reserved;
    rust_host_handle = dlopen("libvibecoder_android_host.so", RTLD_NOW | RTLD_LOCAL);
    if (rust_host_handle != NULL) {
        rust_host_abi_version =
                (abi_version_fn) dlsym(rust_host_handle, "vibecoder_android_host_abi_version");
        rust_host_probe_snapshot = (probe_snapshot_v2_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_probe_snapshot_json_v2");
    }
    // Loading the JNI bridge itself must remain diagnosable even when the Rust host is absent or
    // incompatible; nativeProbeSnapshot reports that state instead of crashing class loading.
    return JNI_VERSION_1_6;
}

static jstring literal(JNIEnv *env, const char *value) {
    return (*env)->NewStringUTF(env, value);
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeProbeSnapshot(
        JNIEnv *env,
        jclass clazz,
        jstring app_private_dir,
        jstring native_library_dir,
        jstring packaged_executable_dir,
        jbyteArray inventory_json,
        jbyteArray additional_evidence_json) {
    (void) clazz;
    if (app_private_dir == NULL || native_library_dir == NULL ||
        packaged_executable_dir == NULL || inventory_json == NULL ||
        additional_evidence_json == NULL) {
        return literal(env, "{\"schema\":1,\"native_loaded\":false,\"probe_ok\":false,\"error\":\"jni_invalid_arguments\"}");
    }

    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_probe_snapshot == NULL) {
        return literal(env, "{\"schema\":1,\"native_loaded\":false,\"probe_ok\":false,\"error\":\"rust_host_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"rust_host_abi_mismatch\"}");
    }

    const char *app = (*env)->GetStringUTFChars(env, app_private_dir, NULL);
    const char *native_dir = (*env)->GetStringUTFChars(env, native_library_dir, NULL);
    const char *exec_dir = (*env)->GetStringUTFChars(env, packaged_executable_dir, NULL);
    jbyte *inventory = (*env)->GetByteArrayElements(env, inventory_json, NULL);
    jbyte *additional = (*env)->GetByteArrayElements(env, additional_evidence_json, NULL);
    const jsize inventory_len = (*env)->GetArrayLength(env, inventory_json);
    const jsize additional_len = (*env)->GetArrayLength(env, additional_evidence_json);
    if (app == NULL || native_dir == NULL || exec_dir == NULL || inventory == NULL || additional == NULL) {
        if (app != NULL) (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
        if (native_dir != NULL) (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
        if (exec_dir != NULL) (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
        if (inventory != NULL) (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
        if (additional != NULL) (*env)->ReleaseByteArrayElements(env, additional_evidence_json, additional, JNI_ABORT);
        return literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"jni_argument_access_failed\"}");
    }

    int64_t required = rust_host_probe_snapshot(
            app,
            native_dir,
            exec_dir,
            (const uint8_t *) inventory,
            (size_t) inventory_len,
            (const uint8_t *) additional,
            (size_t) additional_len,
            NULL,
            0u);
    uint8_t *buffer = NULL;
    jstring result = NULL;
    if (required == -1) {
        result = literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"rust_probe_ffi_error\"}");
    } else if (required <= 0 || required > (1024 * 1024)) {
        result = literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"rust_probe_size_failed\"}");
    } else {
        buffer = (uint8_t *) calloc((size_t) required + 1u, 1u);
        if (buffer == NULL) {
            result = literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"jni_allocation_failed\"}");
        } else {
            int64_t written = rust_host_probe_snapshot(
                    app,
                    native_dir,
                    exec_dir,
                    (const uint8_t *) inventory,
                    (size_t) inventory_len,
                    (const uint8_t *) additional,
                    (size_t) additional_len,
                    buffer,
                    (size_t) required);
            if (written != required) {
                result = literal(env, "{\"schema\":1,\"native_loaded\":true,\"probe_ok\":false,\"error\":\"rust_probe_write_failed\"}");
            } else {
                buffer[required] = '\0';
                result = (*env)->NewStringUTF(env, (const char *) buffer);
            }
        }
    }

    free(buffer);
    (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
    (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
    (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
    (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
    (*env)->ReleaseByteArrayElements(env, additional_evidence_json, additional, JNI_ABORT);
    return result;
}
