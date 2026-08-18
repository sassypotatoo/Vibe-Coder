#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE="${1:-$ROOT/.runtime-cache/OmniRoute-release-v3.8.50.zip}"
GENERATED_JNI="$ROOT/android/app/build/generated/jniLibs/arm64-v8a"
JCODE="$GENERATED_JNI/libvibecoder_jcode_exec.so"
JCODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-jcode-build-evidence.json"
NODE="$GENERATED_JNI/libvibecoder_node_exec.so"
NODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-node-cross-build-evidence.json"
BUNDLE="$ROOT/android/app/build/generated/omnirouteBundle"
OMNIROUTE_VERIFY_STAMP="$ROOT/android/app/build/outputs/vibecoder-part34-omniroute-verification-stamp.json"
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-alpha-build-evidence.json"
fail() { printf 'part34_alpha_build_and_verify: %s\n' "$1" >&2; exit 1; }
run_stage() {
  local label="$1" limit="$2"; shift 2
  local started rc elapsed
  started="$(date +%s)"
  printf '[part34-stage] START %s timeout=%ss\n' "$label" "$limit"
  set +e
  timeout --signal=TERM --kill-after=30s "${limit}s" "$@"
  rc=$?
  set -e
  elapsed=$(( $(date +%s) - started ))
  if [[ "$rc" -eq 124 || "$rc" -eq 137 ]]; then
    fail "stage_timeout:${label}:${limit}s"
  fi
  [[ "$rc" -eq 0 ]] || fail "stage_failed:${label}:rc=${rc}"
  printf '[part34-stage] DONE %s elapsed=%ss\n' "$label" "$elapsed"
}


python3 "$ROOT/scripts/validate_checkpoint.py"
[[ -f "$ARCHIVE" && -s "$ARCHIVE" ]] || fail "reviewed_omniroute_archive_missing:$ARCHIVE"
[[ -f "$JCODE" && -s "$JCODE" ]] || fail "jcode_payload_not_staged"
[[ -f "$JCODE_EVIDENCE" && -s "$JCODE_EVIDENCE" ]] || fail "jcode_build_evidence_missing"
[[ -f "$NODE" && -s "$NODE" ]] || fail "node_payload_not_staged_for_development_alpha"
[[ -f "$NODE_EVIDENCE" && -s "$NODE_EVIDENCE" ]] || fail "node_cross_build_evidence_missing_for_development_alpha"
python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null || fail "jcode_android_elf_invalid"
python3 "$ROOT/scripts/verify_android_elf.py" "$NODE" >/dev/null || fail "node_android_elf_invalid"
python3 "$ROOT/scripts/verify_node_cross_build_evidence.py" "$NODE" "$NODE_EVIDENCE" >/dev/null || fail "node_cross_build_evidence_invalid"

node_version="$(node --version 2>/dev/null || true)"
[[ "$node_version" == "v24.19.0" ]] || fail "host_node_version_must_be_24_19_0:actual=${node_version:-missing}"
command -v npm >/dev/null 2>&1 || fail "npm_not_found"

rm -rf "$BUNDLE" "$ROOT/android/app/build/generated/omnirouteAssets"
run_stage omniroute-build-and-verify 1200 python3 "$ROOT/scripts/build_omniroute_android_bundle.py" \
  "$ARCHIVE" "$BUNDLE" \
  --evidence "$ROOT/android/app/build/outputs/vibecoder-part34-omniroute-bundle-evidence.json"
run_stage omniroute-asset-stage 120 python3 "$ROOT/scripts/stage_omniroute_android_asset.py" "$BUNDLE" \
  --verification-stamp "$OMNIROUTE_VERIFY_STAMP" \
  --consume-verified-bundle

run_stage omniroute-aapt-policy 60 python3 "$ROOT/scripts/verify_omniroute_aapt_asset_policy.py" \
  "$ROOT/android/app/build/generated/omnirouteAssets/omniroute/bundle"

# Reconstruct the fixed diagnostic identity only at build time; no binary keystore is tracked.
KEYSTORE_DIR="$ROOT/android/signing"
KEYSTORE_FILE="$KEYSTORE_DIR/vibecoder-diagnostic-debug.jks"
mkdir -p "$KEYSTORE_DIR"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$KEYSTORE_FILE"
expected_keystore="8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6"
actual_keystore="$(sha256sum "$KEYSTORE_FILE" | awk '{print $1}')"
[[ "$actual_keystore" == "$expected_keystore" ]] || fail "reconstructed_keystore_integrity_mismatch"

# Development phase: package the already-proven Node runtime directly in the base APK.
# No Google Play ownership, split install, or Play delivery is required for this Alpha lane.
run_stage android-host-build 600 bash "$ROOT/scripts/build_android_host.sh"
run_stage android-apk-build 600 bash "$ROOT/scripts/build_android_shell.sh"
run_stage android-apk-verify 240 bash "$ROOT/scripts/verify_android_diagnostic_apk.sh" "$APK" sideload_alpha
run_stage alpha-evidence 120 python3 "$ROOT/scripts/write_alpha_build_evidence.py" \
  "$APK" "$JCODE_EVIDENCE" "$NODE" "$NODE_EVIDENCE" "$EVIDENCE"
printf 'Part 34 full Alpha package evidence: %s\n' "$EVIDENCE"
