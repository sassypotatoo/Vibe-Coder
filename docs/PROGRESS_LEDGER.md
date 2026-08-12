# Development progress ledger

## Global workflow

- First 50% is split into 25 sequential parts.
- Each part uses: inspect -> smallest implementation -> source/static validation -> fix -> repeat.
- The first full compile was deferred through Part 24 and completed in Part 25 (50% milestone).
- UI implementation stays at the final stage.

## Part 1 — Foundation and integration boundaries

Status: COMPLETE.

- [x] Provider-neutral domain/agent/gateway/workspace contracts.
- [x] Core orchestration boundary.
- [x] Third-party provenance and MIT notices.
- [x] Claude recovered source excluded from dependencies/shipping.
- [x] UI-last/security/fail-closed invariants persisted.

## Part 2 — Jcode transport boundary and connection lifecycle

Status: COMPLETE.

- [x] Inspected pinned Jcode 0.73.0 SDK/harness public boundary.
- [x] Vendored only `jcode-sdk` + `jcode-harness-api`, not Jcode TUI/internal crates.
- [x] Added `vibecoder-agent-jcode` adapter crate.
- [x] Added persisted, secret-free connection configuration.
- [x] Added private SDK-owned runtime mode as the default.
- [x] Added shared-runtime mode with custom-socket/autostart split-brain guard.
- [x] Added explicit Disconnected/Connecting/Connected/Faulted lifecycle.
- [x] Added handshake-derived server identity and capability snapshot.
- [x] Added monotonically increasing connection generation.
- [x] Added stale/closed socket detection and retryable fault normalization.
- [x] Raw `JcodeClient` is kept private to the adapter boundary.
- [x] Provider/session/turn/model logic deliberately remains outside Part 2.
- [x] Full compile not run; Part 25 policy preserved.
- [x] Audit loop fixed nested-config serialization mismatch before checkpoint.
- [x] Audit loop serialized connect/disconnect/reconnect lifecycle to remove transport races.
- [x] Audit loop removed raw SDK/startup stderr from persisted failure state.
- [x] Audit loop made missing Jcode executable non-retryable and rejected empty path overrides.
- [x] Vendored SDK/harness subset verified byte-for-byte against the uploaded pinned Jcode source.
- [x] Final source validator passes after all fixes.

## Part 3 — Jcode session create/resume/cancel mapping

Status: COMPLETE.

- [x] Extended the provider-neutral agent contract with explicit safe session resume and active `ensure_ready()` negotiation.
- [x] Added `JcodeAgentRuntime` over the Part-2 connection manager.
- [x] Session create uses the canonical verified project root as Jcode working directory.
- [x] Session resume verifies persisted Jcode working-directory metadata before binding.
- [x] Added in-memory session -> project/root/connection-generation registry.
- [x] Serialized create/resume/cancel attachment changes on one Jcode connection.
- [x] Cancel refuses unknown/unbound sessions and reattaches known sessions after reconnect.
- [x] Existing session bindings cannot be silently rebound to another project id/root, including an upstream session-id collision.
- [x] Model override remains fail-closed until the dedicated model-selection part.
- [x] Upstream create/attach `working_dir: None` mismatch discovered; strict resume verification now uses `list_sessions()` and new-session creation corroborates metadata without racing a mid-write record.
- [x] Jcode session ids are validated against the upstream persisted-id format before resume/cancel.
- [x] Rust MSRV corrected from 1.85 to 1.88 because the vendored SDK uses stable let chains.
- [x] Fixed core preflight so it negotiates runtime readiness instead of misreading a disconnected adapter as unsupported.
- [x] Full compile not run; Part 25 policy preserved.



## Part 4 — Jcode turn execution and streaming result mapping

Status: COMPLETE.

- [x] Mapped real Jcode SDK `run()` into provider-neutral `run_turn`.
- [x] Require an already verified/bound session and reviewed `streaming` capability before sending a prompt.
- [x] Empty prompts and premature per-turn model overrides fail closed.
- [x] Long blocking SDK turns run on a dedicated worker instead of blocking the async orchestration executor.
- [x] One active turn per single attached Jcode connection; overlapping turns are rejected.
- [x] Cancel remains possible concurrently through a separate cloned SDK handle.
- [x] Disconnect/reconnect is rejected while an in-flight turn owns the transport.
- [x] Dropping an async turn future triggers best-effort upstream cancellation through an active-turn lease.
- [x] Assistant text, tool lifecycle, progress, status, usage, permission-request and completion events are normalized.
- [x] Provider/Jcode reasoning deltas are deliberately not exposed as application events or final results.
- [x] Final turn result preserves text, tool-call outputs/errors, usage and cancellation state.
- [x] Callback panics are contained so presentation/event consumers cannot unwind through the Jcode collector.
- [x] Audit loop blocked create/resume/second-turn attachment changes before they can steal an active turn's one-session bridge.
- [x] Audit loop pinned cancel to the active turn's original connection generation; cancel never reconnects/reattaches mid-turn.
- [x] Full compile not run; Part 25 policy preserved.


