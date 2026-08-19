#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-node-build-evidence.json"
CROSS_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-node-cross-build-evidence.json"
GENERATED_JNI="$ROOT/android/app/build/generated/jniLibs/arm64-v8a"
NODE="$GENERATED_JNI/libvibecoder_node_exec.so"
fail() { printf 'part34_node_build_and_verify: %s\n' "$1" >&2; exit 1; }

# Part 34.2.3 accepts only a Node candidate tied to machine-readable cross-build evidence. A random
# prebuilt with the same filename is not equivalent to the pinned Node source + NDK build contract.
[[ -f "$NODE" && -s "$NODE" ]] || fail "node_payload_not_staged_run_scripts_provision_node_android_sh_first"
[[ -f "$CROSS_EVIDENCE" && -s "$CROSS_EVIDENCE" ]] || fail "node_cross_build_evidence_missing_run_scripts_provision_node_android_sh_first"
python3 "$ROOT/scripts/verify_android_elf.py" "$NODE" >/dev/null || fail "node_payload_elf_invalid"
python3 "$ROOT/scripts/verify_node_cross_build_evidence.py" "$NODE" "$CROSS_EVIDENCE"

# Validate source authority before generated payload manipulation. Generated app/build files are
# deliberately excluded from CHECKSUMS.sha256.
python3 "$ROOT/scripts/validate_checkpoint.py"

# Reconstruct the fixed diagnostic signing identity exactly as the established Part-31 lane does.
KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
rm -f "$KEYSTORE_FILE"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore=$(sha256sum "$KEYSTORE_FILE" | cut -d' ' -f1)
[[ "$actual_keystore" == "$expected_keystore" ]] || fail "reconstructed_keystore_integrity_mismatch"
expected_cert="9D:73:BF:AE:B1:6E:70:67:23:BF:C4:17:CE:43:A9:ED:6B:10:28:68:35:E8:A3:05:0A:8D:DD:ED:67:50:64:45"
actual_cert=$(keytool -list -v -keystore "$KEYSTORE_FILE" -storepass vibecoder-debug | grep "SHA256:" | cut -d':' -f2- | tr -d ' ' | tr '[:lower:]' '[:upper:]')
[[ "$actual_cert" == "$expected_cert" ]] || fail "reconstructed_keystore_certificate_mismatch"

# Keep exactly the evidenced Node candidate across the clean generated-JNI reset. Host is rebuilt by
# build_android_host.sh below; Jcode is intentionally absent from this Node-only evidence APK.
NODE_TMP="$(mktemp)"
trap 'rm -f "$NODE_TMP"' EXIT
install -m 0644 "$NODE" "$NODE_TMP"
rm -rf "$ROOT/android/app/build/generated/jniLibs"
mkdir -p "$GENERATED_JNI"
install -m 0644 "$NODE_TMP" "$NODE"
python3 "$ROOT/scripts/verify_android_elf.py" "$NODE" >/dev/null || fail "restaged_node_payload_elf_invalid"
python3 "$ROOT/scripts/verify_node_cross_build_evidence.py" "$NODE" "$CROSS_EVIDENCE"

bash "$ROOT/scripts/build_android_host.sh"
bash "$ROOT/scripts/build_android_shell.sh"
bash "$ROOT/scripts/verify_android_diagnostic_apk.sh" "$APK" node
python3 "$ROOT/scripts/write_node_build_evidence.py" "$APK" "$CROSS_EVIDENCE" "$EVIDENCE"
printf 'Part 34 Node build evidence: %s\n' "$EVIDENCE"
