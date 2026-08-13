#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="24.19.0"
SHA256="f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f"
URL="https://nodejs.org/download/release/v${VERSION}/node-v${VERSION}.tar.xz"
NDK_REVISION_REQUIRED="28.2.13676358"
NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
API="${VIBECODER_ANDROID_API:-29}"
JOBS="${VIBECODER_BUILD_JOBS:-4}"
CACHE="${VIBECODER_RUNTIME_CACHE:-$ROOT/.runtime-cache}"
ARCHIVE="$CACHE/node-v${VERSION}.tar.xz"
WORK="$CACHE/node-v${VERSION}-android-arm64"
DEST_NATIVE="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
OUTPUT_DIR="$ROOT/android/app/build/outputs"
CONFIGURE_LOG="$OUTPUT_DIR/vibecoder-part34-node-configure.log"
BUILD_LOG="$OUTPUT_DIR/vibecoder-part34-node-make.log"
CROSS_EVIDENCE="$OUTPUT_DIR/vibecoder-part34-node-cross-build-evidence.json"

fail() { printf 'provision_node_android: %s\n' "$1" >&2; exit 1; }
for tool in python3 make sha256sum tar xz awk; do
  command -v "$tool" >/dev/null 2>&1 || fail "tool_missing:$tool"
done
[[ -n "$NDK_ROOT" && -d "$NDK_ROOT" ]] || fail "android_ndk_root_missing"
[[ -f "$NDK_ROOT/source.properties" ]] || fail "android_ndk_source_properties_missing"
ACTUAL_NDK_REVISION="$(awk -F= '/^[[:space:]]*Pkg\.Revision[[:space:]]*=/{gsub(/[[:space:]]/, "", $2); print $2; exit}' "$NDK_ROOT/source.properties")"
[[ -n "$ACTUAL_NDK_REVISION" ]] || fail "android_ndk_revision_unreadable"
[[ "$ACTUAL_NDK_REVISION" == "$NDK_REVISION_REQUIRED" ]] || \
  fail "android_ndk_revision_mismatch:expected=${NDK_REVISION_REQUIRED}:actual=${ACTUAL_NDK_REVISION}"
[[ "$API" =~ ^[0-9]+$ ]] || fail "android_api_not_integer:$API"
[[ "$API" == "29" ]] || fail "android_api_mismatch:expected=29:actual=$API"
[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || fail "build_jobs_invalid:$JOBS"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) NDK_HOST_TAG="linux-x86_64" ;;
  Darwin-x86_64|Darwin-arm64) NDK_HOST_TAG="darwin-x86_64" ;;
  *) fail "unsupported_node_android_build_host:$(uname -s)-$(uname -m)" ;;
esac
NDK_TOOLCHAIN_BIN="$NDK_ROOT/toolchains/llvm/prebuilt/$NDK_HOST_TAG/bin"
NDK_CC="$NDK_TOOLCHAIN_BIN/aarch64-linux-android${API}-clang"
NDK_CXX="$NDK_TOOLCHAIN_BIN/aarch64-linux-android${API}-clang++"
NDK_AR="$NDK_TOOLCHAIN_BIN/llvm-ar"
[[ -x "$NDK_CC" ]] || fail "android_ndk_c_compiler_missing:$NDK_CC"
[[ -x "$NDK_CXX" ]] || fail "android_ndk_cxx_compiler_missing:$NDK_CXX"
[[ -x "$NDK_AR" ]] || fail "android_ndk_ar_missing:$NDK_AR"

