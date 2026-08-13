# Part 27 — Android host integration and packaged-runtime probes

Part 27 remains UI-free. Its job is to turn the Part-26 Android packaging inventory into an actual
Android host boundary without pretending that a desktop Linux source tree is a phone.

## Confirmed integration bugs repaired

1. **The core artifact had no Android `cdylib` producer.** Part 26 named a native core artifact, but
   `vibecoder-core` is an ordinary Rust library. Part 27 adds `vibecoder-android-host` with
   `crate-type = ["rlib", "cdylib"]` and a tiny stable C ABI smoke symbol. The logical core artifact
   is now `libvibecoder_android_host.so`.
2. **Private Jcode could fall back to ambient PATH.** The Android host always supplies an explicit
   package-owned Jcode executable path and a private writable Jcode home. PATH lookup is not an
   Android fallback.
3. **npm was modeled like a native executable.** npm is script/data interpreted by Node. The local
   process runtime now supports a bounded trusted fixed argv prefix before project-controlled args.
   Android does not register npm until its packaged data entrypoint is pinned, materialized and
   verified; the readiness model separately requires a runtime-binding proof.
4. **OmniRoute asset presence could overstate gateway readiness.** The OmniRoute inventory row now
   requires a real service round trip. Bytes existing in an APK do not prove a gateway is serving.
5. **`nativeLibraryDir` was too broad an assumption for child processes.** Modern Android packaging
   may load JNI libraries directly from an APK instead of extracting every library as a normal
   filesystem file. `AndroidHostPaths` therefore distinguishes the JNI native-library directory
   from a package-owned child-executable filesystem directory. They may be the same only when the
   Android package layout really exposes those files.

## Android host boundary

`crates/vibecoder-android-host` owns three roots supplied by the future Android shell:

- writable app-private data;
- JNI/native-library code;
- package-owned child-process executables that are actually present as filesystem files.

The writable data root must not overlap either code root. On Android both code directories are
checked as non-writable by the app UID. `LocalProcessRuntime` receives only the child-executable
root. Jcode and Node are resolved as direct package-root children, never through PATH.

The host exports `vibecoder_android_host_abi_version()` as a minimal C ABI smoke boundary. It is
not a UI API and it does not claim JNI/Kotlin integration is finished.

## Native probe implementation

`vibecoder-runtime-packaging::native_probe` performs bounded structural checks for package native
artifacts:

- ordinary file, not symlink;
- ELF64 little-endian;
- AArch64 `e_machine`;
- bounded program-header table parsing;
- every `PT_LOAD` segment aligned/congruent for 16 KiB page compatibility.

On a non-Android host, structural evidence can be collected but execution and version evidence stay
`NotRun`. On Android, native executable version probes use bounded stdout/stderr and a hard timeout.
The currently pinned Jcode and Node requirements have explicit version matching.

Jcode additionally has a private-runtime/API-socket round-trip probe. A successful `--version`
process is not enough to mark the Agent capability ready.

## Readiness remains fail closed

The Part-26 proof states are extended with:

- `service_round_trip` for runtime services such as OmniRoute;
- `runtime_binding` for data interpreted by another runtime, such as npm through Node.

Therefore:

- OmniRoute asset presence alone cannot mark Gateway ready;
- npm asset presence alone cannot mark Website Build ready;
- a desktop structural ELF pass cannot turn into Android execution evidence;
- missing package files become failed package-presence evidence, not optimistic unknown success.

## Still intentionally unproven after Part 27

This checkpoint does **not** claim any of the following:

- Rust Part-27 changes were recompiled in this runner; `rustc`/`cargo` are unavailable here;
- `aarch64-linux-android` cross compilation succeeded;
- an APK/AAB packages `libvibecoder_android_host.so`;
- a physical Android ARM64 device loaded the host library;
- package-owned child executables are installed at a real executable filesystem path;
- Jcode or Node ARM64 payloads are packaged or successfully executed;
- Jcode's Unix socket round trip passed on Android;
- OmniRoute is packaged, launched or serving its local HTTP API;
- npm is pinned/bound to a verified Node interpreter;
- the Android JDK/Gradle/SDK/build-tools payload exists;
- production UI exists.

Part 25 remains the last genuinely compiled Rust baseline. Part 27 is a source/static checkpoint and
keeps device/runtime proof explicitly false.

## Next boundary

Part 28 should add the minimal Android shell/APK packaging layer, choose and pin the real ARM64
payload sources/layout, provide the three host roots from Android package metadata, and invoke the
Part-27 probes on-device. It must preserve the fail-closed evidence model rather than replacing it
with build-time assumptions.

### Probe timeout hardening

The first probe draft used reader threads and joined them after killing or observing the direct
child. That can hang if a descendant inherits stdout/stderr and keeps the pipe open. The final Part
27 probe sets both pipes nonblocking, drains/discards beyond the capture cap, polls the direct child
under the timeout, and never waits for pipe EOF after child exit. A descendant-held pipe therefore
cannot turn the version-probe timeout into an unbounded join.
