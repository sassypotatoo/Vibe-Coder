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

## Part 34.10.6 — second compile-log deep-audit repair

The second GitHub Actions compile was analyzed as evidence rather than treated as a generic build
failure. The strict Java 17 gate passed, and the pinned Jcode v0.73.0 Android ARM64 release build
completed and passed the Android ELF/16 KiB checks. The remaining first-party Rust failure was in
`vibecoder-android-host`: `omniroute_service.rs` called the `ProcessRuntime::cancel` trait method
without importing the trait, and retained a readiness variable whose assignments were never read.
The trait is now imported explicitly and the unused readiness state has been removed without
weakening the two-consecutive-attestation requirement.

The Node lane had two related build-graph issues. Node v24.19.0's top-level Makefile generates
`out/Makefile` and the GYP `*.host.mk`/`*.target.mk` recipes lazily, so a host-flag sanitizer cannot
run immediately after `android-configure`. The provisioner now materializes `out/Makefile` first,
then sanitizes generated host recipes, then runs the host/target toolchain preflight, and only then
starts compilation. Earlier real CI evidence also proved that the generated `node_js2c.host.mk`
passed the Android AArch64-only `-mbranch-protection=standard` flag to Ubuntu host `g++`, which
rejected it. The sanitizer is intentionally narrow: it removes only that evidence-proven flag from
`*.host.mk`, hashes every `*.target.mk` before and after, and fails if the expected host flag is not
present or any target recipe changes. No pinned Node source file is patched.

The deep regression sweep also found a stale OmniRoute Android runtime-profile SHA-256 assertion.
The source-authority profile already had checksum `c9d8cfa91c5d8ec1e4f5862fe4d6e6266ad02db9286daf0b5350268ad0bc3625`, while the service
regression and state metadata still carried an older hash. The state/validators now bind to the
actual checked-in profile, and Part 34.3 bundle/asset/service regressions are explicit CI gates so a
future profile drift cannot bypass the normal Alpha workflow.

Warning triage from the second compile:

- First-party actionable warnings: the three `last_profile` unused-assignment warnings were removed.
- Pinned upstream Jcode warnings: profile-package-spec and unused/dead-code warnings are retained as
  upstream provenance; the Jcode release binary still built and passed ELF verification. VibeCoder
  does not rewrite pinned upstream Jcode merely to silence warnings.
- Node `--openssl-no-asm`: emitted by Node's upstream Android configure path; it is a performance
  warning, not a correctness failure, and the reviewed Android flow is not weakened to suppress it.
- Android SDK Manager deprecation and Gradle cache-save warnings: CI/tooling warnings, not source
  compilation failures. They remain visible rather than being hidden.
- Diagnostic JKS format warning: the stable diagnostic signing identity is preserved; no keystore
  format migration is performed as part of a compile-error repair.

This checkpoint does not claim that the third external Android CI compile has passed. It records
source repairs and local regression/audit evidence only; full Alpha APK and physical-device proof
remain evidence gates.

## Part 34.10.9 — real CI evidence + Node GYP graph verifier repair

The latest external CI run materially advanced the evidence boundary. The minimal diagnostic APK
built and passed APK verification, and the exact Jcode 0.73.0 Android ARM64 payload again passed ELF
and 16 KiB compatibility checks before the Jcode diagnostic APK built and passed APK verification.
Those two successful build paths are preserved unchanged in this repair.

The Node lane failed before compilation because the Part 34.10.8 generated-graph guard searched
`*.target.mk` for the source token `sources/android/cpufeatures/cpu-features.c`. Inspection of Node
v24.19.0's pinned GYP Make generator shows that compilable source paths are converted through
`Target()` to `.o` paths and the generated target recipe records those paths in `OBJS`. The guard was
therefore capable of rejecting a valid graph simply because it looked for a source filename that the
Make generator does not retain there.

The verifier now checks for `sources/android/cpufeatures/cpu-features.o`, requires that the matching
recipe declares `TARGET := zlib`, still requires the cpufeatures include path, and rejects either the
source or object token in host recipes. The regression fixture now mirrors the generated GYP Make
shape and also proves that an object appearing under a non-zlib target is rejected. The source-level
zlib cpufeatures patch itself is unchanged.

A separate source-capability audit corrects an earlier diagnostic-APK interpretation: the current
source already wires the Chat Send control into the JNI/Rust one-shot persisted model conversation,
and the core already contains the Jcode coding-action and explicit-loop controllers. What remains
false is automatic UI selection of the coding-action path. The downloaded Jcode diagnostic APK is
not the full Alpha package: it proves Jcode packaging/execution compatibility but does not package the
Node + OmniRoute runtime required by the real chat bootstrap. This checkpoint does not claim a Node
binary or full Alpha APK until the repaired verifier and downstream build pass external CI.

## Part 34.10.10 — conservative coding-action routing and Node cpufeatures path repair

The normal Chat composer now has two real backend routes behind the same Send control. A deterministic,
conservative classifier keeps general conversation and explanation-style coding questions on the existing
one-shot persisted model-conversation path. A clear project mutation request, for example “Add a Start
button to the home screen,” selects one persisted agent-action turn instead. This is routing, not an
autonomous outer loop: Part 34.9 loop mode remains explicitly opt-in.

The routed action path uses the exact model selected during bootstrap with an empty fallback list and the
existing before-response fallback boundary. The Android core now also owns an app-private
`LocalCheckpointStore`; this is required by the Part 34.8 action contract so a Jcode mutation remains
checkpointed/rollback-safe. Stop distinguishes a cancellable model turn from an active agent action and
uses the existing persisted conversation/Jcode cancellation contract for the latter. The JNI response
reports `turn_kind` plus mutation evidence for action turns, so Java/UI does not need to invent whether a
workspace change occurred.