## Part 5 — Jcode permission and capability negotiation

Status: COMPLETE.

- [x] Reviewed pinned Jcode 0.73.0 bridge capability list and permission-response behavior.
- [x] `RuntimeCapabilities.permissions` now comes from the actual Jcode hello handshake.
- [x] Pinned bridge remains correctly reported as `permissions=false`; no fake capability is introduced.
- [x] Added an in-memory permission broker bound to session id + connection generation.
- [x] Added request-id/action/description validation and duplicate request rejection.
- [x] Added real `respond_to_permission` mapping for permission-capable compatible harnesses.
- [x] `AllowOnce` maps to upstream single-use `Allow`; `Deny` maps to upstream `Deny`.
- [x] `AllowSession` is implemented locally as an exact action+description grant and never sends upstream `AllowAlways`.
- [x] Double response, stale response, cross-session response, and post-turn response fail closed.
- [x] Pending permission state is cleared on normal turn completion and abandoned-turn cleanup.
- [x] Unexpected permission events from a runtime that did not advertise the capability are denied and the turn is best-effort cancelled.
- [x] Audit loop removed the turn-control/network-wait coupling so explicit cancel remains deliverable while a permission response is in flight.
- [x] Added a per-turn permission-request budget and control-character/size validation to bound malformed/flooded permission events.
- [x] Deep source audit caught a future Rust move error in permission-decision mapping (`PermissionDecision` is not `Copy`); mapping now matches by reference before the decision is consumed by broker completion.
- [x] Undeliverable permission prompts (no event consumer or callback panic) now fail closed with deny + cancel instead of hanging indefinitely.
- [x] Malformed/oversized permission request ids are never reflected into upstream deny responses; unsafe ids trigger cancellation without echoing untrusted protocol material.
- [x] Audit fixed an inherited connection-generation race: public `connect()` can no longer recover/create a new Jcode transport while a turn is active.
- [x] Full compile not run; Part 25 policy preserved.



## Part 6 — Jcode model discovery and selection mapping

Status: COMPLETE.

- [x] Fixed the provider-neutral contract so model discovery is session-scoped.
- [x] Exposed session model discovery/selection through `VibeCoderCore`; callers do not bypass the adapter boundary.
- [x] Reviewed Jcode 0.73.0 bridge behavior: model requests exist without a dedicated hello capability token.
- [x] Added operational model-capability verification bound to the current connection generation.
- [x] Added fresh Jcode `list_models` discovery with `get_runtime_info` provider-route corroboration.
- [x] Upstream audit found Jcode bridge model-cache state is not synchronously cleared on attachment changes, and an empty fresh model list does not overwrite a prior non-empty cache. Model-sensitive operations now use a fresh sidecar API connection to the same live socket, subscribe before target-session attach, and wait for fresh `ModelInfo` before reading the catalog.
- [x] Preserve exact upstream model ids; no alias normalization, prefix synthesis, or suffix stripping.
- [x] Reject malformed/duplicate upstream model ids instead of silently filtering them.
- [x] Added real `set_model` mapping with fresh-catalog authorization and post-switch runtime corroboration.
- [x] Caller-supplied provider labels must match unambiguous Jcode route/runtime metadata.
- [x] Model discovery/switching require a verified session/project binding and current transport generation.
- [x] Model changes are blocked while a turn is active.
- [x] `RunTurnOptions.model` safely selects the persistent session model before starting the turn.
- [x] `CreateSessionOptions.model` remains intentionally rejected because Jcode cannot make create+model atomic.

- [x] Deep audit rejected an unsafe first cache-reset design: reconnecting the manager-owned private Jcode client can drop its SDK-owned ephemeral `JCODE_HOME` and delete private runtime state. Replaced it with a fresh same-socket sidecar API client; the owner stays alive.
- [x] Sidecar-discovered catalogs authorize exact model ids, while the actual model mutation runs on the verified manager-owned connection that will execute the turn; the owner generation is rechecked after sidecar discovery.
- [x] Post-switch audit found reused owner runtime metadata could retain a stale provider field if an upstream model-change event omitted it. Model switches now receive independent post-switch corroboration from a second fresh sidecar before the operation/turn proceeds.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 7 adds the OmniRoute HTTP client boundary and strict URL/auth validation.

## Part 7 — OmniRoute HTTP client boundary and strict URL/auth validation

Status: COMPLETE.

