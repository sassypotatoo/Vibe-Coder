# Part 31 Reviewed Source Fixes

This checkpoint applies only findings that survived source verification of the external Part-31 review. It does not claim a new Android APK compile or physical-device pass.

## Applied fixes

- Removed the duplicate Jcode `--version` execution from the socket-round-trip path. The stronger handshake now consumes the native evidence already collected in the same diagnostic run.
- Preserved both the expected runtime version requirement and the parsed observed semantic version in serialized component evidence.
- Replaced the Node-only magic range branch with a small deterministic comparator/range parser that supports exact `major.minor.patch` and bounded OR ranges such as `>=22.22.2 <23 || >=24.0.0 <27`.
- Added explicit `version_requirement_pinned` inventory state. Unpinned Android build/runtime placeholders now fail readiness with `runtime_version_requirement_unpinned` instead of looking like ordinary version mismatches.
- Added Android APK-asset presence evidence from Java `AssetManager` and a backwards-compatible Rust FFI v2 symbol so DataBundle/JavaArchive package presence is no longer structurally unobservable.
- Cached the Rust host `dlopen`/`dlsym` boundary once in `JNI_OnLoad`; the JNI bridge no longer opens/closes the host library on every diagnostic request.
- Documented the intentional dependency between the shell's shared native/executable root and `packaging.jniLibs.useLegacyPackaging = true`.
- Added an explicit Android-host Tokio executor boundary for synchronous JNI callers that need async agent/gateway/core APIs. Nested synchronous `block_on` from an already-active Tokio context fails closed.
- Made missing/incomplete Gradle-wrapper prerequisites explicit while retaining the CI/system-Gradle path only after exact Gradle 9.5.0 verification.
- Promoted the 250 ms SIGTERM→SIGKILL process-group grace period into the public process contract and documented the resulting cancellation latency tradeoff.
- Clarified that `access(W_OK)` is an app-identity writability sanity check, not a cryptographic integrity or complete platform-policy attestation.

## Findings deliberately not applied as code changes

- The proposed Apple-Silicon NDK host tag change to `darwin-aarch64` was rejected. The pinned Android NDK host-tool directory remains `darwin-x86_64`; both Android build scripts retain that mapping.
- Production Node remains deliberately pinned to exact `24.19.0`. The generalized range parser is maintenance hardening, not permission to silently accept an unreviewed Node update.
- OmniRoute's multi-segment asset path remains valid while the component is a DataBundle. Existing native-path validation already rejects multi-segment paths if a future native artifact is modeled.

## Evidence status

The current runner still has no Rust/Cargo or Android SDK/NDK/Gradle toolchain, so the modified Rust source is not claimed compiled and no APK/device success is claimed. The reviewed-fixes checkpoint passed its source validator, Java stub compilation with warnings denied, JNI C syntax compilation with warnings denied, Bash/Python syntax checks, JSON/TOML/YAML/XML parsing, checksum coverage, and fresh-archive integrity validation.
