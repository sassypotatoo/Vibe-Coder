# VibeCoder Core — Part 31 reviewed-fixes checkpoint

Backend-first source for the private VibeCoder application. Production UI remains intentionally absent until the final stage.

## Completed so far

- Part 1: provider-neutral domain/contracts, core boundary, provenance and security invariants.
- Part 2: real Jcode SDK transport boundary and connection lifecycle.
- Part 3: safe Jcode session create/resume/cancel mapping with project-root verification.
- Part 4: real Jcode turn execution, streaming event normalization, cancellation coordination, and final turn-result mapping.
- Part 5: handshake-derived permission capability mapping plus a session/generation-bound permission broker.
- Part 6: session-scoped Jcode model discovery, provider corroboration, and verified model switching.
- Part 7: strict OmniRoute HTTP client boundary, URL/auth validation, redirect/proxy hardening, and bounded raw responses.
- Part 8: credential-scoped OmniRoute health, strict `/v1/models` parsing, and chat/responses-only model mapping.
- Part 9: deterministic primary/fallback model-route policy with exact catalog resolution and pre-response-only fallback safety.
- Part 10: strict persisted config loading plus reference-only, redacted, zeroizing secret resolution for phone-local operation.
- Part 11: concrete phone-local managed workspace roots plus canonical project/path containment.
- Part 12: Unix/Android operation-time safe file reads, private directory creation, hard-link defense, and atomic replacement writes.
- Part 13: exact/all-or-nothing text edit+patch primitives plus bounded fd-based project file discovery and literal text search.
- Part 14: structured command requests, fail-closed allow/deny policy, and non-serializable allow-once execution envelopes.
- Part 15: real phone-local process lifecycle with trusted executable resolution, cancellation, timeout, and bounded output capture.
- Part 16: app-private project/session persistence with revision-guarded atomic records and restart re-corroboration.
- Part 17: immutable project snapshots with SHA-256 integrity metadata and atomic Android/Linux rollback.
- Part 18: normalized website/Android build-job identity, lifecycle, bounded output/result, diagnostics, and artifact-candidate metadata model.
- Part 19: read-only website toolchain/package-manager/framework detection and semantic build intent.
- Part 20: approved website dependency-install/build pipeline with manifest+lockfile drift binding and normalized build-stage lifecycle.
- Part 21: bounded build-failure evidence capture, deterministic failure fingerprint, pre-repair checkpoint, and exactly one Jcode repair turn.
- Part 22: bounded repair/rebuild loop guards with retry budget, repeated-error stop, cancellation, and fresh rebuild preparation.
- Part 23: end-to-end backend task state, runtime-attested exact gateway routing, and independent Jcode model/provider corroboration before inference.
- Part 24: data-driven failure fixtures and Rust contract tests for gateway attestation, exact model identity, task transitions, cancellation, replay guards, and stale authority.
- Part 25: pinned Rust 1.88 toolchain readiness, a committed workspace lockfile, the first full compile/fix loop, 124 passing workspace tests, formatting verification, and warning-denied Clippy validation.
- Part 26: Android ARM64 runtime inventory, fail-closed device proof states, 16 KB native-code readiness tracking, and a W^X-safe split between writable runtime data and package-installed executable code.
- Part 27: UI-free Android host `cdylib`, explicit Jcode/Node package paths, ELF/16-KB/device probes, interpreted-tool argv binding, and separate JNI vs child-executable roots.
- Part 28: ARM64-only Android diagnostic shell, JNI→Rust readiness snapshot, pinned shell toolchain metadata, Node source provisioning, and reviewed Jcode/OmniRoute archive staging boundaries.
- Part 29: exact Jcode v0.73.0 commit provenance, Android/Bionic cross-build staging, dynamic-loader identity rejection, and reproducible minimal/Jcode diagnostic APK CI jobs.
- Part 30: APK signature/16-KB/ARM64 verification plus an ADB device-proof harness requiring Core readiness and the real Jcode private-socket round trip.
- Part 31: reproducible first-APK build/evidence lanes, generated-native-output isolation, stable diagnostic signing, and explicit Android command-line-tools pinning.
- Part 31 review pass: verified external-review fixes for version evidence, asset evidence, async execution boundary, JNI host caching, Gradle diagnostics, and cancellation-grace documentation.