- [x] Added a dedicated `vibecoder-gateway-omniroute` adapter crate; core/domain remain free of reqwest/url types.
- [x] Added strict absolute URL parsing and canonical `/v1/` API-root normalization.
- [x] Remote plain HTTP is rejected; loopback HTTP remains available for the upstream local default.
- [x] URL user-info, query/fragment, port 0, endpoint-specific paths and ambiguous duplicate path separators are rejected.
- [x] Added bounded request/connect timeout configuration and bounded incremental response collection.
- [x] Redirects are disabled so Bearer credentials cannot follow a server redirect to another target.
- [x] Ambient/system proxies are disabled; proxying cannot silently become an extra credential-bearing hop.
- [x] Historical Part 7 state: `api_key_env` was reference-only and never loaded there; Part 10 has now removed it from transport config.
- [x] Added ephemeral/redacted `RequestAuth::Bearer` with HTTP-header-shape validation and no serialization.
- [x] Added raw GET `/v1/models` transport seam for Part 8 without prematurely interpreting model/health semantics.
- [x] Reviewed upstream HEAD behavior and kept it availability-only; it never receives a VibeCoder Bearer token.
- [x] Raw reqwest error prose is not persisted; transport failures map to stable gateway error codes.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 8 maps authenticated OmniRoute health and model-catalog semantics.


## Part 8 — OmniRoute authenticated health and model-catalog mapping

Status: COMPLETE.

- [x] Changed provider-neutral gateway calls to accept a borrowed, Debug-redacted, non-serializable `GatewayCredential`; no plaintext credential is retained by core/client.
- [x] Implemented `ModelGateway` for `OmniRouteClient` using credential-scoped `GET /v1/models`.
- [x] Exposed gateway health and gateway model discovery through `VibeCoderCore` rather than forcing callers to bypass the adapter boundary.
- [x] `HEAD /v1/models` remains availability-only and is not accepted as authenticated health truth.
- [x] Added stable health states for missing/rejected auth, access denial, rate limiting, missing endpoint, invalid response, no usable models, and unavailability.
- [x] Successful health requires HTTP 200, JSON content type, valid `{object:"list",data:[...]}` shape, and at least one coding-usable model.
- [x] Maps only chat/responses-capable rows; specialty-only image/audio/embedding/rerank/moderation/video/music rows stay out of the coding catalog.
- [x] Multi-capability rows explicitly advertising `chat` or `responses` remain usable even when they carry a specialty `type`.
- [x] Preserves exact upstream model id, optional display name, and exact `owned_by` provider; no alias/provider guessing.
- [x] Rejects duplicate usable model ids and bounds catalog size, ids, providers, display names, types, and endpoint metadata.
- [x] Malformed JSON/envelope, empty bodies, non-JSON success responses, oversized responses, and bad status classes fail closed with stable codes.
- [x] Recorded the selected private architecture as Android phone-local-first: no mandatory cloud/remote agent or build server.
- [x] Explicitly did not claim OmniRoute Node/Jcode executable Android packaging is solved; those remain local-runtime bring-up blockers, not hidden server dependencies.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 9 adds the model route policy and fallback configuration model.


## Part 10 — strict config + secret references

- [x] Added strict bounded backend JSON loader.
- [x] Reject duplicate JSON keys, unknown fields, and plaintext secret field names.
- [x] Replaced `api_key_env` transport config with application-level `credential_ref`.
- [x] Added `SecretReference`, `SecretResolver`, `SecretValue`, dev environment resolver, and Android secure-store backend seam.
- [x] Secret values are Debug-redacted, non-serializable/non-cloneable, bounded, and zeroized on drop.
- [x] Core can resolve health/catalog/route-policy/preflight credentials per request.
- [ ] Android Keystore-backed platform adapter remains later Android integration work; not claimed complete.

Next: Part 11 workspace-root creation and canonical path containment.


## Part 11 — phone-local workspace root and canonical containment

Status: COMPLETE.

- [x] Added concrete `vibecoder-workspace-local` runtime for the phone-local architecture.
- [x] Platform supplies one existing absolute app-private directory; runtime canonicalizes it as the trust anchor.
- [x] Runtime creates/verifies fixed `vibecoder/projects` descendants and rejects symlink/non-directory substitutions.
- [x] Removed caller-controlled `WorkspaceSpec.root`; new-project specs generate a fresh private id and physical roots are derived only from that identity.
- [x] Added id-based project create/open plus exact serialized `ProjectRef` re-verification.
- [x] Added project-relative path resolution rejecting absolute paths, `..`, backslashes, oversized components, existing symlink components, non-directory intermediate components, and canonical escapes.
- [x] Core project-session start/resume now calls `verify_project` rather than trusting the stored root.
- [x] Core exposes managed project create/open/remove and relative-path resolution through the provider-neutral workspace boundary.
- [x] Added source-level tests for managed root creation/open, tampered refs, traversal/control characters, safe non-existent descendants, non-directory parents, fixed-root symlinks, and symlink-safe project deletion.
- [x] `read_write_files` remains false; Part 11 does not claim safe file I/O or process isolation.
- [x] Documented that resolved paths are not durable authorization tokens; Part 12 must enforce containment at operation time.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 12 adds safe file read/write primitives and atomic writes.

## Part 12 — safe file read/write primitives and atomic writes

Status: COMPLETE.

