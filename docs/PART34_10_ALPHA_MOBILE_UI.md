# Part 34.10 — minimal Alpha mobile UI foundation

This slice implements only UI that the current source can represent honestly.

## Implemented

- Portrait Android shell with VibeCoder app bar.
- Slide-out **Old Chats** drawer.
- Read-only rendering of already-persisted conversation files from
  `filesDir/vibecoder/state/conversations`.
- **Chat** and **Preview** tabs.
- New-chat visual reset, message composer, Send and Stop controls.
- Runtime diagnostics preserved behind the settings button; automatic diagnostics run only when
  the physical-device acceptance harness supplies its explicit diagnostic-test intent extra.

## Deliberately not faked

The durable Rust controller exists, but Android JNI does not yet expose the conversation/action
entry point. Therefore Send does not manufacture an assistant response or write a second Java chat
store. Stop is visible but disabled until real turn cancellation authority reaches the UI.

Preview is a real tab, but the live local preview runtime is not wired in this slice. The UI shows
an explicit placeholder instead of a fake browser URL, deployment, build output or device preview.

## Persistence boundary

The drawer is read-only. Rust remains the sole writer/validator of persisted project and
conversation state. Java applies bounded reads, rejects symlinks, requires direct children of the
app-private conversation directory, limits file size, and renders only `user`/`assistant` roles.

## Part 34.10.1 deep-audit hardening

A post-UI source/compile audit found and fixed concrete issues instead of treating the earlier static
validator pass as a compile proof:

- Strict Java 17 warnings-as-errors exposed an unused try-with-resources `FileLock`; the installer
  now validates the acquired lock and the CI workflow has a dedicated `javac -Xlint:all -Werror`
  gate against the pinned Android platform jar.
- Old-chat directory enumeration and JSON parsing moved off the main/UI thread onto a lifecycle-
  owned single-thread executor. Teardown invalidates generations, shuts the executor down, and stale
  work cannot resurrect the Activity.
- The Java read ceiling now matches Rust's 16 MiB persisted-conversation ceiling, and candidate files
  are sorted newest-first before the 50-chat UI cap is applied.
- Conversation filenames must contain canonical UUIDs and the JSON project/conversation identity must
  match that filename. Pending-session state is also corroborated before a chat is rendered.
- Large valid conversations stay persisted untouched but the Alpha UI bounds rendered text to 512 KiB
  total and 64 KiB per message with Unicode-safe truncation, avoiding a multi-megabyte TextView burst.
- Because targetSdk 36 is edge-to-edge on modern Android, the root view applies status/navigation-bar,
  cutout and IME insets on Android 15+. The manifest retains resize behavior for the soft keyboard.
- The custom Old Chats overlay registers an Android 13+ overlay-priority Back callback while open, so
  Back dismisses the drawer before normal activity navigation.

This audit still does **not** claim a real Android/Gradle/Rust build in this runner: Android SDK/NDK,
Gradle 9.5.0 and Cargo/Rust are not installed here. The temporary local Java platform stubs prove
Java 17 syntax/type/warning cleanliness only; the CI strict-Java step and full APK build remain the
external compile authority.

## Part 34.10.2 — automatic local runtime + real normal-chat UI bridge

The Alpha shell no longer leaves Send as a visual-only control. On Activity startup, a background
bootstrap now performs the package-owned runtime path automatically:

1. read the pinned Android runtime inventory;
2. install or reuse the sealed OmniRoute APK asset through `OmniRouteAssetInstaller`;
3. require the installed manifest SHA to verify;
4. start (or reuse) packaged Node + OmniRoute through the existing Rust supervisor;
5. require active/ready, exact-model-only, hidden-reroute-disabled runtime attestation; and
6. initialise a process-local Rust app controller and fetch a fresh exact-model catalog.

There is no end-user OmniRoute ZIP/runtime installation step. If the APK does not actually contain
the generated sealed runtime bundle or packaged Node executable, bootstrap fails closed and the
composer remains disabled. Provider authentication/account availability is a separate concern: when
OmniRoute exposes no usable model, the UI reports that state instead of inventing a model or silently
changing routing policy.

`New Chat`, `Send`, and `Stop` now cross Java -> JNI -> Rust. New chats are durable Rust-owned
projects/conversations. Send uses the existing one-shot persisted **normal conversation** controller,
so the user message is durably committed before one exact model request and the assistant reply is
durably committed before returning to the UI. Stop owns a real cancellation token; Java retries the
cancel entry point briefly to close the race where Stop is tapped just before the Rust turn registers.
The Rust inference future is polled with bounded timeout slices so cancellation drops the in-flight
HTTP future without enabling Tokio macro features or adding a hidden retry/model fallback.

This UI bridge intentionally wires normal AI conversation first. The already-implemented Jcode
coding-action controller is **not yet selected automatically by the UI**, so `coding_agent_send_ui_wired`
remains false. Preview also remains a placeholder.

The chat JNI result path no longer decodes arbitrary model output with JNI `NewStringUTF` (modified
UTF-8). Rust JSON bytes are decoded using Java's standard UTF-8 `String(byte[], "UTF-8")` constructor,
so supplementary Unicode such as emoji cannot corrupt a valid assistant reply. Live chat rendering is
bounded with the same 64 KiB Alpha display limit used for restored messages; the complete persisted
assistant response is not truncated by the UI.

