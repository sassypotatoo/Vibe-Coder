# Part 29 — Jcode Android ARM64 packaging boundary

Part 29 refuses to treat a generic Linux ARM64 binary as an Android runtime payload.

## Pinned Jcode source

- repository: `https://github.com/1jehuang/jcode.git`
- tag: `v0.73.0`
- commit: `44ffa55281fad71c02be984c0674d92412210452`
- package version: `0.73.0`
- Android Rust target: `aarch64-linux-android`
- minimum Android API used by the build script: 29

The historical `jcode-master.zip` SHA-256 remains recorded only as provenance for the already
vendored `jcode-sdk` + `jcode-harness-api` public boundary. It is not the authority for the Android
runtime executable.

## Why the Linux ARM64 release is not reused

Jcode's v0.73 release workflow builds `aarch64-unknown-linux-gnu` and contains Termux-specific
handling. VibeCoder therefore builds the exact pinned source with the Android NDK/Bionic target
instead of renaming a glibc Linux executable and hoping Android accepts it.

## Build path

1. `scripts/fetch_jcode_android_source.sh` obtains the exact commit when network is available.
2. `scripts/verify_jcode_android_source.sh` requires the exact commit/version and rechecks the
   vendored SDK/harness files against `third_party/jcode/VENDORED_MANIFEST.sha256`.
3. `scripts/build_jcode_android.sh` cross-compiles `--target aarch64-linux-android`, with default
   feature stacks disabled for the first minimal agent runtime, and stages the resulting binary as
   `libvibecoder_jcode_exec.so` under the APK's ARM64 native payload directory.
4. `scripts/verify_android_elf.py` rejects non-AArch64, non-PIE, non-16-KB-compatible binaries and
   rejects foreign dynamic interpreters such as glibc's loader. Android executables may use
   `/system/bin/linker64`; static PIE may omit PT_INTERP.
5. The existing Android host must still execute the packaged file, validate its version, then start
   `jcode api-bridge --api-socket <private socket>` and complete the Jcode SDK round trip before the
   Agent capability becomes READY.

## Current checkpoint evidence

This source checkpoint implements and statically validates the provenance/build/probe path. The
current runner still lacks Rust/Cargo, Android SDK/NDK and Gradle, so it cannot honestly claim that
Jcode was cross-compiled, packaged into an APK, installed, or socket-tested on a physical Android
phone. Those remain the next required proofs, not assumed outcomes.