- [x] Added provider-neutral workspace methods for bounded reads, private directory creation, and atomic writes.
- [x] Added Unix/Android fd-relative traversal from app-private root through fixed managed descendants using `openat` + `O_NOFOLLOW`.
- [x] Re-verify `ProjectRef` and corroborate the opened project directory inode at operation time.
- [x] Added 16 MiB hard read/write ceilings plus caller-specific read limits.
- [x] Reject symlinks, non-regular read/write targets, owner-read-only write targets, and `st_nlink != 1` hard-link aliases.
- [x] Atomic writes use unique same-parent `0600`/owner-mode temp files, file `fsync`, `renameat`, and parent-directory `fsync`; no in-place truncation.
- [x] Preserve existing owner execute mode while stripping group/other mode on replacement.
- [x] New nested directories are created `0700` and verified after concurrent `EEXIST`.
- [x] Added source-level fixtures for round-trip I/O, atomic replacement/temp cleanup, limits, symlink parent/final rejection, hard-link rejection, missing parents, executable-mode preservation, and read-only targets.
- [x] Exposed safe file primitives through `VibeCoderCore`.
- [x] Explicitly did not claim Jcode built-in file-tool confinement or same-uid process isolation.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 13 adds file edit/patch and search primitives.


## Part 13 — safe file edit/patch and project search

Status: COMPLETE.

- [x] Added exact UTF-8 text edit requiring one unique match, including overlapping-match ambiguity detection.
- [x] Added bounded all-or-nothing multi-hunk patches; failed later hunks commit zero earlier hunks.
- [x] Re-check target inode, owner mode, and full contents immediately before atomic patch rename.
- [x] Reuse Part 12 private temp inode, hard-link defense, fsync, rename, and parent-sync rules.
- [x] Added fd-based Android/Linux deterministic project file discovery without symlink following.
- [x] Skip hard-linked/special/internal-temp/unrepresentable entries and never expose absolute app-private paths.
- [x] Added bounded literal UTF-8 search with file/result/byte/depth/walk budgets and bounded previews.
- [x] Exposed edit/patch/list/search through the provider-neutral workspace contract and `VibeCoderCore`.
- [x] Kept Jcode built-in tool confinement and shell/process isolation explicitly unresolved.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 14 adds the command request model, allow/deny policy, and execution envelope.

## Part 14 — command request policy and execution envelope

Status: COMPLETE.

- [x] Added `vibecoder-command-policy` with structured runtime-tool/workspace-executable command requests.
- [x] Added bounded argv/path/session validation and blocked generic absolute/path-traversal command authority.
- [x] Added explicit runtime-tool allowlist plus separately gated workspace executables; deny-all remains the default.
- [x] Every eligible command requires allow-once/deny; no persistent/automatic command grant exists in Part 14.
- [x] Pending approvals are session+project bound, duplicate/flood bounded, collision-safe, and revocable on session cleanup. Core corroborates that session/project pair with the managed workspace and current Jcode binding before request and again before allow-once.
- [x] `allow_once` echo-checks the returned approval payload against the broker-retained command; denial can still clear a correctly scoped tampered display without granting authority.
- [x] Added private non-cloneable/non-serializable `CommandExecutionEnvelope` with runtime-managed-clean environment policy and no caller stdin/shell-string mode.
- [x] Added command argument Debug redaction and explicit common-shell runtime-id rejection.
- [x] Integrated request/decision/revoke entry points into `VibeCoderCore` while preserving a deny-all default constructor.
- [x] Kept real execution, output capture, cancellation, timeout enforcement, executable re-resolution, and process isolation for Part 15.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 15 adds process lifecycle, cancellation, output capture, timeout enforcement, and operation-time executable resolution.

## Part 15 — local process lifecycle, cancellation, timeout, and bounded output

Status: COMPLETE.

- [x] Added provider-neutral `vibecoder-process-contract` and Unix/Android `vibecoder-process-local` runtime.
- [x] Part 14 envelopes are consumed by ownership and converted to private-field authorized command material; there is no inverse/public constructor.
- [x] Added explicit runtime-tool registry relative to fixed app-private `vibecoder/runtime`; no ambient PATH lookup authority.
- [x] Reconstruct/reverify managed project root, working directory, and executable immediately before spawn.
- [x] Reject symlink/special/hard-linked/non-owner-executable targets and retain structured argv only.
- [x] Child uses `env_clear`, runtime-owned HOME/TMPDIR, null stdin, and separate piped stdout/stderr.
- [x] Added Unix process-group setup, explicit cancellation, timeout, TERM-to-KILL escalation, and stable termination classification.
- [x] Added bounded nonblocking output capture, a bounded live-event queue, truncation/overflow reporting, and output-redacted Debug formatting.
- [x] Added bounded active-process registry (4 global, 2/project) and collision-safe process ids.
- [x] Core keeps execution fail-closed until a process runtime is explicitly attached and rechecks workspace + current Jcode session binding at execution time.
- [x] Post-spawn pipe/supervisor setup failure terminates and waits the child rather than knowingly orphaning it.
- [x] Strong kernel process isolation, network restriction, argument sandboxing, Jcode built-in command-tool confinement, and actual Android runtime packaging remain explicitly unresolved.
- [x] Full compile not run; Part 25 policy preserved.
- [x] Final seal audit caught and removed an accidental duplicate `Debug` derive that would have been a Rust compile blocker; the validator now guards against it.