Source-side Java 17 `-Xlint:all -Werror` compilation was rerun against local Android API stubs, and
the generated `NativeBridge` JNI header was force-included while compiling `native_bridge.c` with
Clang `-Wall -Wextra -Werror`. This runner still has no Android SDK/NDK or Rust/Cargo, so a real Gradle
APK build, fresh Rust compile, Node/OmniRoute execution and physical chat round trip remain external
acceptance gates rather than claimed proof.

### Part 34.10.2 final bootstrap hardening

- The Android controller filters the fresh OmniRoute catalog to model IDs accepted by the durable conversation contract (non-empty, <=512 bytes, trimmed, ASCII-graphic) before deterministic selection.
- App-open controller/catalog readiness is retried at most 4 times with 250 ms between attempts. This is startup readiness polling only: it sends no inference request, consumes no model quota, and does not introduce model retry/fallback behavior.
- After the bounded warm-up window, an empty catalog remains an honest `provider_setup_required` state; the UI does not fabricate a model.

## Part 34.10.3 — pre-compile deep audit and one-APK Alpha build lane

A final pre-compile audit found a packaging-level blocker that the earlier component-specific proof
jobs did not catch: Jcode, Node and the OmniRoute runtime bundle were each prepared/proven in
separate lanes, but no build job assembled all three payloads into the **same** APK. A normal-chat
bootstrap can only be usable when the installed package contains the Android host/JNI libraries,
Jcode executable, Node executable and sealed OmniRoute asset together.

The source now has a fail-closed full-Alpha package lane:

1. the exact Jcode 0.73.0 Android payload is taken from the Jcode proof job;
2. the exact Node 24.19.0 Android payload plus its cross-build evidence is taken from the Node proof
   job;
3. the exact reviewed OmniRoute 3.8.50 branch archive is fetched from `release/v3.8.50` and must
   match SHA-256 `1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7`
   and embedded reviewed commit `ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f`;
4. OmniRoute is admitted, patched, backend-built with host Node 24.19.0, sealed and independently
   verified before staging under generated APK assets;
5. the Rust Android host and JNI shell are built without deleting the staged Jcode/Node payloads;
6. the final APK verifier requires host + JNI + Jcode + Node + verified OmniRoute bundle in one
   signed, zipaligned, arm64-only package; and
7. package evidence records hashes but deliberately leaves device-execution claims false.

The earlier OmniRoute fetch prototype incorrectly guessed a `v3.8.50` tag. The reviewed archive root
and upstream ref are actually `release/v3.8.50`; the corrected fetcher downloads that exact branch
archive and accepts it only when both the reviewed archive SHA and embedded commit comment match.
A moving branch therefore cannot silently change the build input.

Normal app startup also no longer launches the heavyweight diagnostic suite in parallel with the
chat runtime bootstrap. Diagnostics auto-run only when the physical-device harness passes the
`vibecoder_diagnostic_test` intent extra; the settings button still allows manual diagnostics. This
avoids duplicate OmniRoute asset hashing/startup work on every ordinary Activity launch.

This is still **source/build-lane readiness**, not a claim that the full Alpha APK has compiled. The
current audit runner lacks Android SDK/NDK, Gradle and Rust/Cargo. The next external compile is the
first authority that can turn `full_alpha_apk_compiled` and package/device proof fields true.

The device harness also defines an `alpha` acceptance mode for the eventual combined APK. That mode
requires Jcode device readiness, exact Node 24.19.0 device execution, verified OmniRoute asset
installation, loopback service readiness/attestation and explicit service stop in the **same installed
package**. It deliberately does not require a paid/provider-specific model request, so package/runtime
acceptance remains separable from provider account setup.


## Part 34.10.4 compile-log repair

The first GitHub Actions compile exposed two concrete issues. `vibecoder-core` now declares its direct Tokio time dependency and the local Cargo.lock package dependency list is aligned. Node Android cross-builds now explicitly split host GCC/G++ tools from NDK AArch64 target tools at make time; generated Makefile and build-log checks fail closed if an `obj.host` recipe uses the Android target compiler. This repairs the observed failures without patching or weakening the pinned Node source archive. A successful Node Android binary and full Alpha APK remain evidence gates, not source claims.

### Part 34.10.5 — second compile-log loop repair
- GitHub run 85903371815 confirmed the previous Tokio and Node host/target compiler fixes progressed the build.
- Fixed `vibecoder-process-local` runtime-service argv validation to use the existing `is_forbidden_display_char` policy instead of an undefined helper.
- Removed the production-only unused `GatewayChatMessage` import while preserving the test import.
- Node Android generated `.host.mk` files are sanitized only for the proven ARM64-only `-mbranch-protection=*` leakage; target makefiles remain untouched.
- Node log verification rejects ARM64 branch-protection flags on `obj.host`, and cross-build evidence is bound to the sanitizer source.
- CI now triggers on any `crates/**` change so process/runtime crate fixes cannot silently skip Android compilation.
- Fresh GitHub recompile is still required; no successful Node/Full Alpha binary is claimed by this source checkpoint.
