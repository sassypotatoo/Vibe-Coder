# Part 26 — Android ARM64 runtime packaging and execution-readiness boundary

Part 26 starts the second half of VibeCoder development. It does **not** add the production UI.
It repairs one Android portability bug from the host-only baseline and creates a fail-closed inventory
for the runtime binaries/data that must eventually execute on the phone.

## Confirmed Part-25 portability bug repaired

Part 15 placed trusted runtime executables beneath writable app-private `vibecoder/runtime`. That is
valid on ordinary desktop Linux but is not a valid Android 10+ application design: apps targeting
API 29+ cannot directly `execve()` files from writable app home. Part 26 therefore separates:

- writable app-private state/data: projects, HOME, TMPDIR, extracted non-code assets;
- package-installed executable code: an Android platform-supplied code root, expected to be backed
  by the APK/AAB native-library installation path.

`LocalProcessRuntime::initialize` now requires both roots. Runtime-tool paths are relative to the
package-installed executable root, never the writable runtime-data root. The roots may not overlap.
On Android the package-code root is rechecked as a non-writable directory before executable
resolution.

Direct `WorkspaceExecutable` launches are also fail-closed on Android because the project tree is writable app data; scripts must be passed to a trusted package-installed interpreter instead.

This is an architecture correction, **not** a claim that an arbitrary `.so` can already be invoked
on every Android device. The actual package layout and `execve` probe remain mandatory evidence.

## Versioned ARM64 inventory

`config/android-runtime-inventory.json` now records the current phone-local inventory. The first
entries are:

- VibeCoder Rust core (`aarch64-linux-android`, in-process native code);
- Jcode 0.73.0 (native process target, version + Unix-socket round-trip probe);
- Node satisfying OmniRoute 3.8.50's reviewed engine range;
- OmniRoute 3.8.50 plus the deterministic-routing patch as non-executable packaged data;
- npm CLI data for the first website-build path;
- Android-build placeholders for JDK, Gradle launcher, android.jar, aapt2, zipalign, D8/R8 and
  apksigner. Exact Android-build distributions/versions remain intentionally unpinned until their
  later build-toolchain part proves a real compatible source.

The inventory rejects native code in `writable_app_data`.

## Readiness evidence

`vibecoder-runtime-packaging` evaluates each component from explicit proof states. A component does
not become ready merely because it is listed. Depending on artifact class, the Android adapter must
provide:

- package presence;
- ARM64 native identity for native code;
- successful process execution for native executables;
- version corroboration when required;
- Jcode Unix-socket bind/connect round-trip;
- 16 KB page-size compatibility proof for every native artifact.

`NotRun` and `Failed` are both blocking. The report exposes separate Core, Agent, Gateway, Website
Build and Android Build readiness, and `backend_ready()` requires Core + Agent + Gateway.

## Why 16 KB is tracked now

Android 15 introduced 16 KB page-size devices, and current Android distribution requirements expect apps/updates
targeting Android 15/API 35+ to support 16 KB page sizes. Since VibeCoder ships native code, every
packaged native artifact must carry explicit 16 KB compatibility evidence rather than assuming a
4 KB-only build is publishable.

## Still intentionally unproven after Part 26

- physical Android device execution;
- exact APK/AAB packaging for Jcode or Node executable artifacts;
- a compatible Android ARM64 JDK distribution;
- Gradle/Android SDK build execution on-device;
- package-manager variants beyond the baseline inventory;
- strong same-UID process isolation;
- Android Keystore adapter;
- production UI.

The next implementation part should consume this boundary from an Android host shell/app module and
produce real on-device probe evidence instead of adding another optimistic portability flag.

## Validation performed in this runner

The Part 26 source boundary is statically validated here, but it is **not** being presented as a
new compiled baseline. This runner does not contain `rustc` or `cargo`, so the modified Rust crates
could not be recompiled or clippy-checked after the Part 26 edits. The Part 25 checkpoint therefore
remains the last host-compiled baseline. Android ARM64 cross-compilation and physical-device probes
remain false and are deliberately carried forward as readiness blockers.