Then: Part 16 adds the project/session persistence model.

## Part 16 — Project/session persistence
- [x] Added provider-neutral `ProjectStateStore` and app-private local persistence adapter.
- [x] Project state stores ProjectId only; physical workspace roots are never persisted as authority.
- [x] Persisted Jcode session ids are re-corroborated through `resume_session` after restart.
- [x] Added bounded per-project JSON records under fixed app-private state directories.
- [x] Added revision compare-and-swap updates to reject stale/lost-update writes.
- [x] Added temp+fsync+renameat+parent-fsync crash-safe file replacement.
- [x] Added operation-time O_NOFOLLOW directory re-entry and symlink/hard-link/wrong-owner rejection.
- [x] Added pre-create session pending marker so crashes do not make half-created sessions look absent.
- [x] Persisted exact model preference and explicit route policy without persisting resolved model authority.
- [x] Persisted schema contains no plaintext secrets, prompts, reasoning, tool/process output, command approvals, or absolute project roots.
- [x] UI remains deferred and full compile remains reserved for Part 25.

Next: Part 17 adds checkpoint/snapshot metadata and rollback.

## Part 17 — Checkpoint/snapshot metadata and rollback

Status: complete in source/static form; full compilation remains locked to Part 25.

Added provider-neutral checkpoint metadata/capabilities plus an Android/Linux app-private snapshot store. Checkpoint trees are immutable and SHA-256 verified with source/copy/source corroboration; symlinks, hard links, special files, unsafe names, and oversized trees fail closed. Rollback stages a clone and atomically exchanges it with the live project using `RENAME_EXCHANGE`, verifies the restored tree, and exchange-backs on verification failure.

Core now blocks checkpoint/rollback with active project processes or an active Jcode turn, and a project-scoped lifecycle permit closes the start-process/direct-mutation/removal/session race around replacement. Rollback advances the command-authorization epoch both before replacement and after commit, reopens the workspace afterward, and force-refreshes a persisted Jcode session. The audit also fixed a false source/copy/source claim by adding a real pre-copy source digest, and removed ambiguous post-commit cleanup errors. Same-UID malicious-process isolation remains explicitly deferred.

Next: Part 18 adds the build-job abstraction and normalized build result.


## Part 18 — Build job abstraction and normalized result

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added provider-neutral `vibecoder-build-contract`.
- [x] Added fresh build ids and explicit website/Android targets.
- [x] Added queued/running/succeeded/failed/cancelled/timed-out lifecycle semantics.
- [x] Wrapped Part 15 bounded process events/results without creating new execution authority.
- [x] Kept stdout/stderr contents Debug-redacted and non-persisted at the build layer.
- [x] Added bounded diagnostic and artifact-candidate metadata containers.
- [x] Artifact paths are project-relative and optional recorded SHA-256 metadata is strictly lowercase when present; byte verification is deferred.
- [x] Exit code zero does not imply artifact existence or integrity.
- [x] Toolchain detection, package-manager selection, website pipeline, error parsing, and Android pipeline remain later parts.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 19 adds website toolchain detection and package-manager abstraction.

## Part 19 — Website toolchain detection and package-manager abstraction

Implemented a read-only website analyzer over the safe workspace boundary. It detects supported
lockfiles/package managers, rejects conflicting manager evidence, classifies common web frameworks,
records bounded advisory `engines.node` metadata, and emits a semantic build intent without
executing or exposing package-script bodies. Static HTML projects require no package manager.
Runtime tool ids are fixed logical ids only; Part 20 remains responsible for approved execution and
trusted runtime resolution.

Next: Part 20 implements the website build pipeline state machine.


## Part 20 — Website build pipeline state machine

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added move-only `vibecoder-web-build-pipeline` with explicit install/build stages and terminal failure/cancel/timeout states.
- [x] Each stage emits an exact structured package-manager command but still requires Part-14 allow-once approval.
- [x] Locked npm/pnpm/Yarn/Bun install intents use frozen/immutable semantics; unlocked dependency installs fail closed.
- [x] Dependency install lifecycle scripts are disabled by default.
- [x] Part-19 toolchain reports now fingerprint exact package.json and selected lockfile bytes.
- [x] Toolchain detection now uses targeted root metadata probes rather than recursive project listing, so node_modules growth cannot break post-install reinspection.
- [x] Core re-inspects before approval and rechecks again under the project lifecycle gate with Jcode quiescent immediately before start.
- [x] Authorized envelope command must exactly match the current pipeline-stage command.
- [x] Stage execution reuses the Part-18 normalized BuildJob and Part-15 bounded process lifecycle.
- [x] Process success does not claim a verified web artifact.
- [x] General engines.node/version corroboration and Android ARM64 Node/package-manager runtime proof remain unresolved.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 21 captures build errors and orchestrates the first agent repair turn.


