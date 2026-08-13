#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-}"
NDK_ROOT="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
API="${VIBECODER_ANDROID_API:-29}"
TARGET="aarch64-linux-android"
TRIPLE="aarch64-linux-android"
DEST="$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_jcode_exec.so"
CARGO_TARGET_DIR="${VIBECODER_JCODE_TARGET_DIR:-$ROOT/.toolchains/build/jcode-v0.73.0}"

fail() { printf 'build_jcode_android: %s\n' "$1" >&2; exit 1; }
[[ -n "$SOURCE_DIR" ]] || fail "usage: $0 /path/to/exact-jcode-v0.73.0-checkout"
command -v cargo >/dev/null 2>&1 || fail "cargo_not_found"
command -v rustup >/dev/null 2>&1 || fail "rustup_not_found"
[[ -n "$NDK_ROOT" && -d "$NDK_ROOT" ]] || fail "android_ndk_root_missing"
bash "$ROOT/scripts/verify_jcode_android_source.sh" "$SOURCE_DIR"
SOURCE_DIR="$(cd "$SOURCE_DIR" && pwd -P)"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) HOST_TAG="linux-x86_64" ;;
  Darwin-x86_64) HOST_TAG="darwin-x86_64" ;;
  Darwin-arm64) HOST_TAG="darwin-x86_64" ;;
  *) fail "unsupported_ndk_host" ;;
esac
TOOLCHAIN="$NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG/bin"
CC="$TOOLCHAIN/${TRIPLE}${API}-clang"
AR="$TOOLCHAIN/llvm-ar"
RANLIB="$TOOLCHAIN/llvm-ranlib"
STRIP="$TOOLCHAIN/llvm-strip"
[[ -x "$CC" ]] || fail "android_clang_missing:$CC"
[[ -x "$AR" ]] || fail "android_llvm_ar_missing:$AR"
[[ -x "$RANLIB" ]] || fail "android_llvm_ranlib_missing:$RANLIB"
[[ -x "$STRIP" ]] || fail "android_llvm_strip_missing:$STRIP"

rustup target add "$TARGET" >/dev/null
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC"
export CC_aarch64_linux_android="$CC"
export AR_aarch64_linux_android="$AR"
export RANLIB_aarch64_linux_android="$RANLIB"
export RANLIB="$RANLIB"
export STRIP_aarch64_linux_android="$STRIP"
export STRIP="$STRIP"
export JCODE_RELEASE_BUILD="1"
export JCODE_BUILD_SEMVER="v0.73.0"
export CARGO_TARGET_DIR
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384"

# Build the exact pinned jcode executable for Android/Bionic. Do NOT substitute the official
# aarch64-unknown-linux-gnu release: that artifact has a different libc/runtime identity.
cd "$SOURCE_DIR"

# Apply Android-specific compatibility patches. We modify the verified source to
# gate desktop-native dependencies (arboard) and guard their usages with #[cfg].
# We also preserve the existing OpenSSL vendored shim in the root Cargo.toml.
python3 -c '
import sys, os

def patch_root_cargo():
    path = "Cargo.toml"
    if not os.path.exists(path):
        raise SystemExit("jcode_root_cargo_missing")
    with open(path, "r") as f:
        content = f.read()

    # Do not use a broad substring check here. Upstream Jcode already contains
    # names such as `linux-compat-vendored-openssl`, which include both words
    # "openssl" and "vendored" without enabling the Cargo dependency feature.
    # That previously made this function return early and left openssl-sys
    # searching for a host OpenSSL installation during Android cross-compile.
    import tomllib
    parsed = tomllib.loads(content)
    dependency = parsed.get("dependencies", {}).get("openssl")
    if dependency is None:
        lines = content.splitlines(keepends=True)
        out = []
        inserted = False
        for line in lines:
            out.append(line)
            if not inserted and line.strip() == "[dependencies]":
                out.append("openssl = { version = \"0.10\", features = [\"vendored\"] }\n")
                inserted = True
        if not inserted:
            raise SystemExit("jcode_root_dependencies_table_missing")
        content = "".join(out)
        with open(path, "w") as f:
            f.write(content)
    elif not (
        isinstance(dependency, dict)
        and "vendored" in dependency.get("features", [])
    ):
        raise SystemExit("jcode_root_openssl_dependency_unexpected")

    # Fail before the expensive cross-compile if the deterministic shim did
    # not actually enable the vendored feature.
    verified = tomllib.loads(open(path, "rb").read().decode("utf-8"))
    verified_dependency = verified.get("dependencies", {}).get("openssl")
    if not (
        isinstance(verified_dependency, dict)
        and "vendored" in verified_dependency.get("features", [])
    ):
        raise SystemExit("jcode_vendored_openssl_patch_not_applied")

