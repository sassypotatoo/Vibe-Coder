# Part 28 — Minimal Android shell and runtime payload provisioning

Part 28 is the first Android application shell for VibeCoder. It is intentionally a diagnostic
surface, not the production UI. Its purpose is to turn the Part-27 Android host/readiness contracts
into an APK-facing integration boundary without manufacturing runtime success.

## What was added

- `android/` is a real Android application project for `arm64-v8a` only.
- The shell uses `compileSdk 36`, `targetSdk 36`, `minSdk 29`, AGP 9.3.0, Gradle 9.5.0, and the
  NDK r28 line recorded in `config/android-payload-provisioning.json`.
- A tiny C JNI bridge loads `libvibecoder_android_host.so` dynamically and resolves a stable C ABI.
- `vibecoder-android-host` exposes a bounded JSON snapshot FFI for package/runtime readiness.
- The minimal screen shows only measured states: ARM64 device, JNI bridge, Rust host, core, Jcode,
  OmniRoute, website build, Android build, expected native files, and the raw bounded probe snapshot.
- Diagnostic work runs off the Android main thread so runtime probes do not deliberately freeze the UI.
- The same runtime inventory and provisioning metadata checked at source level are bundled as assets.

## Confirmed integration bug repaired during this loop

The first Part-28 FFI draft called the Jcode socket round-trip even when the Jcode executable was not
packaged. A missing optional-at-this-stage payload therefore turned the complete snapshot into an FFI
error instead of producing the correct fail-closed `Jcode NOT READY` evidence. The FFI now attempts the
socket round-trip only after Jcode package presence, AArch64 identity, execution, version, and 16 KiB
proofs have all passed.

## Package-owned native executable strategy

Part 27 requires child-process native code to be a real package-owned filesystem file, not a copied
executable under writable app data. The diagnostic APK therefore requests legacy JNI packaging for the
Part-28 `libvibecoder_*_exec.so` payload convention, making installer extraction the intended child-code
path. The Android host still validates the exact directory and file at runtime; the Gradle setting is
not treated as proof.

## Pinned provisioning inputs

`config/android-payload-provisioning.json` separates build-time shell tooling from future in-app Android
build-tool payloads.

- VibeCoder Android host: built from this workspace for `aarch64-linux-android`.
- Node: source pin `24.19.0` with SHA-256
  `f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f`.
- Jcode: the existing reviewed `0.73.0` archive identity is preserved; the full reviewed archive is not
  present in this checkpoint and is not replaced with a guessed URL or unrelated release.
- OmniRoute: the existing reviewed `3.8.50` archive identity is preserved; the full archive is likewise
  still an external reviewed input.
- Gradle wrapper: the distribution URL and official Gradle 9.5.0 distribution digest are pinned. The
  wrapper JAR itself is not fabricated; `scripts/bootstrap_gradle_wrapper.sh` generates it only from a
  caller-supplied distribution that passes the pinned SHA-256 check.

Provisioning scripts never turn source/archive verification into runtime readiness. Part-27 device
probes remain authoritative.

## Validation performed here

- JSON/TOML/XML structural parsing/checks.
- Bash syntax checks for every provisioning/build script.
- Python byte-code compilation for Python scripts.
- Host C compiler syntax/warning check for the JNI bridge using the installed JDK JNI headers.
- Java diagnostic-shell source compiled against temporary Android API stubs with `javac -Xlint:all -Werror`.
- APK build preflight was executed and stopped before Gradle with `android_sdk_root_missing`; no Gradle task ran.
- Full checkpoint static validator after checksum regeneration.

## Still intentionally unproven after Part 28

This execution runner does not contain Gradle, Android SDK, Android NDK, Cargo, or Rust. Attempts to
obtain external binary toolchains are unavailable from the runner network, so no APK result is
fabricated. Therefore all of the following remain false until real evidence exists:

- Android APK build success;
- Android ARM64 Rust cross-compilation;
- packaged Jcode execution/socket handshake;
- packaged Node execution;
- local OmniRoute service round-trip;
- physical-device install/run;
- website creation/build on the phone;
- in-app Android project build;
- production UI completion.

Part 25 remains the last fully compiled Rust workspace baseline. Part 28 is an Android-shell source
checkpoint that reaches the real APK build boundary without claiming to have crossed it.
