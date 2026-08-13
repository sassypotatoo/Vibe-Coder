#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-minimal}"
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part31-build-evidence.json"
GENERATED_JNI="$ROOT/android/app/build/generated/jniLibs/arm64-v8a"
fail() { printf 'part31_build_and_verify: %s\n' "$1" >&2; exit 1; }
[[ "$MODE" == "minimal" || "$MODE" == "jcode" ]] || fail "mode_must_be_minimal_or_jcode"

# Never build from a checkpoint whose own integrity/static contracts already fail.
python3 "$ROOT/scripts/validate_checkpoint.py"

# Reconstruct the diagnostic signing identity from the text-safe authority.
KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
rm -f "$KEYSTORE_FILE"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore=$(sha256sum "$KEYSTORE_FILE" | cut -d' ' -f1)
if [[ "$actual_keystore" != "$expected_keystore" ]]; then
  fail "reconstructed_keystore_integrity_mismatch"
fi
# Verify the certificate fingerprint
expected_cert="9D:73:BF:AE:B1:6E:70:67:23:BF:C4:17:CE:43:A9:ED:6B:10:28:68:35:E8:A3:05:0A:8D:DD:ED:67:50:64:45"
actual_cert=$(keytool -list -v -keystore "$KEYSTORE_FILE" -storepass vibecoder-debug | grep "SHA256:" | cut -d':' -f2- | tr -d ' ' | tr '[:lower:]' '[:upper:]')
if [[ "$actual_cert" != "$expected_cert" ]]; then
  fail "reconstructed_keystore_certificate_mismatch"
fi

if [[ "$MODE" == "jcode" ]]; then
  JCODE="$GENERATED_JNI/libvibecoder_jcode_exec.so"
  [[ -f "$JCODE" && -s "$JCODE" ]] || fail "jcode_payload_not_staged"
  python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null
  JCODE_TMP="$(mktemp)"
  trap 'rm -f "$JCODE_TMP"' EXIT
  install -m 0644 "$JCODE" "$JCODE_TMP"
  rm -rf "$ROOT/android/app/build/generated/jniLibs"
  mkdir -p "$GENERATED_JNI"
  install -m 0644 "$JCODE_TMP" "$JCODE"
  python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null
else
  # A minimal APK must stay minimal even in a reused local workspace.
  rm -rf "$ROOT/android/app/build/generated/jniLibs"
fi

bash "$ROOT/scripts/build_android_host.sh"
bash "$ROOT/scripts/build_android_shell.sh"
bash "$ROOT/scripts/verify_android_diagnostic_apk.sh" "$APK" "$MODE"
python3 "$ROOT/scripts/write_android_build_evidence.py" "$APK" "$MODE" "$EVIDENCE"
printf 'Part 31 build evidence: %s\n' "$EVIDENCE"
