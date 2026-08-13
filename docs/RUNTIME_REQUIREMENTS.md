# Runtime and toolchain requirements discovered in Part 1

These are pinned from the inspected upstream sources, not guesses.

## OmniRoute 3.8.50

Its `package.json` declares Node:

- `>=22.22.2 <23` OR
- `>=24.0.0 <27`

The development sandbox used during Part 1 currently exposes Node 22.16.0, which is below that
minimum. Therefore OmniRoute is not executed in Part 1. This is not treated as a product failure;
the first full compile/runtime bring-up is explicitly deferred. Before the OmniRoute runtime part,
the toolchain must satisfy the upstream Node engine range.

The inspected source and docs agree on the default local service address:
`http://localhost:20128`, with the OpenAI-compatible API rooted at `/v1`.

## Jcode 0.73.0

Jcode uses Rust edition 2024 and the vendored Jcode SDK uses stabilized `let`-chain syntax. The
minimum VibeCoder Rust toolchain is therefore pinned to **1.88** rather than merely the first 2024
edition release. Rust 1.88 is the release that stabilized let chains.

Part 25 installed a task-local official Rust 1.88.0 toolchain, generated the root lockfile, and
completed the first full host compile/test loop. This proves the Rust workspace baseline only;
Android cross-compilation and on-device runtime bring-up remain separate work.

## Jcode SDK boundary discovered in Part 2

The pinned Jcode 0.73.0 public SDK crates are `jcode-sdk` 0.1.0 and `jcode-harness-api` 0.1.0.
The harness protocol major is `1`. The vendored Rust SDK currently uses Unix-domain sockets and is
therefore a Unix/Linux backend component, not code intended to execute inside the eventual Android
UI process.

The private runtime path expects a `jcode` executable (or an explicit configured binary) and starts
the harness API bridge. Parts 2-5 validate configuration, transport, session/turn mapping, and permission mediation at source
level; actual runtime bring-up remains deferred under the Part-25 first-full-compile rule. The Jcode
SDK is synchronous, so long turn execution already runs on a dedicated blocking worker while cancel
and permission-response control paths use independent cloned SDK handles.

## Part 7 OmniRoute client dependency

VibeCoder now pins reqwest `0.12` with Rustls TLS and `url` 2.x for the outbound OmniRoute HTTP
boundary. Redirects and ambient proxies are disabled by the adapter. The adapter itself can talk to
an already-running OmniRoute service and therefore does not require starting OmniRoute's Node
runtime during Part 7. Running the bundled/upstream OmniRoute application later still requires the
Node engine range recorded above.

## Android local-first target chosen before Part 8

The private product must not require a rented/owned remote server. Jcode, the gateway, workspace and
build toolchains are intended to execute on the Android device, potentially as separate local
processes/services. The existing loopback OmniRoute URL is therefore product-relevant, not merely a
development convenience.

This checkpoint does **not** claim that OmniRoute's Node >=22.22.2 runtime or the Jcode executable
has already been packaged for Android ARM64. Those are explicit local-runtime bring-up blockers to
prove or replace behind the existing contracts. Remote AI model APIs are still expected when a
remote model is selected.


## Part 10 secret runtime note

Phone production requires an Android implementation of `AppSecureStoreBackend`; Part 10 defines and validates the contract but does not claim the Keystore bridge is packaged yet. The explicit environment resolver is for development/testing only.

## Part 12 Android/Unix file primitive dependency

The managed file runtime now uses the Rust `libc` 0.2 boundary for Unix/Android `openat`, `mkdirat`, `fstatat`, `fstat`, `fchmod`, `renameat`, `unlinkat`, and `fsync` operations plus `O_NOFOLLOW`/`O_DIRECTORY`. No non-Unix insecure fallback is enabled. Android runtime packaging is still not claimed proven until the scheduled runtime/compile milestones.


## Part 15 Unix/Android process primitives

The local executor now depends on Unix/Android process facilities exposed through Rust `std` plus
`libc`: `setpgid`, process-group `kill`, nonblocking pipe `fcntl`, null/piped stdio, and a clean
child environment. Runtime tools are registered by opaque id + path relative to a separate
package-installed executable-code root; writable app-private `vibecoder/runtime` is data-only and
ambient PATH lookup is not used as authority.

This checkpoint proves the source boundary only. It does not claim that Jcode, Node/OmniRoute, the
JDK, Gradle, `aapt2`, or other Android build binaries have been packaged and executed successfully
on a physical Android ARM64 device. That runtime bring-up remains a later milestone.

## Part 16 persistence primitives

The local state adapter requires the same Unix/Android fd-relative primitives already used by safe
workspace I/O: `open/openat`, `O_NOFOLLOW`, `fstat/fstatat(AT_SYMLINK_NOFOLLOW)`, `renameat`,
`unlinkat`, and `fsync`. Android app code must inject an existing app-private base directory. No
external/shared-storage state root is supported by this checkpoint.


## Part 18 build abstraction

No new external runtime is introduced by Part 18. Build jobs are a provider-neutral wrapper over the Part 15 local process runtime. Website Node/package-manager detection starts in Part 19; Android JDK/Gradle/SDK bring-up remains later Android-build work.


## Part 20 website runtime note

The website pipeline uses logical runtime ids (`npm`, `pnpm`, `yarn`, `bun`) that the process layer resolves only through the trusted runtime-tool registry. Part 26 requires native launchers behind that registry to come from package-installed code, not writable app data. Locked install command semantics are now modeled, but this checkpoint does not claim those package-manager launchers or Node have been packaged/executed on Android ARM64. `engines.node` is retained as advisory metadata; general npm-semver range verification against a provisioned Node version remains a later runtime-bring-up requirement.


## Part 26 Android executable-placement correction

Do not provision executable code under writable app-private `vibecoder/runtime`. That Part-15
host-Linux placement is incompatible with Android's API-29+ writable-app-home execution restriction.
The writable tree remains valid for HOME/TMPDIR/state and non-code data. Native process executables
must come from a distinct package-installed code root and must pass the Part-26 ARM64, execution,
version, and 16 KB compatibility evidence gates before they are registered as Android-ready.

## Part 27 Android host/probe refinement

The first Android host crate now produces `libvibecoder_android_host.so`. Jcode and Node are modeled
as package-owned native child executables, but the host requires a real filesystem executable root
rather than inferring child-process availability from JNI library metadata. npm remains an APK data
bundle and cannot become Website Build authority from asset presence alone; a verified Node→npm
runtime binding is required. OmniRoute similarly requires a service round trip before the Gateway
capability can be ready. Physical-device proof remains pending.

## Part 31 reviewed runtime-identity evidence

Runtime inventory rows now carry explicit `version_requirement_pinned` state. Jcode 0.73.0, Node 24.19.0, the VibeCoder core identity, and the reviewed OmniRoute bundle identity are pinned in the current Android inventory. npm and the future phone-local Android build toolchain remain explicitly unpinned and therefore cannot become ready merely because files happen to be present.

Native executable diagnostics now serialize the expected version requirement and the semantic version observed from the trusted executable. Exact production pins remain exact; the probe parser also supports the bounded OR-range grammar already used by the test/runtime compatibility contract.