The latest external CI before this source change again proved both the Minimal and Jcode diagnostic APK
lanes. The Node lane then progressed past the 34.10.9 generated-graph verifier and exposed a real Make
dependency failure: an absolute NDK cpufeatures source path became an impossible
`obj.target/zlib//usr/local/.../cpu-features.o` target. Part 34.10.10 copies the exact pinned-NDK
`cpu-features.c`/`.h` into the temporary Node tree at
`deps/zlib/vibecoder-android-cpufeatures/` and references that relative source from zlib GYP. The graph
verifier now requires the corresponding relative `.o`, rejects host leakage, and explicitly rejects the
absolute-object regression. No fake cpufeatures implementation is introduced.

This checkpoint is still source/regression evidence. Because the current audit runner has no Cargo/Rust
or Android SDK/NDK, the modified Android-host Rust has not been recompiled here. Minimal/Jcode must be
reconfirmed and Node must pass the new relative-source build in external CI before a full Alpha APK or
physical coding-action round trip is claimed.


## Part 34.10.11 — Android `renameat2` ABI repair + Node CI timeout extension

The first external compile after agent-action routing exposed one shared Android Rust compile error in
`vibecoder-checkpoint-local`: Android libc declares the `renameat2` flags argument as unsigned, while
`libc::RENAME_EXCHANGE` is typed as `i32` in the resolved libc crate. The call now casts only that
constant to `libc::c_uint`; atomic exchange semantics and the fail-closed rollback design are unchanged.
Both Minimal and Jcode lanes failed before APK packaging on this same workspace error, so this is not a
Jcode payload regression.

The Node lane independently proved the Part 34.10.10 relative cpufeatures graph: the expected relative
`cpu-features.o` was present, the absolute-NDK-object regression was absent, and no Node compiler or
linker error appeared before GitHub canceled the job at its 120-minute job timeout. The Node proof job
budget is therefore increased to 240 minutes while retaining `VIBECODER_BUILD_JOBS=2`; Node source,
cpufeatures staging, host/target split, and Jcode paths are otherwise unchanged.

This checkpoint does not claim post-fix APK or Node-binary success. External CI must reconfirm the
Minimal/Jcode lanes and allow the Node compile to finish before the full Alpha package can run.

## Part 34.10.12 — Node configure-time host architecture repair

The next external CI run reconfirmed both protected Android lanes after the Part 34.10.11 ABI repair:
the Minimal diagnostic APK built and passed APK verification, and the Jcode 0.73.0 Android ARM64
payload again passed its Android ELF/16 KiB checks before the Jcode diagnostic APK built and passed
verification. The Node lane also retained the repaired relative cpufeatures graph, so neither of those
previous blockers regressed.

The Node job did not time out this time. After roughly two hours of real host/target compilation it
failed because an `obj.host` V8 target was compiled by `/usr/bin/g++` from
`deps/v8/src/heap/base/asm/arm64/push_registers_asm.cc`. The x86_64 host assembler then rejected the
ARM64 `stp`/`blr`/`ldp` instructions. This is a host-architecture selection failure, not a reason to
compile host objects with the Android target compiler.

The root cause is fixed before GYP generation. `provision_node_android.sh` now binds `CC_host`,
`CXX_host`, and `AR_host` while invoking Node's `android-configure`, so Node's configure-time host
architecture detection sees the actual CI host compiler instead of falling back to the ARM64 NDK
compiler placed in `CC`/`CXX` by the Android configure wrapper. The configure-output verifier now
requires `host_arch=x64`, `target_arch=arm64`, and `want_separate_host_toolset=1`.

A new generated-graph guard runs immediately after GYP makefile materialization and before the
expensive Node compile. It requires the V8 host graph to select
`deps/v8/src/heap/base/asm/x64/push_registers_asm.o` and rejects ARM64 push-register sources or objects
from the host graph. This turns the observed two-hour-late architecture mix into an early fail-closed
configuration error if it ever regresses.

No agent-action routing, Minimal/Jcode build path, vendored Jcode source, Node cpufeatures patch, or
Node timeout budget is changed in this repair. The current environment still cannot perform the real
Node Android cross-build, so post-fix Node binary/full Alpha success remains an external CI evidence
gate rather than a source-level claim.

## Part 34.10.13 — idempotent clean-host sanitizer repair

The next external CI run proved the Part 34.10.12 configure-time architecture repair before any
expensive Node compilation: `host_arch=x64`, `target_arch=arm64`, the separate host toolset was
enabled, the generated V8 host graph selected only the x64 push-register object, and the repaired
relative cpufeatures graph remained verified. The Minimal and Jcode APK lanes also built and passed
APK verification again.

Node then failed **before compilation** in VibeCoder's own generated-host-makefile sanitizer. That
sanitzer historically required at least one `-mbranch-protection=standard` replacement because the
older misdetected ARM64 host graph leaked that target-only flag into `*.host.mk`. Once host
architecture detection was fixed, the x86_64 host graph was already clean, so zero replacements are
a valid state rather than evidence of a broken graph.

The sanitizer is now idempotent. It still removes only the one evidence-proven Android-only flag when
present, then rescans every host recipe and fails if that flag remains. If no host recipe contains it,
the sanitizer reports `sanitization_mode=already_clean` and succeeds without mutating anything. Every
`*.target.mk` is still SHA-256 guarded before/after and must remain byte-identical, so the no-op path
does not weaken Android target flags. Regression fixtures cover both the removal path and the clean
no-op path.

No Node source, cpufeatures patch, host-architecture repair, Minimal/Jcode build path, agent-action
routing, timeout budget, or vendored Jcode source is changed. A post-repair Node binary/full Alpha APK
remains an external CI evidence gate.