The private product target is phone-local-first: no mandatory remote agent/build server. Local runtime processes may communicate over Android loopback/IPC; remote AI model APIs remain network services. See `docs/ANDROID_LOCAL_FIRST.md`.

The Jcode adapter vendors only the public `jcode-sdk` + `jcode-harness-api` seam from the pinned MIT-licensed upstream source. OmniRoute remains an MIT-licensed HTTP runtime boundary rather than being copied into this source tree. For the private Option-A target, that runtime is intended to run locally on the same Android phone. The recovered Claude Code tree remains reference-only and is neither shipped nor depended on.

## Part 9 routing policy

`vibecoder-routing` resolves an explicit primary + ordered fallback list against a fresh gateway catalog. It never invents aliases or chooses an unconfigured model. Automatic fallback is limited to configured transient failure classes and is forbidden once assistant output or tool activity has started. See `docs/MODEL_ROUTING_POLICY.md`.

Part 23 turns the resolved policy into execution authority only after the running bundled OmniRoute reports the exact pinned profile. The complete source patch rejects aliases, hook/guardrail/task/web/reasoning model changes, auto/combo routing, connection defaults, background/effort rewrites, family fallbacks, and emergency fallback. Core then independently matches the selected exact id+provider in a fresh Jcode session catalog, selects it, corroborates the active identity through another fresh probe, and asks `run_turn` to repeat selection/verification immediately before inference.

## Part 8 gateway truth

`vibecoder-gateway-omniroute` accepts HTTPS remote API roots and loopback-only HTTP. It rejects URL credentials/query/fragment ambiguity, disables redirects and ambient proxies, bounds response bodies, and never stores a Bearer value in the client. authentication references now live outside the OmniRoute transport config. The client stores no secret reference or secret value; Part 10 resolves a persisted `credential_ref` only for the duration of a request.

The reviewed OmniRoute 3.8.50 `HEAD /v1/models` handler remains availability-only. Real health now comes from credential-scoped `GET /v1/models`: a 200 JSON list with at least one chat/responses-capable model. 401, 403, 404, 429, 5xx, malformed JSON, bad content type, empty/non-chat-only catalogs, oversized bodies, and transport failures are mapped to stable fail-closed states/codes without copying raw server error prose.

The Part-28 minimal Android diagnostic shell is source-defined. Part 29 now pins the Jcode Android runtime to exact upstream commit `44ffa55281fad71c02be984c0674d92412210452`, rejects generic Linux/glibc ARM64 payload identity, and adds reproducible CI build jobs. APK compilation and physical-device Android runtime execution remain unproven in this runner. Strong process isolation and production UI remain later work. Part 26 now defines the Android ARM64 packaging/readiness boundary and removes the invalid writable-app-home executable assumption; it still does not claim successful on-device execution. Part 17 adds real project snapshots and rollback without turning checkpoint ids or digests into filesystem authority.

## Part 10 config and secrets

Persisted JSON is bounded, rejects duplicate object keys, unknown schema fields, and common plaintext credential field names. The checked-in phone-local example stores only `credential_ref: { source: "app_secure_store", name: "omniroute.api_key" }`. Actual secret bytes are represented by a non-serializable/non-cloneable `SecretValue`, Debug-redacted, and zeroized on drop.

`AppSecureStoreBackend` is the platform seam for Android Keystore-protected app-private storage. The Android implementation itself is intentionally not claimed complete yet. `EnvironmentSecretResolver` exists only for explicit development/testing and refuses secure-store references rather than silently falling back.

## Validation

Run the source-only checkpoint validator:

```bash
python3 scripts/validate_checkpoint.py
```

