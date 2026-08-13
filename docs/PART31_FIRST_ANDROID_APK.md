# Part 31 — First real APK build boundary

Part 31 keeps the production UI deferred. Its only UI is the diagnostic shell.

## Previous-loop repair

Part 30 recorded Android command-line-tools build `15859902`, but the GitHub Actions workflow did not pass that build to `android-actions/setup-android`. The action can otherwise install its own default command-line-tools revision. Part 31 pins `cmdline-tools-version: 15859902` explicitly and keeps the published checksum in project state.

## One-command build lane

`scripts/part31_build_and_verify.sh minimal` performs checkpoint validation, Android Rust host build, APK build, APK verification, and writes machine-readable build evidence.

`scripts/part31_build_and_verify.sh jcode` additionally requires a pre-staged, Android/Bionic-verified Jcode executable.

The evidence JSON records the APK SHA-256, native-entry SHA-256 values, source manifest identities, and tool versions. It is evidence of a build, not evidence of physical-device execution.

## Device acceptance

A real device is still required for `scripts/test_android_diagnostic_device.sh`. The minimal lane requires Core READY. The Jcode lane additionally requires `agent_ready=true` and the Jcode Unix-socket round trip to be `passed`.

## Current runner limitation

This chat runner still cannot reach the Android SDK binary host and does not have Android SDK/NDK, Gradle, Cargo, or Rust preinstalled. Therefore Part 31 does not fabricate an APK build result here. The CI/local build lane is now deterministic and produces evidence whenever run in a networked Android build environment.

## Generated native payload isolation

The Part 30 re-audit also found that the Android host/Jcode build scripts staged generated `.so` files inside `src/main/jniLibs`. That mutates the checksummed source tree and makes a Jcode CI build fail its own source-integrity validator once the generated executable appears. Part 31 stages generated ARM64 payloads under `android/app/build/generated/jniLibs/arm64-v8a` and Gradle packages that generated directory explicitly. `src/main/jniLibs` remains source-only and contains no generated runtime binaries.

The source checksum validator now explicitly excludes generated/ephemeral build roots such as `android/app/build/`, `target/`, `.toolchains/`, `.gradle/`, and Python bytecode caches. Those locations are not source authority. The Part 31 build lane also clears generated JNI payloads before a minimal build, and preserves only the freshly verified Jcode payload for Jcode mode, preventing stale local artifacts from changing lane identity.

## Stable diagnostic signing identity

Part 31 pins a diagnostic-only debug signing key at `android/signing/vibecoder-diagnostic-debug.jks`. Its certificate SHA-256 is `9d73bfaeb16e706723bfc417ce43a9ed6b10286835e8a3050a8ddded67506445`. The APK verifier rejects any other signer, and build evidence records the expected certificate and keystore hashes. This key is intentionally development-only and is forbidden for production release signing. Stable debug signing allows successive diagnostic APKs to update the same installed test app without forcing an uninstall/data wipe.
