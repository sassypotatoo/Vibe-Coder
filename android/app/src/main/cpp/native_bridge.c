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
typedef int64_t (*omniroute_start_fn)(
        const char *,
        const char *,
        const char *,
        const uint8_t *,
        size_t,
        const char *,
        uint8_t *,
        size_t);
typedef int64_t (*omniroute_simple_fn)(uint8_t *, size_t);
typedef int64_t (*omniroute_gateway_probe_fn)(const uint8_t *, size_t, uint8_t *, size_t);
typedef int64_t (*omniroute_inference_probe_fn)(
        const uint8_t *, size_t, const uint8_t *, size_t, const uint8_t *, size_t, uint8_t *, size_t);
typedef int64_t (*app_controller_init_fn)(
        const char *, const char *, const char *, const uint8_t *, size_t, uint8_t *, size_t);
typedef int64_t (*chat_simple_fn)(uint8_t *, size_t);
typedef int64_t (*chat_send_fn)(
        const char *, const char *, const uint8_t *, size_t, uint8_t *, size_t);
typedef int64_t (*chat_cancel_fn)(const char *, const char *, uint8_t *, size_t);

static void *rust_host_handle = NULL;
static abi_version_fn rust_host_abi_version = NULL;
static probe_snapshot_v2_fn rust_host_probe_snapshot = NULL;
static omniroute_start_fn rust_host_omniroute_start = NULL;
static omniroute_simple_fn rust_host_omniroute_status = NULL;
static omniroute_gateway_probe_fn rust_host_omniroute_gateway_probe = NULL;
static omniroute_inference_probe_fn rust_host_omniroute_inference_probe = NULL;
static omniroute_simple_fn rust_host_omniroute_stop = NULL;
static app_controller_init_fn rust_host_app_controller_init = NULL;
static chat_simple_fn rust_host_chat_create = NULL;
static chat_send_fn rust_host_chat_send = NULL;
static chat_cancel_fn rust_host_chat_cancel = NULL;

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
        rust_host_omniroute_start = (omniroute_start_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_omniroute_start_json");
        rust_host_omniroute_status = (omniroute_simple_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_omniroute_status_json");
        rust_host_omniroute_gateway_probe = (omniroute_gateway_probe_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_omniroute_gateway_probe_json");
        rust_host_omniroute_inference_probe = (omniroute_inference_probe_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_omniroute_inference_probe_json");
        rust_host_omniroute_stop = (omniroute_simple_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_omniroute_stop_json");
        rust_host_app_controller_init = (app_controller_init_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_app_controller_init_json");
        rust_host_chat_create = (chat_simple_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_chat_create_json");
        rust_host_chat_send = (chat_send_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_chat_send_json");
        rust_host_chat_cancel = (chat_cancel_fn) dlsym(
                rust_host_handle,
                "vibecoder_android_host_chat_cancel_json");
    }
    // Loading the JNI bridge itself must remain diagnosable even when the Rust host is absent or
    // incompatible; nativeProbeSnapshot reports that state instead of crashing class loading.
    return JNI_VERSION_1_6;
}

static jstring literal(JNIEnv *env, const char *value) {
    return (*env)->NewStringUTF(env, value);
}

#define CHAT_JSON_CAPACITY (2u * 1024u * 1024u)

// Rust/serde emits standard UTF-8 while JNI NewStringUTF expects modified UTF-8. Chat replies may
// contain supplementary Unicode (for example emoji), so pass the raw bytes through Java's UTF-8
// decoder instead of corrupting or rejecting valid model output.
static jstring utf8_bytes_to_jstring(JNIEnv *env, const uint8_t *bytes, size_t length) {
    if (bytes == NULL || length == 0u || length > CHAT_JSON_CAPACITY) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_utf8_output_invalid\"}");
    }
    jbyteArray data = (*env)->NewByteArray(env, (jsize) length);
    if (data == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_utf8_allocation_failed\"}");
    }
    (*env)->SetByteArrayRegion(env, data, 0, (jsize) length, (const jbyte *) bytes);
    if ((*env)->ExceptionCheck(env)) {
        (*env)->ExceptionClear(env);
        (*env)->DeleteLocalRef(env, data);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_utf8_copy_failed\"}");
    }
    jclass string_class = (*env)->FindClass(env, "java/lang/String");
    if (string_class == NULL) {
        (*env)->ExceptionClear(env);
        (*env)->DeleteLocalRef(env, data);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_string_class_missing\"}");
    }
    jmethodID constructor = (*env)->GetMethodID(env, string_class, "<init>", "([BLjava/lang/String;)V");
    if (constructor == NULL) {
        (*env)->ExceptionClear(env);
        (*env)->DeleteLocalRef(env, string_class);
        (*env)->DeleteLocalRef(env, data);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_string_constructor_missing\"}");
    }
    jstring charset = (*env)->NewStringUTF(env, "UTF-8");
    if (charset == NULL) {
        (*env)->DeleteLocalRef(env, string_class);
        (*env)->DeleteLocalRef(env, data);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_charset_allocation_failed\"}");
    }
    jstring result = (jstring) (*env)->NewObject(env, string_class, constructor, data, charset);
    if ((*env)->ExceptionCheck(env)) {
        (*env)->ExceptionClear(env);
        result = NULL;
    }
    (*env)->DeleteLocalRef(env, charset);
    (*env)->DeleteLocalRef(env, string_class);
    (*env)->DeleteLocalRef(env, data);
    return result == NULL
            ? literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_utf8_decode_failed\"}")
            : result;
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


