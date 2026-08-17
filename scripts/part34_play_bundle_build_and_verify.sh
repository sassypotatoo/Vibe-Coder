#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE="${1:-}"
NODE="${2:-}"
NODE_EVIDENCE="${3:-}"
JCODE="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_jcode_exec.so"
JCODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-jcode-build-evidence.json"
BASE_NODE="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
FEATURE_NODE="$ROOT/android/node_runtime/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
BUNDLE_DIR="$ROOT/android/app/build/generated/omnirouteBundle"
AAB="$ROOT/android/app/build/outputs/bundle/debug/app-debug.aab"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-play-bundle-evidence.json"
fail() { printf 'part34_play_bundle_build_and_verify: %s\n' "$1" >&2; exit 1; }

[[ -n "$ARCHIVE" && -f "$ARCHIVE" && -s "$ARCHIVE" ]] || fail "reviewed_omniroute_archive_missing"
[[ -n "$NODE" && -f "$NODE" && -s "$NODE" ]] || fail "node_runtime_binary_missing"
[[ -n "$NODE_EVIDENCE" && -f "$NODE_EVIDENCE" && -s "$NODE_EVIDENCE" ]] || fail "node_cross_build_evidence_missing"
[[ -f "$JCODE" && -s "$JCODE" ]] || fail "jcode_payload_not_staged"
[[ -f "$JCODE_EVIDENCE" && -s "$JCODE_EVIDENCE" ]] || fail "jcode_build_evidence_missing"

python3 "$ROOT/scripts/validate_checkpoint.py"
python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null || fail "jcode_android_elf_invalid"
python3 "$ROOT/scripts/stage_node_play_feature.py" "$NODE" "$NODE_EVIDENCE"
[[ -f "$FEATURE_NODE" && -s "$FEATURE_NODE" ]] || fail "node_feature_payload_not_staged"
# A reused diagnostic workspace may still contain the old base-packaged Node proof. Production
# Play bundles must never inherit that stale generated file. The independently staged feature copy
# above is now the source used for bundle hash/evidence verification.
rm -f "$BASE_NODE"

node_version="$(node --version 2>/dev/null || true)"
[[ "$node_version" == "v24.19.0" ]] || fail "host_node_version_must_be_24_19_0:actual=${node_version:-missing}"
command -v npm >/dev/null 2>&1 || fail "npm_not_found"

rm -rf "$BUNDLE_DIR" "$ROOT/android/app/build/generated/omnirouteAssets"
python3 "$ROOT/scripts/build_omniroute_android_bundle.py" \
  "$ARCHIVE" "$BUNDLE_DIR" \
  --evidence "$ROOT/android/app/build/outputs/vibecoder-part34-omniroute-bundle-evidence.json"
python3 "$ROOT/scripts/stage_omniroute_android_asset.py" "$BUNDLE_DIR"

KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore="$(sha256sum "$KEYSTORE_FILE" | awk '{print $1}')"
[[ "$actual_keystore" == "$expected_keystore" ]] || fail "reconstructed_keystore_integrity_mismatch"

export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}/ndk/28.2.13676358}"
bash "$ROOT/scripts/build_android_host.sh"

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[[ -n "$SDK_ROOT" && -d "$SDK_ROOT" ]] || fail "android_sdk_root_missing"
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
"${GRADLE[@]}" --no-daemon --stacktrace :app:bundleDebug
[[ -f "$AAB" && -s "$AAB" ]] || fail "debug_aab_missing_after_successful_gradle_task"
python3 "$ROOT/scripts/verify_node_feature_bundle.py" "$AAB" "$FEATURE_NODE" >/dev/null
python3 "$ROOT/scripts/write_play_bundle_evidence.py" "$AAB" "$FEATURE_NODE" "$NODE_EVIDENCE" "$EVIDENCE"
printf 'Part 34 Play bundle package evidence: %s\n' "$EVIDENCE"