Part 29 contains `android/`, exact Jcode Android source/build verification, and `.github/workflows/android-diagnostic-apk.yml`. The workflow has an independent minimal diagnostic-APK job plus a stricter Jcode Android cross-compile/APK job. This runner still cannot claim an APK was built because its Android/Rust build toolchains are absent.

Part 25 completed the first full compile at the 50% milestone. The full workspace validation commands are recorded in `docs/PART25_COMPILE_AUDIT.md`.

## Part 11 workspace containment

`vibecoder-workspace-local` accepts one existing platform-supplied app-private directory and creates only `vibecoder/projects/<ProjectId>` descendants beneath its canonical root. Project creation/re-open is id-based; models/callers cannot choose arbitrary physical roots. Project refs are reverified before agent session use, and project-relative paths reject absolute traversal, existing symlink components, and canonical escapes. `read_write_files` is now true on Unix/Android because Part 12 added operation-time fd-relative file primitives. This does not yet confine Jcode built-in tools or future shell processes. See `docs/WORKSPACE_CONTAINMENT.md` and `docs/SAFE_FILE_IO.md`.

## Part 12 safe file I/O

The local workspace walks from a freshly verified project directory handle using `openat` + `O_NOFOLLOW`, rejects non-regular/symlink/hard-linked read targets, bounds reads/writes to 16 MiB, creates private directories, and replaces files through a same-directory temporary inode + `fsync` + `renameat` rather than truncating an existing inode. No insecure non-Unix fallback is advertised. Jcode tool confinement and process isolation remain later work.


## Part 13 edit and search

The workspace now exposes exact single-edit and bounded multi-hunk atomic patch APIs. Ambiguous or missing expected text commits nothing, and the target is rechecked immediately before rename. Android/Linux project discovery walks directory descriptors without following symlinks, returns only project-relative paths, and feeds a bounded literal UTF-8 search. Binary/oversized/unsafe aliases are skipped. These are workspace primitives only; Jcode built-in tool confinement remains later work. See `docs/PROJECT_EDIT_SEARCH.md`.


## Part 14 command policy

`vibecoder-command-policy` accepts only structured program + argv + project-relative working-directory requests. Runtime tools must come from an explicit trusted-tool allowlist, workspace executables must be explicitly enabled, and every eligible command requires an allow-once/deny decision. Common shell runtime ids are forbidden, caller environment/stdin are not part of the contract, and approved execution envelopes are private, non-cloneable, and non-serializable. No process is spawned in Part 14; see `docs/COMMAND_POLICY.md`.


## Part 15 process execution

`vibecoder-process-contract` and `vibecoder-process-local` consume Part 14 allow-once envelopes to start real local processes. Runtime tools come only from an explicit registry whose native executables resolve beneath a distinct package-installed code root; there is no ambient PATH authority. The child gets a clean environment, null stdin, bounded stdout/stderr capture, timeout/cancellation supervision, and its own Unix process group. Core re-verifies the managed project and current Jcode session binding immediately before start. Strong kernel isolation and Jcode built-in command-tool confinement remain unresolved; Part 26 defines Android runtime packaging evidence but physical-device execution is still unproven. See `docs/PROCESS_EXECUTION.md`.


## Part 16 project/session persistence

`vibecoder-persistence-contract` defines the narrow versioned state schema and revision-CAS store boundary. `vibecoder-persistence-local` stores one bounded record per project under the fixed app-private VibeCoder state root using no-follow fd traversal and atomic replacement. Project roots are re-derived from `ProjectId`, persisted Jcode session ids are re-corroborated on resume, and model/route data remains preference/configuration rather than live authority. See `docs/PROJECT_SESSION_PERSISTENCE.md`.

## Part 17 checkpoint + rollback

Part 17 stores checkpoints outside agent-visible project roots under the app-private `vibecoder/checkpoints` tree. A snapshot is published only when a pre-copy live-source digest, copied tree digest, fresh digest of the copy, and post-copy live-source digest all agree. Symlinks, hard-linked regular files, special files, control-character path components, and internal temporary namespaces fail closed rather than becoming incomplete restore points.

