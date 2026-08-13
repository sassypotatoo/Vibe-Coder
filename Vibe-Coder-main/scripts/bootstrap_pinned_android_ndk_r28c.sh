#!/usr/bin/env bash
set -euo pipefail

# Offline/bootstrap helper for environments where sdkmanager is unavailable but the official Google
# r28c Linux archive has been supplied out-of-band. This script does not download anything.
EXPECTED_REVISION="28.2.13676358"
EXPECTED_ARCHIVE_BYTES="722261334"
EXPECTED_ARCHIVE_SHA1="a7b54a5de87fecd125a17d54f73c446199e72a64"

fail() { printf 'bootstrap_pinned_android_ndk_r28c: %s\n' "$1" >&2; exit 1; }
[[ $# -eq 2 ]] || fail "usage: bootstrap_pinned_android_ndk_r28c.sh ARCHIVE_ZIP DEST_DIR"
ARCHIVE="$(realpath "$1")"
DEST="$(realpath -m "$2")"
for tool in sha1sum unzip awk stat realpath find; do
  command -v "$tool" >/dev/null 2>&1 || fail "tool_missing:$tool"
done
[[ -f "$ARCHIVE" ]] || fail "archive_missing:$ARCHIVE"
ACTUAL_BYTES="$(stat -c '%s' "$ARCHIVE")"
[[ "$ACTUAL_BYTES" == "$EXPECTED_ARCHIVE_BYTES" ]] || fail "archive_size_mismatch:expected=${EXPECTED_ARCHIVE_BYTES}:actual=${ACTUAL_BYTES}"
printf '%s  %s\n' "$EXPECTED_ARCHIVE_SHA1" "$ARCHIVE" | sha1sum --check --status || fail "archive_sha1_mismatch"
[[ ! -e "$DEST" ]] || fail "destination_already_exists:$DEST"
mkdir -p "$(dirname "$DEST")"
TMP="${DEST}.tmp.$$"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP"
unzip -q "$ARCHIVE" -d "$TMP"
mapfile -t ROOTS < <(find "$TMP" -mindepth 1 -maxdepth 1 -type d -print)
[[ ${#ROOTS[@]} -eq 1 ]] || fail "archive_root_count_unexpected:${#ROOTS[@]}"
NDK_ROOT="${ROOTS[0]}"
[[ -f "$NDK_ROOT/source.properties" ]] || fail "source_properties_missing"
ACTUAL_REVISION="$(awk -F= '/^[[:space:]]*Pkg\.Revision[[:space:]]*=/{gsub(/[[:space:]]/, "", $2); print $2; exit}' "$NDK_ROOT/source.properties")"
[[ "$ACTUAL_REVISION" == "$EXPECTED_REVISION" ]] || fail "ndk_revision_mismatch:expected=${EXPECTED_REVISION}:actual=${ACTUAL_REVISION}"
[[ -x "$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android29-clang" ]] || fail "api29_arm64_clang_missing"
mv "$NDK_ROOT" "$DEST"
printf 'ANDROID_NDK_ROOT=%s\n' "$DEST"
trap - EXIT
rm -rf "$TMP"