static jstring omniroute_call_simple(JNIEnv *env, omniroute_simple_fn function, const char *missing_error) {
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || function == NULL) {
        return literal(env, missing_error);
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"rust_host_abi_mismatch\"}");
    }
    const size_t capacity = 1024u * 1024u;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    if (buffer == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"jni_allocation_failed\"}");
    }
    int64_t written = function(buffer, capacity);
    jstring result;
    if (written <= 0 || written > (int64_t) capacity) {
        result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"omniroute_ffi_write_failed\"}");
    } else {
        buffer[written] = '\0';
        result = (*env)->NewStringUTF(env, (const char *) buffer);
    }
    free(buffer);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteStart(
        JNIEnv *env,
        jclass clazz,
        jstring app_private_dir,
        jstring native_library_dir,
        jstring packaged_executable_dir,
        jbyteArray inventory_json,
        jstring expected_manifest_sha256) {
    (void) clazz;
    if (app_private_dir == NULL || native_library_dir == NULL || packaged_executable_dir == NULL ||
        inventory_json == NULL || expected_manifest_sha256 == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_omniroute_start == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"rust_omniroute_service_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"rust_host_abi_mismatch\"}");
    }

    const char *app = (*env)->GetStringUTFChars(env, app_private_dir, NULL);
    const char *native_dir = (*env)->GetStringUTFChars(env, native_library_dir, NULL);
    const char *exec_dir = (*env)->GetStringUTFChars(env, packaged_executable_dir, NULL);
    const char *manifest_sha = (*env)->GetStringUTFChars(env, expected_manifest_sha256, NULL);
    jbyte *inventory = (*env)->GetByteArrayElements(env, inventory_json, NULL);
    const jsize inventory_len = (*env)->GetArrayLength(env, inventory_json);
    if (app == NULL || native_dir == NULL || exec_dir == NULL || manifest_sha == NULL || inventory == NULL) {
        if (app != NULL) (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
        if (native_dir != NULL) (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
        if (exec_dir != NULL) (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
        if (manifest_sha != NULL) (*env)->ReleaseStringUTFChars(env, expected_manifest_sha256, manifest_sha);
        if (inventory != NULL) (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"jni_argument_access_failed\"}");
    }

    const size_t capacity = 1024u * 1024u;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_omniroute_start(
                app, native_dir, exec_dir, (const uint8_t *) inventory, (size_t) inventory_len,
                manifest_sha, buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"omniroute_start_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = (*env)->NewStringUTF(env, (const char *) buffer);
        }
    }

    free(buffer);
    (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
    (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
    (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
    (*env)->ReleaseStringUTFChars(env, expected_manifest_sha256, manifest_sha);
    (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteStatus(JNIEnv *env, jclass clazz) {
    (void) clazz;
    return omniroute_call_simple(
            env,
            rust_host_omniroute_status,
            "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"rust_omniroute_status_not_packaged\"}");
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteGatewayProbe(
        JNIEnv *env,
        jclass clazz,
        jbyteArray credential_utf8) {
    (void) clazz;
    if (credential_utf8 == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_omniroute_gateway_probe == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"rust_omniroute_gateway_probe_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }
    const jsize credential_len = (*env)->GetArrayLength(env, credential_utf8);
    if (credential_len < 0 || credential_len > 8192) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"gateway_credential_size_invalid\"}");
    }
    jbyte *credential = NULL;
    if (credential_len > 0) {
        credential = (*env)->GetByteArrayElements(env, credential_utf8, NULL);
        if (credential == NULL) {
            return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"jni_argument_access_failed\"}");
        }
    }
    const size_t capacity = 1024u * 1024u;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_omniroute_gateway_probe(
                (const uint8_t *) credential, (size_t) credential_len, buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"gateway_probe_failed\",\"error\":\"omniroute_gateway_probe_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = (*env)->NewStringUTF(env, (const char *) buffer);
        }
    }
    free(buffer);
    if (credential != NULL) {
        (*env)->ReleaseByteArrayElements(env, credential_utf8, credential, JNI_ABORT);
    }
    return result;
}


JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteInferenceProbe(
        JNIEnv *env,
        jclass clazz,
        jbyteArray credential_utf8,
        jbyteArray model_utf8,
        jbyteArray prompt_utf8) {
    (void) clazz;
    if (credential_utf8 == NULL || model_utf8 == NULL || prompt_utf8 == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_omniroute_inference_probe == NULL) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"rust_omniroute_inference_probe_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }

    const jsize credential_len = (*env)->GetArrayLength(env, credential_utf8);
    const jsize model_len = (*env)->GetArrayLength(env, model_utf8);
    const jsize prompt_len = (*env)->GetArrayLength(env, prompt_utf8);
    if (credential_len < 0 || credential_len > 8192 || model_len <= 0 || model_len > 512 ||
        prompt_len <= 0 || prompt_len > (64 * 1024)) {
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"inference_argument_size_invalid\"}");
    }

    jbyte *credential = NULL;
    jbyte *model = NULL;
    jbyte *prompt = NULL;
    if (credential_len > 0) credential = (*env)->GetByteArrayElements(env, credential_utf8, NULL);
    model = (*env)->GetByteArrayElements(env, model_utf8, NULL);
    prompt = (*env)->GetByteArrayElements(env, prompt_utf8, NULL);
    if ((credential_len > 0 && credential == NULL) || model == NULL || prompt == NULL) {
        if (credential != NULL) (*env)->ReleaseByteArrayElements(env, credential_utf8, credential, JNI_ABORT);
        if (model != NULL) (*env)->ReleaseByteArrayElements(env, model_utf8, model, JNI_ABORT);
        if (prompt != NULL) (*env)->ReleaseByteArrayElements(env, prompt_utf8, prompt, JNI_ABORT);
        return literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"jni_argument_access_failed\"}");
    }

    const size_t capacity = 1024u * 1024u;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_omniroute_inference_probe(
                (const uint8_t *) credential, (size_t) credential_len,
                (const uint8_t *) model, (size_t) model_len,
                (const uint8_t *) prompt, (size_t) prompt_len,
                buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"component_id\":\"omniroute\",\"status\":\"inference_probe_failed\",\"error\":\"omniroute_inference_probe_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = (*env)->NewStringUTF(env, (const char *) buffer);
        }
    }

    free(buffer);
    if (credential != NULL) (*env)->ReleaseByteArrayElements(env, credential_utf8, credential, JNI_ABORT);
    (*env)->ReleaseByteArrayElements(env, model_utf8, model, JNI_ABORT);
    (*env)->ReleaseByteArrayElements(env, prompt_utf8, prompt, JNI_ABORT);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeOmniRouteStop(JNIEnv *env, jclass clazz) {
    (void) clazz;
    return omniroute_call_simple(
            env,
            rust_host_omniroute_stop,
            "{\"schema\":1,\"component_id\":\"omniroute\",\"active\":false,\"ready\":false,\"error\":\"rust_omniroute_stop_not_packaged\"}");
}

static jstring chat_call_simple(JNIEnv *env, chat_simple_fn function, const char *missing_error) {
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || function == NULL) {
        return literal(env, missing_error);
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }
    const size_t capacity = CHAT_JSON_CAPACITY;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    if (buffer == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_allocation_failed\"}");
    }
    int64_t written = function(buffer, capacity);
    jstring result;
    if (written <= 0 || written > (int64_t) capacity) {
        result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"chat_ffi_write_failed\"}");
    } else {
        buffer[written] = '\0';
        result = utf8_bytes_to_jstring(env, buffer, (size_t) written);
    }
    free(buffer);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeAppControllerInit(
        JNIEnv *env,
        jclass clazz,
        jstring app_private_dir,
        jstring native_library_dir,
        jstring packaged_executable_dir,
        jbyteArray inventory_json) {
    (void) clazz;
    if (app_private_dir == NULL || native_library_dir == NULL ||
        packaged_executable_dir == NULL || inventory_json == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_app_controller_init == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_app_controller_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }

    const char *app = (*env)->GetStringUTFChars(env, app_private_dir, NULL);
    const char *native_dir = (*env)->GetStringUTFChars(env, native_library_dir, NULL);
    const char *exec_dir = (*env)->GetStringUTFChars(env, packaged_executable_dir, NULL);
    jbyte *inventory = (*env)->GetByteArrayElements(env, inventory_json, NULL);
    const jsize inventory_len = (*env)->GetArrayLength(env, inventory_json);
    if (app == NULL || native_dir == NULL || exec_dir == NULL || inventory == NULL) {
        if (app != NULL) (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
        if (native_dir != NULL) (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
        if (exec_dir != NULL) (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
        if (inventory != NULL) (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_argument_access_failed\"}");
    }

    const size_t capacity = CHAT_JSON_CAPACITY;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_app_controller_init(
                app, native_dir, exec_dir, (const uint8_t *) inventory, (size_t) inventory_len,
                buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"app_controller_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = utf8_bytes_to_jstring(env, buffer, (size_t) written);
        }
    }
    free(buffer);
    (*env)->ReleaseStringUTFChars(env, app_private_dir, app);
    (*env)->ReleaseStringUTFChars(env, native_library_dir, native_dir);
    (*env)->ReleaseStringUTFChars(env, packaged_executable_dir, exec_dir);
    (*env)->ReleaseByteArrayElements(env, inventory_json, inventory, JNI_ABORT);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeChatCreate(JNIEnv *env, jclass clazz) {
    (void) clazz;
    return chat_call_simple(
            env,
            rust_host_chat_create,
            "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_chat_create_not_packaged\"}");
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeChatSend(
        JNIEnv *env,
        jclass clazz,
        jstring project_id,
        jstring conversation_id,
        jbyteArray prompt_utf8) {
    (void) clazz;
    if (project_id == NULL || conversation_id == NULL || prompt_utf8 == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_chat_send == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_chat_send_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }
    const jsize prompt_len = (*env)->GetArrayLength(env, prompt_utf8);
    if (prompt_len <= 0 || prompt_len > (128 * 1024)) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"chat_prompt_size_invalid\"}");
    }
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *conversation = (*env)->GetStringUTFChars(env, conversation_id, NULL);
    jbyte *prompt = (*env)->GetByteArrayElements(env, prompt_utf8, NULL);
    if (project == NULL || conversation == NULL || prompt == NULL) {
        if (project != NULL) (*env)->ReleaseStringUTFChars(env, project_id, project);
        if (conversation != NULL) (*env)->ReleaseStringUTFChars(env, conversation_id, conversation);
        if (prompt != NULL) (*env)->ReleaseByteArrayElements(env, prompt_utf8, prompt, JNI_ABORT);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_argument_access_failed\"}");
    }
    const size_t capacity = CHAT_JSON_CAPACITY;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_chat_send(
                project, conversation, (const uint8_t *) prompt, (size_t) prompt_len,
                buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"chat_send_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = utf8_bytes_to_jstring(env, buffer, (size_t) written);
        }
    }
    free(buffer);
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, conversation_id, conversation);
    (*env)->ReleaseByteArrayElements(env, prompt_utf8, prompt, JNI_ABORT);
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_vibecoder_shell_NativeBridge_nativeChatCancel(
        JNIEnv *env,
        jclass clazz,
        jstring project_id,
        jstring conversation_id) {
    (void) clazz;
    if (project_id == NULL || conversation_id == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_invalid_arguments\"}");
    }
    if (rust_host_handle == NULL || rust_host_abi_version == NULL || rust_host_chat_cancel == NULL) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_chat_cancel_not_packaged\"}");
    }
    if (rust_host_abi_version() != 1u) {
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"rust_host_abi_mismatch\"}");
    }
    const char *project = (*env)->GetStringUTFChars(env, project_id, NULL);
    const char *conversation = (*env)->GetStringUTFChars(env, conversation_id, NULL);
    if (project == NULL || conversation == NULL) {
        if (project != NULL) (*env)->ReleaseStringUTFChars(env, project_id, project);
        if (conversation != NULL) (*env)->ReleaseStringUTFChars(env, conversation_id, conversation);
        return literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_argument_access_failed\"}");
    }
    const size_t capacity = CHAT_JSON_CAPACITY;
    uint8_t *buffer = (uint8_t *) calloc(capacity + 1u, 1u);
    jstring result;
    if (buffer == NULL) {
        result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"jni_allocation_failed\"}");
    } else {
        int64_t written = rust_host_chat_cancel(project, conversation, buffer, capacity);
        if (written <= 0 || written > (int64_t) capacity) {
            result = literal(env, "{\"schema\":1,\"status\":\"failed\",\"error\":\"chat_cancel_ffi_write_failed\"}");
        } else {
            buffer[written] = '\0';
            result = utf8_bytes_to_jstring(env, buffer, (size_t) written);
        }
    }
    free(buffer);
    (*env)->ReleaseStringUTFChars(env, project_id, project);
    (*env)->ReleaseStringUTFChars(env, conversation_id, conversation);
    return result;
}