## Part 21 — Build-error capture and first agent repair turn

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added authority-free `vibecoder-build-repair` with bounded failed-build evidence capture.
- [x] Only terminal `Failed` builds are repair eligible; cancelled/timed-out/successful builds fail closed.
- [x] Evidence strips terminal/bidi noise, redacts common credential-bearing lines and absolute-path-shaped tokens, and stays bounded.
- [x] Added deterministic SHA-256 failure fingerprint excluding BuildId for Part-22 repeated-error detection.
- [x] Core freshly verifies project/session scope and requires zero active controlled processes plus Jcode quiescence.
- [x] Stale command approvals are revoked and a `BeforeBuildRepair` checkpoint is mandatory before mutation.
- [x] Exactly one repair turn runs while the same-project lifecycle permit remains held.
- [x] Repair prompt labels build evidence as untrusted data and tells the agent not to rebuild inside the turn.
- [x] Raw build output remains non-persisted; Debug output redacts repair evidence/prompt/assistant text.
- [x] Retry/rebuild/rollback loop policy remains deferred to Part 22.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 22 adds retry budgets, repeated-error guards, and cancellation across the repair/rebuild loop.

## Part 22 — Loop guards: retry budgets, repeated-error detection, cancellation

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added authority-free `vibecoder-build-loop` guard/state machine.
- [x] Default repair budget is 3 attempts; policy is hard-bounded to 1–8.
- [x] Exact Part-21 failure fingerprints drive consecutive repeated-error detection.
- [x] Default repeated-error stop occurs on the second identical fingerprint before another repair turn.
- [x] Repair and rebuild permits are move-only and scoped to one loop/project/attempt.
- [x] Cancelled/timed-out/successful builds terminate rather than consume repair attempts.
- [x] Repair-turn cancellation terminates the loop and cannot authorize a rebuild.
- [x] Rebuild preparation performs a fresh website inspection/pipeline and still requires Part-14 allow-once approval per stage.
- [x] Loop cancellation invalidates project command approvals and can cancel the active Jcode repair turn or guarded website process through existing runtime cancellation boundaries.
- [x] Loop state/evidence is transient and not persisted.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 23 integrates the end-to-end backend task state machine and deterministic OmniRoute/Jcode model identity boundary.

## Part 23 — End-to-end backend task and exact model corroboration

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added authority-free `vibecoder-task-orchestration` state and event observer.
- [x] Bound every task to a fresh task id, exact project/session, resolved policy, and move-only route attempt.
- [x] Added bounded non-empty prompts without retaining prompt contents in task state.
- [x] Added monotonic assistant/background/tool progress observation and no replay after observable progress.
- [x] Added provider-neutral gateway execution-profile attestation contract.
- [x] Completed the hash-pinned OmniRoute 3.8.50 exact-model runtime patch across all audited mutation paths.
- [x] Added the strictly pinned `/v1/vibecoder/runtime-profile` adapter boundary.
- [x] Added fresh Jcode catalog id/provider matching plus a separate active-model/provider corroboration API.
- [x] Kept a second fresh Jcode selection/corroboration inside `run_turn` immediately before inference.
- [x] Added Core prompt -> event/tools -> result orchestration under one project lifecycle permit.
- [x] Revoked command approvals before and after every backend turn attempt.
- [x] Kept prose-backed agent errors `Unknown`; no string-based transient-failure guessing.
- [x] Kept task prompt, assistant text, and tool output out of Debug/persistence state.
- [x] Verified the OmniRoute patch applicator against pristine original 3.8.50 bytes and its idempotent complete-patch path.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 24 adds source-level integration fixtures and failure-path contract tests.

## Part 24 — Static integration fixtures and failure-path contracts

Status: COMPLETE in source/static form; full compilation remains locked to Part 25.

- [x] Added strict raw-response fixtures for the pinned OmniRoute runtime-profile interpreter.
- [x] Bound every audited hidden model-reroute path to concrete deterministic patch guards.
- [x] Added task-state fixtures for catalog/active identity, progress, fallback, unsafe failure, and cancellation transitions.
- [x] Added provider-neutral Core integration fixtures for success and terminal failure paths.
- [x] Added a Core harness with fake agent/gateway/workspace/process adapters and zero external authority.
- [x] Required gateway/Jcode id or provider mismatches to stop before inference.
- [x] Required hidden-reroute attestation failure to stop before gateway catalog use.
- [x] Exercised only the explicit configured fallback when the primary is missing and the attempt is pristine.
- [x] Covered terminal cancellation and prose-backed agent errors without fallback guessing.
- [x] Added stale pre-turn envelope and during-turn pending-approval invalidation contracts.
- [x] Corrected the shared active-process guard from a checkpoint-specific error to the provider-neutral process boundary.
- [x] Added fixture schema/coverage/source-wiring validation while preserving every prior checkpoint check.
- [x] Full compile not run; Part 25 policy preserved.

