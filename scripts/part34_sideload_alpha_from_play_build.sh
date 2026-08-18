#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NODE="${1:-}"
NODE_EVIDENCE="${2:-}"
JCODE="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_jcode_exec.so"
JCODE_EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-jcode-build-evidence.json"
BASE_NODE="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
PLAY_AAB="$ROOT/android/app/build/outputs/bundle/debug/app-debug.aab"
FEATURE_NODE="$ROOT/android/node_runtime/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
EVIDENCE="$ROOT/android/app/build/outputs/vibecoder-part34-sideload-alpha-build-evidence.json"
fail() { printf 'part34_sideload_alpha_from_play_build: %s\n' "$1" >&2; exit 1; }
run_stage() {
  local label="$1" limit="$2"; shift 2
  local started rc elapsed
  started="$(date +%s)"
  printf '[part34-sideload] START %s timeout=%ss\n' "$label" "$limit"
  set +e
  timeout --signal=TERM --kill-after=30s "${limit}s" "$@"
  rc=$?
  set -e
  elapsed=$(( $(date +%s) - started ))
  if [[ "$rc" -eq 124 || "$rc" -eq 137 ]]; then fail "stage_timeout:${label}:${limit}s"; fi
  [[ "$rc" -eq 0 ]] || fail "stage_failed:${label}:rc=${rc}"
  printf '[part34-sideload] DONE %s elapsed=%ss\n' "$label" "$elapsed"
}

[[ -n "$NODE" && -f "$NODE" && -s "$NODE" ]] || fail "node_runtime_binary_missing"
[[ -n "$NODE_EVIDENCE" && -f "$NODE_EVIDENCE" && -s "$NODE_EVIDENCE" ]] || fail "node_cross_build_evidence_missing"
[[ -f "$JCODE" && -s "$JCODE" ]] || fail "jcode_payload_not_staged"
[[ -f "$JCODE_EVIDENCE" && -s "$JCODE_EVIDENCE" ]] || fail "jcode_build_evidence_missing"
[[ -f "$PLAY_AAB" && -s "$PLAY_AAB" ]] || fail "verified_play_aab_missing"
[[ -f "$FEATURE_NODE" && -s "$FEATURE_NODE" ]] || fail "verified_node_feature_payload_missing"

python3 "$ROOT/scripts/validate_checkpoint.py"
run_stage play-aab-recheck 180 python3 "$ROOT/scripts/verify_node_feature_bundle.py" "$PLAY_AAB" "$FEATURE_NODE"
run_stage node-cross-evidence 60 python3 "$ROOT/scripts/verify_node_cross_build_evidence.py" "$NODE" "$NODE_EVIDENCE"
run_stage node-elf 60 python3 "$ROOT/scripts/verify_android_elf.py" "$NODE"

# Sideload-only staging. The normal Alpha never places Node here, and the Play AAB producer
# explicitly removes this path before producing/verifying the production bundle.
install -m 0644 "$NODE" "$BASE_NODE"
trap 'rm -f "$BASE_NODE"' EXIT
run_stage packaged-node-elf 60 python3 "$ROOT/scripts/verify_android_elf.py" "$BASE_NODE"

# Never allow an older base-only APK to satisfy this lane accidentally.
rm -f "$APK"
run_stage sideload-apk-build 600 bash "$ROOT/scripts/build_android_shell.sh"
run_stage sideload-apk-verify 240 bash "$ROOT/scripts/verify_android_diagnostic_apk.sh" "$APK" sideload_alpha
run_stage sideload-evidence 120 python3 "$ROOT/scripts/write_sideload_alpha_build_evidence.py" \
  "$APK" "$JCODE_EVIDENCE" "$NODE" "$NODE_EVIDENCE" "$EVIDENCE"
printf 'Part 34 sideload Alpha package evidence: %s\n' "$EVIDENCE"
