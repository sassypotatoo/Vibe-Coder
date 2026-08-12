#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="24.19.0"
SHA256="f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f"
URL="https://nodejs.org/download/release/v${VERSION}/node-v${VERSION}.tar.xz"
NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
API="${VIBECODER_ANDROID_API:-29}"
JOBS="${VIBECODER_BUILD_JOBS:-4}"
CACHE="${VIBECODER_RUNTIME_CACHE:-$ROOT/.runtime-cache}"
ARCHIVE="$CACHE/node-v${VERSION}.tar.xz"
WORK="$CACHE/node-v${VERSION}-android-arm64"
DEST_NATIVE="$ROOT/android/app/src/main/jniLibs/arm64-v8a/libvibecoder_node_exec.so"
DEST_NPM="$ROOT/android/app/src/main/assets/node/npm"

fail() { printf 'provision_node_android: %s\n' "$1" >&2; exit 1; }
for tool in python3 make sha256sum tar; do command -v "$tool" >/dev/null 2>&1 || fail "tool_missing:$tool"; done
[[ -n "$NDK_ROOT" && -d "$NDK_ROOT" ]] || fail "android_ndk_root_missing"
mkdir -p "$CACHE"

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
# Node upstream documents Android as experimental/unsupported; this build is therefore evidence only,
# never proof until Part-27 probes pass on the physical target device.
./android-configure "$NDK_ROOT" "$API" arm64
make -j"$JOBS"
[[ -f out/Release/node ]] || fail "node_android_output_missing"
[[ -d deps/npm ]] || fail "node_npm_bundle_missing"

mkdir -p "$(dirname "$DEST_NATIVE")" "$(dirname "$DEST_NPM")"
rm -rf "$DEST_NPM"
install -m 0644 out/Release/node "$DEST_NATIVE"
cp -R deps/npm "$DEST_NPM"
printf 'staged %s\nstaged %s\n' "$DEST_NATIVE" "$DEST_NPM"