Next: Part 25 performs the precompile audit, dependency/toolchain readiness checks, and first full compile/test fix loop.

## Part 25 — First full compile and compile-fix loop

Status: COMPLETE at the 50% milestone.

- [x] Installed and pinned the task-local Rust 1.88.0 toolchain required by the edition-2024/Jcode source boundary.
- [x] Resolved all 24 workspace members and generated the root `Cargo.lock` with 224 package records (24 workspace packages plus 200 dependency records).
- [x] Fixed the Core repair-loop cancellation return-type mismatch found by the compiler.
- [x] Added the two missing direct test dependencies exposed by full target compilation.
- [x] Canonicalized project-root command working directories to `.` so approval and execution equality remain exact.
- [x] Kept the special-file rejection test meaningful under the runner by using a FIFO after Unix-socket creation was denied before product code ran.
- [x] Applied workspace formatting and completed warning-denied Clippy validation across all workspace targets.
- [x] Passed all 124 root-workspace tests from an empty external target directory with `--locked`.
- [x] Executed the Part-24 Rust fixtures/contracts as part of that passing workspace run.
- [x] Passed 43 separately pinned Jcode public-crate tests without modifying vendored source.
- [x] Recorded two Jcode Unix-socket lifecycle tests as environment-blocked, not passed: the runner returned `EPERM` while binding their fixture sockets.
- [x] Preserved phone-local-first architecture and UI-last policy; Android cross-compilation, packaged runtime execution, and production UI are not claimed complete.

Next: Part 26 begins the post-50% Android ARM64 runtime-packaging inventory and on-device execution-readiness boundary.

## Part 26 — Android ARM64 runtime packaging/readiness boundary

- Part 26 source/static validation completed; this runner has no `rustc`/`cargo`, so these edits are not claimed as a new compiled baseline.

- Audited the Part-25 host-only runtime assumptions before adding Android work.
- Confirmed and repaired the invalid Android assumption that native runtime executables could live
  under writable app-private `vibecoder/runtime`.
- Split writable runtime data from a distinct platform package-installed executable-code root.
- Added the versioned `config/android-runtime-inventory.json` ARM64 component inventory.
- Added `vibecoder-runtime-packaging` with fail-closed package, ARM64, execution, version,
  Unix-socket, and 16 KB compatibility proof states.
- Kept physical-device execution and Android build-toolchain readiness explicitly false until real
  Android evidence is supplied.
- Production UI remains deferred.

## Part 27 — Android host integration and packaged-runtime probes

- [x] Audited Part 26 before extending it and found real Android integration gaps instead of adding UI.
- [x] Added `vibecoder-android-host` as the first `cdylib`-producing Android host boundary.
- [x] Corrected the logical core artifact to `libvibecoder_android_host.so`.
- [x] Removed Android Jcode PATH fallback by binding the private runtime to an explicit package executable.
- [x] Separated JNI/native-library placement from the filesystem root used for child `execve()`.
- [x] Added bounded ELF64/AArch64 and 16 KiB `PT_LOAD` structural probes.
- [x] Added Android-only bounded native executable/version probes; non-Android execution remains `NotRun`.
- [x] Added Jcode private-runtime/API-socket round-trip probing.
- [x] Added fixed trusted interpreter argv prefixes to the local process runtime.
- [x] Kept npm direct execution disabled and added a separate runtime-binding proof requirement.
- [x] Added a service-round-trip proof requirement for OmniRoute so asset presence is not gateway readiness.
- [ ] Android ARM64 cross-compile proof.
- [ ] APK packaging proof.
- [ ] Physical Android device execution proof.
- [ ] Packaged Jcode/Node/OmniRoute payload proof.

Next: Part 28 adds the minimal Android shell/APK packaging and pins/provisions the first real ARM64 runtime payloads.
- [x] Replaced reader-thread joins in Android version probes with nonblocking bounded drains so descendant-held pipes cannot defeat the timeout.

## Part 28 — Minimal Android shell and pinned payload provisioning

- Added an ARM64-only Android diagnostic application shell under `android/`.
- Pinned shell build metadata to compile/target SDK 36, AGP 9.3.0, Gradle 9.5.0, NDK 28.2.13676358,
  and build-tools 36.0.0; this is build-host metadata, not the future in-app Android build runtime.
- Added a JNI bridge plus bounded C ABI JSON snapshot from `vibecoder-android-host`.
- Fixed missing-Jcode behavior so absent payloads remain explicit readiness blockers instead of
  aborting the complete host snapshot.
- Added pinned Node 24.19.0 source provisioning and reviewed-archive verification for Jcode/OmniRoute.
- Added verified Gradle-wrapper bootstrap and Android-shell build scripts.
- Kept the UI deliberately diagnostic; production UI remains deferred.
- Runner lacks Gradle/Android SDK/NDK and Rust/Cargo, so APK compilation and device installation are
  explicitly unproven rather than reported as passed.

