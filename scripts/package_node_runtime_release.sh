#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NODE="${1:-$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so}"
NODE_EVIDENCE="${2:-$ROOT/android/app/build/outputs/vibecoder-part34-node-cross-build-evidence.json}"
TAG="vibecoder-node-runtime-24.19.0-v31"
OUTPUT_DIR="$ROOT/android/node_runtime/build/outputs/runtime-package"
OUTPUT="$OUTPUT_DIR/vibecoder-node-runtime-arm64-v31.apk"
fail() { printf 'package_node_runtime_release: %s\n' "$1" >&2; exit 1; }

[[ -f "$NODE" && -s "$NODE" ]] || fail "node_binary_missing"
[[ -f "$NODE_EVIDENCE" && -s "$NODE_EVIDENCE" ]] || fail "node_evidence_missing"
python3 "$ROOT/scripts/stage_node_runtime_split.py" "$NODE" "$NODE_EVIDENCE"

KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore="$(sha256sum "$KEYSTORE_FILE" | awk '{print $1}')"
[[ "$actual_keystore" == "$expected_keystore" ]] || fail "reconstructed_keystore_integrity_mismatch"

cd "$ROOT/android"
if [[ -x ./gradlew && -f gradle/wrapper/gradle-wrapper.jar ]]; then
  GRADLE=(./gradlew)
elif command -v gradle >/dev/null 2>&1; then
  GRADLE=(gradle)
else
  fail "gradle_missing"
fi
GRADLE_INFO="$("${GRADLE[@]}" --version)"
printf '%s\n' "$GRADLE_INFO" | grep -Eq '^Gradle 9\.5\.0$' || fail "gradle_version_must_be_9_5_0"
"${GRADLE[@]}" --no-daemon --stacktrace :node_runtime:assembleDebug

mapfile -t candidates < <(find "$ROOT/android/node_runtime/build/outputs" -type f -name '*.apk' -print | sort)
((${#candidates[@]} > 0)) || fail "node_runtime_apk_not_produced"
RUNTIME_APK=""
for candidate in "${candidates[@]}"; do
  if python3 "$ROOT/scripts/verify_node_runtime_split_apk.py" "$candidate" "$NODE" >/dev/null 2>&1; then
    RUNTIME_APK="$candidate"
    break
  fi
done
[[ -n "$RUNTIME_APK" ]] || fail "node_runtime_apk_with_expected_payload_not_found"

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
BUILD_TOOLS_VERSION="${ANDROID_BUILD_TOOLS:-36.0.0}"
[[ -n "$SDK_ROOT" ]] || fail "android_sdk_root_missing"
APKSIGNER="$SDK_ROOT/build-tools/$BUILD_TOOLS_VERSION/apksigner"
AAPT2="$SDK_ROOT/build-tools/$BUILD_TOOLS_VERSION/aapt2"
[[ -x "$APKSIGNER" ]] || fail "apksigner_missing"
[[ -x "$AAPT2" ]] || fail "aapt2_missing"
"$APKSIGNER" verify --verbose --Werr "$RUNTIME_APK" >/dev/null
CERT_SHA256="$("$APKSIGNER" verify --print-certs "$RUNTIME_APK" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' | head -n1 | tr -d ':[:space:]' | tr 'A-F' 'a-f')"
[[ "$CERT_SHA256" == "9d73bfaeb16e706723bfc417ce43a9ed6b10286835e8a3050a8ddded67506445" ]] \
  || fail "runtime_apk_signing_certificate_mismatch:$CERT_SHA256"
BADGING="$("$AAPT2" dump badging "$RUNTIME_APK")"
printf '%s\n' "$BADGING" | grep -q "package: name='com.vibecoder.shell'" || fail "runtime_apk_package_name_mismatch"
printf '%s\n' "$BADGING" | grep -q "versionCode='31'" || fail "runtime_apk_version_code_mismatch"
printf '%s\n' "$BADGING" | grep -Eq "split='node_runtime'|featureSplit='node_runtime'" || fail "runtime_apk_split_name_mismatch"

mkdir -p "$OUTPUT_DIR"
cp -f "$RUNTIME_APK" "$OUTPUT"
python3 "$ROOT/scripts/verify_node_runtime_split_apk.py" "$OUTPUT" "$NODE"
sha256sum "$OUTPUT" > "$OUTPUT.sha256"
printf 'Node runtime release package ready\ntag=%s\napk=%s\n' "$TAG" "$OUTPUT"
