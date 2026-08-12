# Part 30 — APK verification and physical-device proof boundary

## Goal

Turn the diagnostic shell into a machine-verifiable Android test target. A successful Gradle task is not enough: the APK must be aligned, signed, ARM64-only, the app must launch on a real ARM64 Android device, the Rust host must load, and the private Jcode lane must complete its Unix-socket API round trip before Agent readiness is accepted.

## APK proof

`scripts/verify_android_diagnostic_apk.sh` verifies the debug APK with pinned SDK Build Tools. It requires 16 KiB zip alignment, a valid APK signature, only `arm64-v8a` native entries, the JNI bridge, the Rust host, and in Jcode mode the packaged Jcode child executable. Every packaged ELF is also passed through the VibeCoder Android ELF verifier.

## Device proof

The minimal Android activity atomically persists `files/vibecoder-diagnostic-result.json`. `scripts/test_android_diagnostic_device.sh` installs the APK with adb, launches the activity, reads that app-private report through `run-as`, and fails closed unless the device is ARM64/API 29+, the Rust host loaded, the host probe succeeded, and Core is READY.

Jcode mode adds stronger acceptance: Agent must be READY and Jcode package presence, AArch64 identity, execution, exact version, 16 KiB compatibility, and private Unix-socket round trip must all be `passed`.

## Current runner evidence

This runner still has no Android SDK/NDK, Gradle, Rust/Cargo, or adb device. During Part 30 the current official Linux Android command-line tools package `15859902` was identified with SHA-256 `4e4c464f145a7512b57d088ac6c278c03c9eea610886b35a5e0804e74eedf583`, and a verified binary download was attempted, but the container download path failed. Therefore no APK build, install, or physical-device handshake is claimed by this checkpoint.

The GitHub Actions workflow remains the reproducible build lane once this source is placed in a connected repository. The minimal APK job is independent from the Jcode cross-compile job so a Jcode porting failure cannot suppress the first installable diagnostic shell.