# Node's upstream Android configure helper intentionally exports CC/CXX as the Android target
# compiler before invoking ./configure. GYP can consequently generate obj.host recipes that inherit
# that Android compiler too. Host generators must execute on the CI host, so bind the two toolchains
# explicitly at make time instead of patching the reviewed Node source tree.
HOST_CC="${VIBECODER_NODE_HOST_CC:-$(command -v gcc || true)}"
HOST_CXX="${VIBECODER_NODE_HOST_CXX:-$(command -v g++ || true)}"
HOST_AR="${VIBECODER_NODE_HOST_AR:-$(command -v ar || true)}"
[[ -n "$HOST_CC" && -x "$HOST_CC" ]] || fail "node_host_c_compiler_missing"
[[ -n "$HOST_CXX" && -x "$HOST_CXX" ]] || fail "node_host_cxx_compiler_missing"
[[ -n "$HOST_AR" && -x "$HOST_AR" ]] || fail "node_host_ar_missing"
case "$HOST_CC" in "$NDK_ROOT"/*) fail "node_host_c_compiler_must_not_be_ndk:$HOST_CC" ;; esac
case "$HOST_CXX" in "$NDK_ROOT"/*) fail "node_host_cxx_compiler_must_not_be_ndk:$HOST_CXX" ;; esac
python3 - <<'PY' || fail "python_version_unsupported_by_node_android_configure"
import sys
if sys.version_info[:2] not in {(3, 9), (3, 10), (3, 11), (3, 12), (3, 13), (3, 14)}:
    raise SystemExit(1)
PY

mkdir -p "$CACHE" "$OUTPUT_DIR"

if [[ ! -f "$ARCHIVE" ]]; then
  command -v curl >/dev/null 2>&1 || fail "curl_missing_and_archive_not_cached"
  curl --fail --location --proto '=https' --tlsv1.2 --output "$ARCHIVE.part" "$URL"
  mv "$ARCHIVE.part" "$ARCHIVE"
fi
printf '%s  %s\n' "$SHA256" "$ARCHIVE" | sha256sum --check --status || fail "node_source_sha256_mismatch"

rm -rf "$WORK"
mkdir -p "$WORK"
tar --extract --xz --file "$ARCHIVE" --directory "$WORK" --strip-components=1 --no-same-owner --no-same-permissions
cd "$WORK"
[[ -x ./android-configure ]] || fail "node_android_configure_missing"

# Node upstream still classifies Android as unsupported/experimental. This is therefore an evidence
# build, not a readiness claim. Keep stdout/stderr so the first real compiler/linker failure can be
# diagnosed instead of reduced to a generic CI red X.
set +e
./android-configure "$NDK_ROOT" "$API" arm64 2>&1 | tee "$CONFIGURE_LOG"
CONFIGURE_STATUS=${PIPESTATUS[0]}
set -e
if (( CONFIGURE_STATUS != 0 )); then
  fail "node_android_configure_failed:status=${CONFIGURE_STATUS}:log=${CONFIGURE_LOG}"
fi
# Upstream android_configure.py historically invokes ./configure through os.system without reliably
# making that inner command's status the wrapper status. Never accept wrapper exit 0 by itself.
python3 "$ROOT/scripts/verify_node_android_configure_output.py" "$WORK/config.gypi" "$WORK/Makefile" \
  || fail "node_android_configure_output_invalid:log=${CONFIGURE_LOG}"
python3 "$ROOT/scripts/verify_node_android_toolchain_split.py" preflight \
  "$WORK/out/Makefile" "$NDK_ROOT" "$HOST_CC" "$HOST_CXX" "$NDK_CC" "$NDK_CXX" \
  || fail "node_android_toolchain_split_preflight_failed"
# Upstream Android configure also leaks Android/ARM64-only compile flags into generated .host.mk
# files. Host generators execute on the Linux/macOS build machine, so strip only the two exact
# target-only inputs observed in the reviewed Node 24.19 Android graph. Never rewrite .target.mk.
python3 "$ROOT/scripts/sanitize_node_android_host_makefiles.py" "$WORK/out/Release" "$NDK_ROOT" \
  || fail "node_android_host_makefile_sanitize_failed"

printf 'vibecoder_node_host_cc=%s\n' "$HOST_CC" > "$BUILD_LOG"
printf 'vibecoder_node_host_cxx=%s\n' "$HOST_CXX" >> "$BUILD_LOG"
printf 'vibecoder_node_target_cc=%s\n' "$NDK_CC" >> "$BUILD_LOG"
printf 'vibecoder_node_target_cxx=%s\n' "$NDK_CXX" >> "$BUILD_LOG"
set +e
make -j"$JOBS" \
  "CC.host=$HOST_CC" "CXX.host=$HOST_CXX" "LINK.host=$HOST_CXX" "AR.host=$HOST_AR" \
  "CC.target=$NDK_CC" "CXX.target=$NDK_CXX" "LINK.target=$NDK_CXX" "AR.target=$NDK_AR" \
  2>&1 | tee -a "$BUILD_LOG"
BUILD_STATUS=${PIPESTATUS[0]}
set -e

# Even a failed build must never regress to compiling obj.host with the Android target compiler.
# On a successful build require evidence that both host and target compilation were observed.
if (( BUILD_STATUS == 0 )); then
  python3 "$ROOT/scripts/verify_node_android_toolchain_split.py" log \
    "$BUILD_LOG" "$HOST_CC" "$HOST_CXX" "$NDK_CC" "$NDK_CXX" --require-observed \
    || fail "node_android_toolchain_split_log_invalid"
else
  python3 "$ROOT/scripts/verify_node_android_toolchain_split.py" log \
    "$BUILD_LOG" "$HOST_CC" "$HOST_CXX" "$NDK_CC" "$NDK_CXX" \
    || fail "node_android_toolchain_split_log_invalid"
  fail "node_android_make_failed:status=${BUILD_STATUS}:log=${BUILD_LOG}"
fi

SOURCE="out/Release/node"
[[ -f "$SOURCE" && -s "$SOURCE" ]] || fail "node_android_output_missing"

# Fail before staging if the produced executable is not Android/Bionic AArch64 PIE with the same
# 16 KiB ELF compatibility contract used by the other packaged child runtimes. NDK r28+ should
# provide 16 KiB ELF alignment by default, but evidence wins over assumptions.
python3 "$ROOT/scripts/verify_android_elf.py" "$SOURCE" >/dev/null || fail "node_android_elf_verification_failed"

# Node is a generated native payload. Never mutate source jniLibs or source assets here. npm is a
# separate website-build capability and is intentionally not staged by this Node-only step.
mkdir -p "$(dirname "$DEST_NATIVE")"
install -m 0644 "$SOURCE" "$DEST_NATIVE"
python3 "$ROOT/scripts/verify_android_elf.py" "$DEST_NATIVE" >/dev/null || fail "staged_node_android_elf_verification_failed"

python3 "$ROOT/scripts/write_node_cross_build_evidence.py" \
  "$DEST_NATIVE" "$ARCHIVE" "$NDK_ROOT" "$API" "$CONFIGURE_LOG" "$BUILD_LOG" "$CROSS_EVIDENCE"
printf 'staged %s\n' "$DEST_NATIVE"
printf 'cross-build evidence %s\n' "$CROSS_EVIDENCE"
