#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE="${1:-$ROOT/.runtime-cache/OmniRoute-release-v3.8.50.zip}"
GENERATED_JNI="$ROOT/android/app/build/generated/jniLibs/arm64-v8a"
JCODE="$GENERATED_JNI/libvibecoder_jcode_exec.so"
NODE="$GENERATED_JNI/libvibecoder_node_exec.so"
JCODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-jcode-build-evidence.json"
NODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-node-cross-build-evidence.json"
BUNDLE="$ROOT/android/app/build/generated/omnirouteBundle"
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-alpha-build-evidence.json"
fail() { printf 'part34_alpha_build_and_verify: %s\n' "$1" >&2; exit 1; }

python3 "$ROOT/scripts/validate_checkpoint.py"
[[ -f "$ARCHIVE" && -s "$ARCHIVE" ]] || fail "reviewed_omniroute_archive_missing:$ARCHIVE"
[[ -f "$JCODE" && -s "$JCODE" ]] || fail "jcode_payload_not_staged"
[[ -f "$NODE" && -s "$NODE" ]] || fail "node_payload_not_staged"
[[ -f "$JCODE_EVIDENCE" && -s "$JCODE_EVIDENCE" ]] || fail "jcode_build_evidence_missing"
[[ -f "$NODE_EVIDENCE" && -s "$NODE_EVIDENCE" ]] || fail "node_cross_build_evidence_missing"
python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null || fail "jcode_android_elf_invalid"
python3 "$ROOT/scripts/verify_android_elf.py" "$NODE" >/dev/null || fail "node_android_elf_invalid"
python3 "$ROOT/scripts/verify_node_cross_build_evidence.py" "$NODE" "$NODE_EVIDENCE"

node_version="$(node --version 2>/dev/null || true)"
[[ "$node_version" == "v24.19.0" ]] || fail "host_node_version_must_be_24_19_0:actual=${node_version:-missing}"
command -v npm >/dev/null 2>&1 || fail "npm_not_found"

rm -rf "$BUNDLE" "$ROOT/android/app/build/generated/omnirouteAssets"
python3 "$ROOT/scripts/build_omniroute_android_bundle.py" \
  "$ARCHIVE" "$BUNDLE" \
  --evidence "$ROOT/android/app/build/outputs/vibecoder-part34-omniroute-bundle-evidence.json"
python3 "$ROOT/scripts/stage_omniroute_android_asset.py" "$BUNDLE"

# Reconstruct the fixed diagnostic identity only at build time; no binary keystore is tracked.
KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore="$(sha256sum "$KEYSTORE_FILE" | awk '{print $1}')"
[[ "$actual_keystore" == "$expected_keystore" ]] || fail "reconstructed_keystore_integrity_mismatch"

# Build host after all child payloads are staged. build_android_host.sh only adds/replaces the host
# library and does not clear Jcode/Node. Gradle then packages the same generated tree plus OmniRoute.
bash "$ROOT/scripts/build_android_host.sh"
bash "$ROOT/scripts/build_android_shell.sh"
bash "$ROOT/scripts/verify_android_diagnostic_apk.sh" "$APK" alpha
python3 "$ROOT/scripts/write_alpha_build_evidence.py" "$APK" "$JCODE_EVIDENCE" "$NODE_EVIDENCE" "$EVIDENCE"
printf 'Part 34 full Alpha package evidence: %s\n' "$EVIDENCE"