Rollback first clones the immutable checkpoint into a reserved project sibling, then Android/Linux uses `renameat2(..., RENAME_EXCHANGE)` to atomically exchange the staged tree with the live project name. There is no unsafe multi-rename fallback. A same-project lifecycle permit prevents process startup, direct workspace mutation, project removal, or session creation/resume from crossing the replacement window; already-active project processes or an active Jcode turn also block checkpoint/rollback. Rollback advances the command-authorization epoch before replacement and again after commit, then forcibly reattaches/corroborates any persisted Jcode session after directory identity changes.

This is not strong same-UID process isolation. An independently malicious process with the app UID remains outside Part 17 guarantees; later isolation/orchestration work must close that boundary.


## Part 18 normalized build jobs

`vibecoder-build-contract` wraps one already-authorized Part 15 process with a fresh build identity and stable `queued/running/succeeded/failed/cancelled/timed_out` semantics. Timeout and cancellation remain distinct. Raw stdout/stderr stays bounded and Debug-redacted; it is not persisted by this layer. Artifact metadata is bounded and project-relative, and artifact metadata may carry a lowercase SHA-256 recorded by a later discovery layer, but Part 18 itself does not verify artifact bytes. Exit code 0 alone never proves that an APK or website bundle exists. See `docs/BUILD_JOBS.md`.


## Part 19 website toolchains

`vibecoder-web-toolchain` safely inspects project metadata through `WorkspaceRuntime`, detects npm/pnpm/Yarn/Bun evidence and common web frameworks, rejects conflicting lockfiles, and returns a semantic build intent without executing or exposing package scripts. See `docs/WEBSITE_TOOLCHAIN.md`.


## Part 20 website build pipeline

`vibecoder-web-build-pipeline` converts the Part-19 report into exact structured package-manager stages. Each stage still requires a normal Part-14 allow-once approval and starts only through the Part-15 trusted runtime registry. Locked installs use package-manager immutable/frozen modes; unlocked dependency installation is fail-closed in Part 20. Install lifecycle scripts are disabled by default. Before approval and again under the project lifecycle gate immediately before spawn, Core re-inspects package metadata and requires the exact package.json and selected lockfile SHA-256 fingerprints to match. A successful build process is not yet a verified website artifact; artifact discovery remains later work.

## Part 21 build repair turn

`vibecoder-build-repair` converts only terminal `Failed` build results into bounded sanitized evidence. It strips terminal control noise, redacts common credential-bearing lines and absolute-path tokens, caps diagnostics/evidence/prompt size, and computes a deterministic SHA-256 failure fingerprint that excludes the build id so Part 22 can detect repeated failures across rebuilds. Raw process output remains transient and is not persisted by this layer.

Core's `run_first_build_repair_turn` requires matching project scope, no active controlled project process, a quiescent/corroborated Jcode session, and a configured checkpoint store. It invalidates stale command approvals, creates a `BeforeBuildRepair` checkpoint, then runs exactly one repair turn while the same-project lifecycle permit remains held. The prompt marks build evidence as untrusted data and explicitly tells the agent not to run another build; retry/rebuild policy belongs to Part 22.
## Part 22 repair/rebuild loop guards

`vibecoder-build-loop` adds authority-free retry accounting, exact repeated-failure detection, and cooperative cancellation around Part 21. The default policy allows three repair attempts and stops on the second consecutive identical Part-21 fingerprint; both limits are hard-bounded. Move-only repair/rebuild permits prevent replay inside one loop. After a non-cancelled repair, Core prepares a fresh Part-20 website pipeline, and every dependency/build stage still requires Part-14 allow-once approval. Dependency-install success is not treated as whole-build success. Loop state is transient and is not persisted as durable authorization. See `docs/BUILD_REPAIR_LOOP.md`.

## Part 23 backend task orchestration