def patch_tui_cargo():
    path = "crates/jcode-tui/Cargo.toml"
    if not os.path.exists(path): return
    with open(path, "r") as f: content = f.read()
    if "target.\"cfg(not(target_os = \\\"android\\\"))\".dependencies" in content: return
    lines = content.splitlines(keepends=True)
    out = []
    has_arboard = False
    for line in lines:
        if line.strip() == "arboard = \"3\"":
            has_arboard = True
            continue
        out.append(line)
    if has_arboard:
        out.append("\n[target.\"cfg(not(target_os = \\\"android\\\"))\".dependencies]\n")
        out.append("arboard = \"3\"\n")
    with open(path, "w") as f: f.writelines(out)

def patch_rs_files():
    # helpers.rs: guard text and image clipboard fallbacks
    path = "crates/jcode-tui/src/tui/app/helpers.rs"
    if os.path.exists(path):
        with open(path, "r") as f: content = f.read()
        search1 = "            if arboard::Clipboard::new()"
        replace1 = "#[cfg(not(target_os = \"android\"))]\n            if arboard::Clipboard::new()"
        if search1 in content and replace1 not in content:
            content = content.replace(search1, replace1)
        search2 = "    if let Ok(mut clipboard) = arboard::Clipboard::new()"
        replace2 = "#[cfg(not(target_os = \"android\"))]\n    if let Ok(mut clipboard) = arboard::Clipboard::new()"
        if search2 in content and replace2 not in content:
            content = content.replace(search2, replace2)
        with open(path, "w") as f: f.write(content)

    # input.rs: guard clipboard text reading
    path = "crates/jcode-tui/src/tui/app/input.rs"
    if os.path.exists(path):
        with open(path, "r") as f: content = f.read()
        search = "    let Ok(mut clipboard) = arboard::Clipboard::new() else {\n        return None;\n    };\n    clipboard.get_text().ok()"
        replace = """    #[cfg(not(target_os = "android"))]
    {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return None;
        };
        clipboard.get_text().ok()
    }
    #[cfg(target_os = "android")]
    {
        None
    }"""
        if search in content and replace not in content:
            content = content.replace(search, replace)
        with open(path, "w") as f: f.write(content)

    # productivity.rs: guard image clipboard copying
    path = "crates/jcode-tui/src/tui/app/productivity.rs"
    if os.path.exists(path):
        with open(path, "r") as f: content = f.read()
        search = "fn copy_image_arboard(png: &[u8]) -> bool {"
        replace = "#[cfg(not(target_os = \"android\"))]\nfn copy_image_arboard(png: &[u8]) -> bool {"
        if search in content and replace not in content:
            content = content.replace(search, replace)
            if "fn copy_image_arboard(_png: &[u8]) -> bool { false }" not in content:
                content += "\n#[cfg(target_os = \"android\")]\nfn copy_image_arboard(_png: &[u8]) -> bool { false }\n"
        with open(path, "w") as f: f.write(content)

patch_root_cargo()
patch_tui_cargo()
patch_rs_files()
'
# Verify that the patched Cargo.toml is still valid.
cargo metadata --no-deps --format-version 1 > /dev/null || fail "patched_cargo_toml_invalid"

# We remove --locked because we have modified Cargo.toml to enable vendoring.
cargo build --release --target "$TARGET" --no-default-features --bin jcode
SOURCE="$CARGO_TARGET_DIR/$TARGET/release/jcode"
[[ -f "$SOURCE" ]] || fail "jcode_android_output_missing"
python3 "$ROOT/scripts/verify_android_elf.py" "$SOURCE"
mkdir -p "$(dirname "$DEST")"
install -m 0644 "$SOURCE" "$DEST"
python3 "$ROOT/scripts/verify_android_elf.py" "$DEST" >/dev/null
printf 'staged %s\n' "$DEST"
