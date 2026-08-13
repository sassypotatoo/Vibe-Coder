#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT/android/app/build/outputs"
EXECUTION_LOG="$OUTPUT_DIR/vibecoder-part34-node-execution.log"
ATTEMPT_JSON="$OUTPUT_DIR/vibecoder-part34-node-execution-attempt.json"
mkdir -p "$OUTPUT_DIR"
rm -f "$EXECUTION_LOG" "$ATTEMPT_JSON"

# This wrapper is the authority for classifying a real execution attempt. It never converts a
# missing toolchain, network failure, configure failure, compiler/linker failure, packaging failure,
# or device absence into a success claim.
set +e
bash "$ROOT/scripts/provision_node_android.sh" 2>&1 | tee "$EXECUTION_LOG"
STATUS=${PIPESTATUS[0]}
set -e

if (( STATUS == 0 )); then
  CLASSIFICATION="cross_build_candidate_produced"
  DETAIL="provision_node_android_completed"
  EVIDENCE_STATUS="succeeded"
else
  EVIDENCE_STATUS="failed"
  if grep -q 'android_ndk_root_missing' "$EXECUTION_LOG"; then
    CLASSIFICATION="toolchain_unavailable"
    DETAIL="android_ndk_root_missing"
  elif grep -q 'android_ndk_revision_mismatch:' "$EXECUTION_LOG"; then
    CLASSIFICATION="toolchain_identity_mismatch"
    DETAIL="android_ndk_revision_mismatch"
  elif grep -q 'android_ndk_.*_compiler_missing:' "$EXECUTION_LOG"; then
    CLASSIFICATION="toolchain_incomplete"
    DETAIL="android_ndk_arm64_compiler_missing"
  elif grep -Eq 'Could not resolve host|curl: \([0-9]+\)|Failed to connect|node_source_sha256_mismatch' "$EXECUTION_LOG"; then
    CLASSIFICATION="source_acquisition_failed"
    DETAIL="node_source_download_or_integrity_failure"
  elif grep -q 'node_android_configure_failed:' "$EXECUTION_LOG"; then
    CLASSIFICATION="configure_failed"
    DETAIL="node_android_configure_failed"
  elif grep -q 'node_android_configure_output_invalid:' "$EXECUTION_LOG"; then
    CLASSIFICATION="configure_evidence_invalid"
    DETAIL="node_android_configure_output_invalid"
  elif grep -Eq 'node_android_generated_makefile_(failed|missing)' "$EXECUTION_LOG"; then
    CLASSIFICATION="build_graph_generation_failed"
    DETAIL="node_android_generated_makefile_failed"
  elif grep -q 'node_android_host_makefile_sanitize_failed' "$EXECUTION_LOG"; then
    CLASSIFICATION="host_target_flag_sanitize_failed"
    DETAIL="node_android_host_makefile_sanitize_failed"
  elif grep -Eq 'node_android_toolchain_split_(preflight_failed|log_invalid)' "$EXECUTION_LOG"; then
    CLASSIFICATION="host_target_toolchain_split_invalid"
    DETAIL="node_android_host_target_toolchain_split_invalid"
  elif grep -q 'node_android_make_failed:' "$EXECUTION_LOG"; then
    CLASSIFICATION="compiler_or_linker_failed"
    DETAIL="node_android_make_failed"
  elif grep -q 'node_android_elf_verification_failed' "$EXECUTION_LOG"; then
    CLASSIFICATION="binary_validation_failed"
    DETAIL="node_android_elf_verification_failed"
  else
    CLASSIFICATION="unclassified_cross_build_failure"
    DETAIL="provision_node_android_nonzero_exit"
  fi
fi

python3 "$ROOT/scripts/write_node_cross_build_attempt.py" \
  "$EVIDENCE_STATUS" "$STATUS" "$CLASSIFICATION" "$DETAIL" "$EXECUTION_LOG" "$ATTEMPT_JSON"
printf 'node cross-build attempt evidence %s\n' "$ATTEMPT_JSON"
exit "$STATUS"