`vibecoder-task-orchestration` is an authority-free prompt-to-result state machine. It binds one task to a project/session and freshly resolved route, requires Jcode catalog and active-model corroboration, observes normalized text/background/tool events, and permanently blocks automatic replay after observable output or tool activity. Core holds the same-project lifecycle permit for the whole task, requires zero controlled processes and a quiescent verified session, revokes command approvals around every turn, and returns a Debug-redacted outcome. Prose-backed agent errors remain `Unknown` and never trigger fallback. See `docs/BACKEND_TASK_ORCHESTRATION.md`.

## Part 24 integration contracts

Part 24 adds versioned JSON fixtures consumed by the OmniRoute adapter tests, the authority-free task-state tests, and a provider-neutral Core integration harness. The cases reject corrupt or unpinned runtime-profile responses, enumerate every audited hidden reroute, require exact gateway/Jcode model+provider identity, exercise configured fallback and terminal cancellation, prove observable progress blocks replay, and verify command authority from before or during a turn is stale afterward. Part 25 executed these Rust tests as part of the clean full workspace run. See `docs/PART24_CONTRACT_FIXTURES.md`.

## Part 25 first full compile

The workspace now has a generated `Cargo.lock` and builds with Rust/Cargo 1.88.0. A clean `cargo test --workspace --all-targets --locked` run passed all 124 root-workspace tests, `cargo fmt --all -- --check` passed, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. The separately pinned Jcode public crates passed 43 tests; two additional Unix-socket lifecycle tests were environment-blocked because this runner denied socket binding. No vendored source was changed to bypass that restriction. See `docs/PART25_COMPILE_AUDIT.md`.


## Part 26 Android ARM64 runtime boundary

Android runtime code and writable runtime data now have separate roots. The versioned inventory in
`config/android-runtime-inventory.json` cannot mark native code as writable app data, and
`vibecoder-runtime-packaging` requires explicit package/ARM64/exec/version/socket/16-KB evidence as
applicable. `LocalProcessRuntime` resolves registered runtime tools only from a distinct
package-installed executable root. No physical-device success is claimed by this checkpoint.


## Part 27 Android host and probes

`vibecoder-android-host` now provides the first `cdylib` Android boundary and binds Jcode/Node to
package-owned executable paths instead of PATH or writable app data. Native ARM64/16-KB structural
probes and Android-only version execution probes are implemented; Jcode also requires a private
API-socket round trip. npm remains data interpreted by Node and needs a runtime-binding proof, while
OmniRoute needs a service round trip. The JNI native-library root and child-executable filesystem
root are distinct inputs because native libraries are not guaranteed to be extracted as ordinary
files. No Android cross-compile, APK or physical-device success is claimed yet. See
`docs/PART27_ANDROID_HOST_PROBES.md`.


## AI Studio / repository handoff guardrails

This checkpoint is a real backend/runtime baseline, not a UI mock. When importing it into AI Studio or another coding agent:

- Preserve the existing `crates/`, `android/`, `scripts/`, `config/`, `third_party/`, root `Cargo.toml`, `Cargo.lock`, `PROJECT_STATE.json`, and validation/provenance files.
- Do not replace real Rust implementations, validators, build scripts, Jcode fetch/build logic, ELF checks, or APK verification with stubs, unconditional `true` results, `touch`-generated APKs, or simulated evidence.
- Do not create a second standalone root Android `app/` project as a replacement for `android/app`. The current Android diagnostic shell remains the integration host until the backend/device proof gate is complete.
- Before and after a deliberate source change, run `python3 scripts/validate_checkpoint.py`. Update `CHECKSUMS.sha256` only for intentional reviewed source changes; never use checksum regeneration to conceal contract failures.
- GitHub Actions invokes repository shell scripts through `bash`, so an AI Studio/GitHub import losing executable mode bits must not break the build. Python utilities are invoked through `python3` for the same reason.
- First proof gate: build the minimal diagnostic APK, install it on an ARM64 Android device, prove the Rust host/Core readiness, then build/test the exact Jcode Android/Bionic lane.
