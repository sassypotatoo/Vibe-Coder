#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
API="${VIBECODER_ANDROID_API:-29}"
TARGET="aarch64-linux-android"
TRIPLE="aarch64-linux-android"
DEST="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_android_host.so"

fail() { printf 'build_android_host: %s\n' "$1" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || fail "cargo_not_found"
command -v rustup >/dev/null 2>&1 || fail "rustup_not_found"
[[ -n "$NDK_ROOT" && -d "$NDK_ROOT" ]] || fail "android_ndk_root_missing"

HOST_TAG=""
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) HOST_TAG="linux-x86_64" ;;
  Darwin-x86_64) HOST_TAG="darwin-x86_64" ;;
  Darwin-arm64) HOST_TAG="darwin-x86_64" ;;
  *) fail "unsupported_ndk_host" ;;
esac
TOOLCHAIN="$NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG/bin"
CC="$TOOLCHAIN/${TRIPLE}${API}-clang"
AR="$TOOLCHAIN/llvm-ar"
[[ -x "$CC" ]] || fail "android_clang_missing:$CC"
[[ -x "$AR" ]] || fail "android_llvm_ar_missing:$AR"

rustup target add "$TARGET" >/dev/null
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC"
export CC_aarch64_linux_android="$CC"
export AR_aarch64_linux_android="$AR"
# Keep the output 16 KiB compatible even if a future linker default changes.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384"

cd "$ROOT"
cargo build --locked --release --target "$TARGET" -p vibecoder-android-host
SOURCE="$ROOT/target/$TARGET/release/libvibecoder_android_host.so"
[[ -f "$SOURCE" ]] || fail "android_host_output_missing"
mkdir -p "$(dirname "$DEST")"
install -m 0644 "$SOURCE" "$DEST"
printf 'staged %s\n' "$DEST"