## Part 29 — Exact Jcode Android source/build boundary and reproducible APK CI

- [x] Re-proved Jcode `v0.73.0` against upstream and pinned the Android runtime build to commit
  `44ffa55281fad71c02be984c0674d92412210452` rather than an ambiguous moving branch/archive name.
- [x] Kept the historical reviewed `jcode-master.zip` digest only as provenance for the already-vendored
  SDK/harness seam; it is no longer runtime executable build authority.
- [x] Rejected the generic `aarch64-unknown-linux-gnu`/Termux release as an Android Bionic payload substitute.
- [x] Added exact-commit/clean-checkout/source-version verification and re-verification of the vendored
  Jcode SDK/harness manifest against the source that will actually be compiled.
- [x] Added a dedicated NDK `aarch64-linux-android` Jcode build/staging path with 16 KiB linker flags.
- [x] Added fail-closed ELF `PT_INTERP` verification: Android `/system/bin/linker64` or static PIE is
  accepted for packaged executables; a glibc loader identity is rejected before execution.
- [x] Added current GitHub Actions jobs for a minimal diagnostic APK and a separate exact-Jcode Android
  cross-compile + APK proof path, so the first installable diagnostic shell is not blocked by Jcode porting.
- [ ] Jcode Android cross-compile proof in this runner; Rust/Cargo + Android NDK are absent.
- [ ] Minimal APK compile proof in this runner; Android SDK/Gradle + Rust are absent.
- [ ] Physical Android install/run proof.
- [ ] Packaged Jcode private-session Unix-socket handshake proof.

Next: Part 30 must consume a real Android/CI build result, install the diagnostic APK, and keep fixing the
first real runtime failure until the Jcode package/version/socket handshake is proven. Node/OmniRoute do
not advance ahead of that proof.

## Part 30 — APK verification and physical-device proof harness

- Removed the bundle-only JNI packaging DSL property from the APK path; the shell keeps only `useLegacyPackaging = true` for extracted package-owned native files.

- [x] Re-audited Part 29 and retried a verified Android command-line-tools bootstrap before accepting the local runner limitation.
- [x] Added APK signature, 16 KiB zip-alignment, ARM64-only package, required-library, and per-ELF verification.
- [x] Advanced the minimal Android shell to Part 30/version 0.30.0.
- [x] Added atomic app-private diagnostic-result persistence for machine verification.
- [x] Added an adb device harness that installs/launches the APK and reads the result through `run-as`.
- [x] Minimal device acceptance requires ARM64/API 29+, Rust host load/probe, and Core READY.
- [x] Jcode device acceptance additionally requires Agent READY plus package/ELF/exec/version/16KiB/socket proof all PASSED.
- [x] Wired APK verification into both GitHub Actions build lanes.
- [ ] Local-runner APK compile: blocked because Android SDK/NDK, Gradle and Rust/Cargo are absent and verified binary download failed.
- [ ] Physical Android install/run proof: no adb device is attached to this runner.
- [ ] Jcode Android private-session socket handshake proof: requires the real Jcode APK on a physical Android device.

Next: execute the minimal CI/device lane, fix the first real build/device failure, then repeat until Jcode Agent readiness is proven before beginning Node/OmniRoute.


## Part 31 — First APK build/evidence lane

- Re-audited Part 30 before new work.
- Fixed the CI command-line-tools provenance mismatch by pinning build 15859902 in setup-android.
- Added a one-command minimal/Jcode APK build + verifier lane.
- Added machine-readable APK/native/source/tool evidence output.
- Advanced the diagnostic report schema marker to Part 31.
- Physical-device proof remains intentionally unclaimed until a real ARM64 Android device runs the harness.

- [x] Repaired generated JNI payload staging so Android-host/Jcode binaries live under `app/build/generated/jniLibs`, not the checksummed source tree.

- [x] Pinned a diagnostic-only debug signing identity so successive CI/test APKs can update the same installation without signature drift.

## Part 31 review-fix pass — external review verification

- Re-audited the external Part-31 report against source instead of applying severity labels blindly.
- Rejected the incorrect `darwin-aarch64` NDK host-tag recommendation and froze the reviewed `darwin-x86_64` mapping.
- Removed duplicate Jcode version execution before socket handshake.
- Added expected/observed runtime version evidence and deterministic exact/range matching.
- Added explicit pinned/unpinned runtime requirement state so placeholder Android build components fail with a dedicated readiness blocker.
- Wired Java APK-asset presence evidence into a backwards-compatible Rust FFI v2 diagnostic path.
- Cached the Rust-host dynamic-library boundary in `JNI_OnLoad`.
- Added an explicit Android Tokio executor for future synchronous JNI → async core/agent/gateway calls.
- Documented Gradle bootstrap prerequisites and process cancellation grace behavior.
- No APK compile or physical-device success is claimed by this review-fix pass.
