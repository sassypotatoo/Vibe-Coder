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

## Part 34 — Durable multi-chat + single-turn Alpha controller

Status: **SOURCE SLICE COMPLETE; interactive APK runtime proof BLOCKED by missing Node/OmniRoute payloads**

- [x] Added a provider-neutral `ConversationId` domain identity.
- [x] Added bounded app-private persisted conversation state with dense user/assistant message order,
  per-conversation Jcode session binding, CAS revisions, session-creation crash markers, and
  turn-in-progress crash markers.
- [x] Added a separate `ConversationStore` contract so the legacy Part-16 single project-session
  registry remains backward compatible while one project can own multiple independent chats.
- [x] Extended the Unix/Android local persistence adapter with private `0600` atomic conversation
  files under `vibecoder/state/conversations`, canonical project+conversation UUID names,
  `O_NOFOLLOW` opens, owner/hard-link checks, bounded JSON reads, fail-closed directory parsing,
  per-project limits, list/remove, and project-wide cleanup.
- [x] Added Core APIs to create/list/load/resume persisted conversations and to cancel a running
  conversation turn.
- [x] Added `run_persisted_conversation_turn`: exactly one call commits one user message, executes
  at most one backend task, commits the assistant result, then stops. It does not autonomously
  re-prompt or loop.
- [x] A user message is committed with `turn_pending=true` before inference. Clean success or clean
  failure clears the marker; a process/app crash leaves an explicit recovery-required state rather
  than pretending the turn completed.
- [x] Conversation persistence schema contains no credential/secret fields, raw process authority,
  arbitrary roots, or fake model responses. User transcript text itself is persisted as sensitive
  user data; model execution still passes through the deterministic OmniRoute/Jcode boundary.
- [x] Existing Part-31 static contracts rechecked after the implementation; before checksum refresh,
  the only reported differences were the five intentionally modified source files.
- [ ] Full Rust compile/test of these Part-34 changes in this execution runner. Rust/Cargo 1.88 is
  not installed here and external toolchain download is unavailable from the runner network.
- [ ] First real Android conversational Alpha turn. The current source still lacks the reviewed
  OmniRoute 3.8.50 bundle and packaged Android Node 24.19.0 runtime, so Gateway readiness correctly
  remains false. No echo/fake assistant response is introduced to disguise that blocker.
- [ ] Physical-device restart test proving chat restoration after Android process death/app restart.
- [ ] Explicit interrupted-turn recovery UX. A crash marker currently fails closed and requires a
  later reviewed recovery action rather than being auto-cleared.

Next: package/prove Node + the reviewed deterministic OmniRoute runtime, wire the Android Alpha UI to
this conversation controller, then execute the real prompt -> model -> Jcode/tools -> response ->
persist -> restart proof. Only after that evidence should Part 34 be called device-verified Alpha.
- [x] Follow-up audit fixed Part-17 rollback integration: all Part-34 conversation Jcode sessions are
  now checked before rollback and force-refreshed after workspace replacement; pending/duplicate or
  runtime-mismatched conversation sessions block rollback fail-closed.
- [x] Turn session resume + durable pending commit now run under the project lifecycle gate; the
  backend task reacquires it, and rollback sees the pending marker if it wins the handoff window.
- [ ] Add uninstall-safe backup/export/restore. App-private projects/chats survive process death and
  normal restart, but Android deletes them on uninstall/Clear storage. External backup must remain
  data recovery, never live workspace authority.

## Part 34.2.1 — Android Node runtime foundation audit

- [x] Reused the existing Android runtime inventory/process/probe architecture instead of creating a
  second Node execution system.
- [x] Confirmed the pinned Node 24.19.0 source/hash and upstream Android ARM64 configure boundary.
- [x] Found the stale Node source-tree JNI staging path left behind by the Part-31 generated-output
  migration.
- [x] Found Node provisioning incorrectly coupled to npm/source-asset staging for this Node-only part.
- [x] Found missing Node ELF pre-package verification, APK evidence mode, device acceptance mode, and
  a validator blind spot for `provision_node_android.sh`.
- [ ] No Node binary was built, packaged, or device-proven in this audit-only step.

Next: Part 34.2.2 repairs the Node-only generated staging + validation lane. OmniRoute remains out of
scope until Node itself is package/device-proven.

## Part 34.2.2 — generated Node staging + validation lane repair

- [x] Moved Node output authority from source `src/main/jniLibs` to
  `android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so`.
- [x] Removed npm/source-asset staging from the Node-only provisioner; npm remains a separate later
  website-build capability.
- [x] Added fail-closed Android ELF verification before and after Node generated staging.
- [x] Extended APK verification with an isolated `node` mode while preserving minimal/Jcode behavior.
- [x] Added a dedicated Node APK packaging/evidence lane and Node-specific machine-readable evidence.
- [x] Closed the checkpoint-validator blind spot so Node source-tree staging/npm coupling now fails
  static validation.
- [ ] Node 24.19.0 Android cross-build executed successfully.
- [ ] APK containing the real Node candidate built and verified.
- [ ] Physical-device Node package/ARM64/exec/version/16-KiB proof.

Next: Part 34.2.3 performs the actual pinned Node Android cross-build, fixes the first real compiler/
linker failure if any, and feeds the resulting candidate through the new Node APK evidence lane.

## Part 34.2.3 — exact Node Android cross-build execution lane

- [x] Kept the existing project-wide Android NDK pin `28.2.13676358` instead of creating a Node-only
  toolchain split; NDK r28+ is the project contract for 16-KiB-aligned native output.
- [x] Added exact NDK revision, exact API 29, Python, build-job and source-hash preflight before Node configure.
- [x] Added durable configure/make logs so the first real compiler/linker failure remains diagnosable.
- [x] Added NDK ARM64 compiler-path preflight and post-`android-configure` generated-config checks so
  upstream wrapper exit status alone cannot create a false configure-success claim.
- [x] Added machine-readable Node cross-build evidence bound to source archive SHA-256, NDK revision,
  Android API, output SHA-256 and verified ELF64/AArch64/Bionic/16-KiB properties.
- [x] Bound APK packaging evidence to the exact cross-build evidence and Node output hash; arbitrary
  same-named prebuilts are rejected.
- [x] Added a dedicated GitHub Actions Node proof job that installs the pinned SDK/NDK, cross-builds
  Node 24.19.0, packages it, and uploads failure logs when configure/make fails.
- [x] Current runner preflight executed and failed closed before configure because no Android NDK is
  installed and external binary download is unavailable in this execution environment.
- [ ] Node 24.19.0 compiler/linker build completed in an NDK-capable runner.
- [ ] Real Node candidate APK packaging evidence produced.
- [ ] Physical-device Node package/ARM64/exec/version/16-KiB proof.

Next: execute the new Node proof job in an NDK-capable CI runner, inspect the first real Node
configure/compiler/linker result, apply only the smallest source/build fix if it fails, and repeat.
OmniRoute remains out of scope until the Node candidate itself is package/device-proven.


### Part 34.2.3 real execution follow-up — toolchain availability boundary

- [x] Executed the real Node cross-build wrapper in the current runner instead of stopping at static
  CI configuration.
- [x] Preserved machine-readable failure evidence and the exact execution log; the first failure is
  `android_ndk_root_missing`, classified as `toolchain_unavailable`.
- [x] Added a single execution-attempt authority used by CI and local runs so configure/compiler/
  linker failures are classified and retained without false success.
- [x] Added a fail-closed offline bootstrap for the exact pinned Linux NDK r28c archive for runners
  where `sdkmanager` cannot be used but the official archive is supplied out-of-band.
- [ ] Configure has not started because this runner has no Android NDK and cannot acquire external
  binary archives; therefore no Node compiler/linker result exists yet.
- [ ] Node binary, APK packaging evidence, and physical-device Node execution remain unproven.

Next acceptance event remains a real NDK-capable execution. The first configure/compiler/linker
failure from that run, if any, is the only authority for the next source/build fix.

## Part 34.2.4 — supervised Node runtime lifecycle source

Status: **SOURCE LIFECYCLE COMPLETE; REAL NODE BINARY/APK/DEVICE PROOF STILL PENDING**

- [x] Reused the hardened local process supervisor instead of creating a second child-process engine.
- [x] Added a separate trusted package-runtime service scope, distinct from user/project command
  authorization. Only executables registered from the package-owned runtime-tool registry may enter
  this path; there is no shell or PATH lookup.
- [x] Runtime services use app-private per-service working directories, a cleared environment,
  bounded stdout/stderr capture, timeout handling, explicit cancellation and process-group
  termination.
- [x] Only one process may own a runtime-service id at a time, preventing duplicate Node/OmniRoute
  service instances inside one app process.
- [x] Android child processes receive `PR_SET_PDEATHSIG(SIGKILL)` plus the parent-death race check,
  so an app-process death cannot intentionally leave a supervised Node child as stale runtime
  authority for the next app start.
- [x] Android host now exposes `start_node_runtime`, `node_runtime_active`, and
  `cancel_node_runtime`; Node remains package-path verified before supervised launch.
- [x] Device acceptance automation now has an isolated `node` mode requiring Node package presence,
  ARM64 identity, real execution, exact 24.19.0 version, and 16-KiB compatibility without falsely
  requiring OmniRoute/gateway readiness.
- [ ] Fresh Rust compile/test in this runner; Cargo/Rust 1.88 is unavailable here.
- [ ] Real Node 24.19.0 Android binary/APK/device execution proof; the pinned Android NDK cannot be
  downloaded or located in this runner.

Next: finish Node-specific Android diagnostic/device acceptance wiring, then the only remaining
Part-34.2 gate is the real pinned binary + APK + physical-device proof. OmniRoute remains Part 34.3.

## Part 34.3.1 — reviewed OmniRoute source admission + Android dependency audit

- [x] Located the exact reviewed `OmniRoute-release-v3.8.50.zip` external input and verified the
  project-pinned SHA-256 `1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7`.
- [x] Verified archive shape: 13,622 entries, no symlinks, no absolute paths, no `..` traversal, and
  no prebuilt `.next`/`.build`/`dist`/`node_modules` runtime authority.
- [x] Applied the existing exact-model VibeCoder patch to the reviewed archive and re-verified every
  patched target against its expected SHA-256.
- [x] Added fail-closed reviewed-source admission tooling and provenance metadata so future builds
  cannot substitute a same-named or structurally unsafe OmniRoute archive.
- [x] Audited Android-native dependency risk. `sharp`, sqlite-vec platform packages, onnxruntime-node,
  and wreq-js do not provide an Android ARM64 platform package in the reviewed lockfile;
  better-sqlite3 is optional.
- [x] Verified the reviewed source already has a viable native-free core direction: Node `node:sqlite`,
  `sql.js` WASM fallback, sqlite-vec -> FTS5 degradation, and lazy/unavailable handling for several
  feature-specific native modules.
- [ ] Build an Android-safe backend-only standalone runtime bundle.
- [ ] Generate and verify the runtime bundle manifest, package it as an APK asset, extract it into
  app-private runtime storage, and prove a loopback OmniRoute service round trip.

Next: Part 34.3.2 builds the explicit Android runtime profile and standalone bundle policy. It must
prune/reject host-native addons rather than shipping Linux binaries into the Android runtime.


## Part 34.3.2 — Android-safe backend runtime profile + bundle sealing lane

Status: **SOURCE LANE COMPLETE; REAL OMNIROUTE STANDALONE BUILD PENDING EXACT NODE 24.19.0 HOST**

- [x] Added an explicit Android backend runtime profile pinned to OmniRoute 3.8.50, the reviewed
  archive hash, the deterministic routing-patch profile, exact Node 24.19.0, loopback port 20128,
  upstream backend-only build mode, minimal privileged-feature build and sqlite-vec disablement.
- [x] Added a post-build Android bundle sealer that dereferences build-machine symlinks, prunes known
  unsupported native/feature packages and the TPROXY native subtree, then rejects any remaining host
  native `.node`/shared-library/executable or ELF/PE/Mach-O payload.
- [x] Added a deterministic runtime manifest with per-file SHA-256, file count, total bytes, tree
  SHA-256, reviewed-source/routing-patch authority, runtime launch profile and explicit feature
  degradations.
- [x] Added an independent bundle verifier that re-hashes the retained tree and rejects manifest
  drift, native bytes, symlinks, missing required runtime files and proof overclaims.
- [x] Added an end-to-end builder: reviewed source admission -> exact Node preflight -> `npm ci` ->
  `npm run build:backend` -> Android sealer -> independent verifier.
- [x] Synthetic known-native pruning test passed; synthetic unknown native addon was rejected.
- [x] Added source-symlink containment and regression coverage: internal links are materialized, while
  external/dangling build-machine symlinks, hash tampering and reintroduced forbidden packages fail closed.
- [x] Executed the real builder preflight in this runner. It failed closed because Node is 22.16.0
  while the VibeCoder build authority requires 24.19.0.
- [ ] Build the real reviewed OmniRoute standalone tree with Node 24.19.0 and the lockfile dependencies.
- [ ] Seal and independently verify the real production bundle.
- [ ] Package the verified bundle as an APK asset and prove app-private extraction/service startup.

Next: obtain the real Node-24-built standalone output, feed it through the now-complete Android bundle
sealer/verifier, then advance to APK asset extraction and OmniRoute service lifecycle wiring.

## Part 34.3.3 — OmniRoute generated APK asset + app-private installer

Status: **SOURCE LANE COMPLETE; REAL PRODUCTION BUNDLE/APK/DEVICE EXTRACTION PROOF PENDING**

- [x] Added a generated Android asset source set dedicated to verified OmniRoute production bundles;
  the reviewed source ZIP is not packaged.
- [x] Added a fail-closed asset stager that independently verifies the sealed bundle before and after
  staging, atomically replaces stale generated assets, and emits evidence without claiming APK/device proof.
- [x] Added Android app-private installation at `files/vibecoder/runtime/omniroute` with exact manifest
  identity/runtime checks, bounded paths/sizes, per-file SHA-256 checks and deterministic tree validation.
- [x] Added serialized installation, stale-stage cleanup, previous-runtime recovery, atomic promotion,
  post-commit verification and rollback to the previous verified runtime on failure.
- [x] Existing installed bundles are fully re-hashed before reuse; a receipt alone is not runtime authority.
- [x] Added APK `omniroute_asset` verification and physical-device asset-install acceptance modes while
  keeping OmniRoute service-readiness proof explicitly false.
- [x] Added regression tests for tampered input rejection, tracked-source output protection and stale
  generated-asset replacement.
- [ ] Real Node-24-built OmniRoute production bundle staged into generated APK assets.
- [ ] Real APK contains the independently verified OmniRoute asset bundle.
- [ ] Physical ARM64 device proves app-private OmniRoute extraction/verification.
- [ ] OmniRoute process start/health/shutdown round trip; this is Part 34.3.4.

Next source slice: Part 34.3.4 wires the already-defined trusted Node runtime supervisor to the verified
app-private OmniRoute entrypoint and adds loopback readiness/shutdown semantics. External Node/real-bundle
proofs remain mandatory before the runtime can be called production-ready.

## Part 34.3.4 — supervised OmniRoute service lifecycle source

Status: **SOURCE LANE COMPLETE; FRESH RUST COMPILE + REAL NODE/BUNDLE/APK/DEVICE ROUND TRIP PENDING**

- [x] Fixed the runtime-service lifetime bug: persistent internal services no longer inherit the
  normal command wall-clock timeout; project command timeout behavior is unchanged.
- [x] Added trusted runtime working-directory and bounded explicit-environment support after
  `env_clear()`, while rejecting PATH/loader/NODE_OPTIONS authority.
- [x] Added exact loopback OmniRoute launch profile: package-owned Node, `server-ws.mjs`, private
  DATA_DIR, production environment and port 20128 only.
- [x] Added Rust-side re-verification of the installed mutable runtime immediately before launch,
  bound to the signed APK asset manifest SHA-256 plus exact receipt/file/tree identities.
- [x] Readiness requires two consecutive exact runtime-profile attestations while the child remains
  active; process start or a matching log line alone is not readiness.
- [x] Startup failure cancels/reaps the child; explicit stop and explicit reverify/restart are
  implemented without an automatic restart loop.
- [x] Added process-global Rust FFI session plus JNI start/status/stop controls. Mutating JNI calls
  use one-shot bounded buffers so a size-query cannot execute start/stop twice.
- [x] Added Android `omniroute_service` diagnostic/device acceptance mode requiring real Node 24.19.0
  device proof, verified asset install, signed-manifest binding, loopback URL and runtime-profile
  attestation, followed by a live status check and explicit stop/reap proof in the same run.
- [x] Added source regression coverage for persistent-service/no-timeout, launch environment,
  re-verification/readiness, FFI/JNI and device acceptance contracts.
- [ ] Fresh Rust compile/test of this slice; Cargo/Rust is unavailable in the current runner.
- [ ] Real Node 24.19.0 Android binary + real Node-24-built OmniRoute bundle packaged into an APK.
- [ ] Physical ARM64 device OmniRoute start/readiness/stop round trip.

Next after external proof: Part 34.4 connects the VibeCoder gateway transport to this proven local
service. Until then `service_round_trip_proven=false` remains authoritative.

## Part 34.4 — Android-local OmniRoute gateway transport source

Status: **SOURCE LANE COMPLETE; PHYSICAL CATALOG ROUND TRIP + FIRST MODEL REQUEST PENDING**

- [x] Reused the existing hardened `vibecoder-gateway-omniroute` client instead of introducing a
  second HTTP implementation.
- [x] Added an Android-host transport bridge fixed to `http://127.0.0.1:20128/v1`; no caller URL,
  redirect, ambient proxy or remote plaintext fallback enters this path.
- [x] Gateway probing requires the supervised OmniRoute service to still be active and to match the
  exact Part 34.3 service/runtime attestation before network I/O.
- [x] Re-fetches the exact runtime-profile attestation through HTTP before credential-scoped
  `GET /v1/models` health/catalog discovery.
- [x] Added borrowed anonymous/ephemeral-bearer credential support capped at 8192 bytes. Credentials
  are not serialized, persisted, logged or returned in diagnostics.
- [x] Added sanitized transport evidence that distinguishes a reached-but-auth-required catalog from
  a transport failure without retaining raw server response bodies.
- [x] Added Android `INTERNET` socket permission without enabling a global cleartext/network-security
  exception; Rust still permits plaintext HTTP only for loopback.
- [x] Added one-shot Rust FFI + JNI gateway probe and an Android diagnostic/device
  `omniroute_gateway` acceptance mode. The probe happens before explicit service stop.
- [x] Part 34.4 explicitly records `inference_request_sent=false` and
  `first_model_request_proven=false`; no chat/responses request belongs in this step.
- [ ] Fresh Rust compile/test in this runner; Cargo/Rust remains unavailable.
- [ ] Real Node 24.19.0 + real OmniRoute bundle APK physical catalog round trip.
- [ ] Android secure-store-backed bearer provisioning.
- [ ] First actual model inference request; this is Part 34.5.

Next: Part 34.5 sends the first real, bounded model request through the proven local transport only
once the runtime/credential acceptance gates are available. No autonomous agent loop is introduced.

## Part 34.5 — first exact-model inference request source

Status: **SOURCE LANE COMPLETE; REAL ANDROID MODEL RESPONSE PENDING EXTERNAL RUNTIME PROOF**

- [x] Extended the provider-neutral gateway contract with bounded text chat request/response types
  and a one-shot completion operation.
- [x] Added exact `POST /v1/chat/completions` transport under the existing loopback-only,
  no-redirect/no-proxy, bounded-response HTTP client.
- [x] Added request bounds: exact model id, 1-64 text messages, required user message, 256 KiB total
  message content, and explicit 1-8192 output-token limit.
- [x] Added strict non-streaming response parsing, stable HTTP/error classification, token-usage
  extraction and rejection of tool/function calls before the Part 34.7 tool bridge.
- [x] Android inference proof re-attests the live service/profile, refreshes the usable model catalog,
  requires an exact model match, then issues exactly one inference request with no VibeCoder retry or
  alternate-model fallback.
- [x] Diagnostic evidence persists no prompt or assistant response text; only sanitized model,
  response-size, finish-reason and token-usage proof metadata are retained.
- [x] Added one-shot Rust FFI/JNI inference bridge and an `omniroute_inference` physical-device mode
  requiring service + transport + exactly-one-response proof before explicit service stop.
- [ ] Fresh Rust compile/test in this runner; Cargo/Rust remains unavailable.
- [ ] Real Node 24.19.0 + real OmniRoute production bundle APK on ARM64 device.
- [ ] Real first model response with `first_model_request_proven=true`.

Next: Part 34.6 connects this inference primitive to the durable single-turn conversation controller.

## Part 34.6 — durable conversation controller to exact OmniRoute model

Status: **SOURCE LANE COMPLETE; REAL ANDROID CONVERSATION TURN PENDING EXTERNAL RUNTIME PROOF**

- [x] Added a model-only durable conversation turn that keeps the existing Jcode agent turn path
  intact for the later Part 34.7 tool bridge.
- [x] User text and `turn_pending=true` are committed before any model network request.
- [x] Re-fetches deterministic runtime profile plus a fresh credential-scoped catalog and requires
  one unambiguous exact model id before inference.
- [x] Sends a bounded contiguous recent-history suffix: max 64 messages, 128 KiB/message and
  256 KiB total, always ending in the current user message and without a leading orphan assistant.
- [x] Issues exactly one non-streaming gateway completion with no VibeCoder retry, model fallback,
  Jcode tool execution or autonomous loop.
- [x] Rechecks requested/observed model identity and persisted-message size before committing the
  assistant response.
- [x] Failure clears the pending marker by CAS while preserving the already-durable user message;
  failed cleanup remains fail-closed for recovery.
- [x] Added secret-reference resolution entry point without persisting the resolved credential.
- [x] Fixed Android CI path coverage so changes under `crates/vibecoder-core/**` trigger validation.
- [ ] Fresh Rust compile/test in this runner; Cargo/Rust remains unavailable.
- [ ] Real Android Node 24.19.0 + OmniRoute runtime and one physical durable conversation response.

Next: Part 34.7 bridges the model turn to Jcode tools without changing the default one-turn stop
semantics. Tool calls remain forbidden in the Part 34.6 model-only path.

### Part 34.7 — OmniRoute ↔ Jcode model/tool bridge

- [x] Reviewed exact Jcode 0.73.0 OpenAI-compatible local transport contract.
- [x] Fixed Android private Jcode bridge to same-phone OmniRoute `127.0.0.1:20128/v1`.
- [x] Kept gateway upstream-provider identity separate from Jcode transport-provider identity.
- [x] Required exact model-id passthrough and fresh model corroboration before the turn.
- [x] Disabled Jcode provider fallback, auto-poke, autoreview, autojudge, telemetry, and ambient proxies.
- [x] Restricted Part 34.7 tools to Jcode minimal profile with `bash` explicitly disabled.
- [x] Reported file tools only when bridge mode and negotiated `session_files` are both present.
- [x] Kept command-tools capability false and SDK blanket permission auto-approve false.
- [x] Bounded one normal bridged turn to 32 tool starts; overflow cancels/fails closed.
- [ ] Real Android model-driven Jcode tool turn proof (blocked on packaged Node/OmniRoute runtime acceptance).
- [ ] Fresh Rust compile in current runner (Rust/Cargo unavailable).

### Part 34.8 — first bounded agent action turn source

Status: **SOURCE LANE COMPLETE; REAL ANDROID MODEL-DRIVEN WORKSPACE MUTATION PENDING EXTERNAL RUNTIME PROOF**

- [x] Added a dedicated persisted coding-action controller instead of treating every model answer as
  a successful action.
- [x] Requires bridged Jcode file-tool capability while command tools remain false.
- [x] Added a second live Jcode tool-policy gate: any unexpected `ToolStart` outside the exact
  reviewed file allowlist cancels/fails the turn in addition to the launch-time tool-profile lock.
- [x] Keeps one outer user turn only; the existing 32 inner tool-start cap remains authoritative.
- [x] Creates an immutable `BeforeAgentChange` checkpoint before model/tool activity and commits the
  user message + `turn_pending=true` before the backend task begins.
- [x] Corroborates live tool start/finish events against the final turn transcript and requires at
  least one successful file tool plus at least one successful mutating file tool.
- [x] Added an ephemeral post-action checkpoint and requires project-tree SHA-256 to differ from the
  pre-action tree before the final assistant response may be committed.
- [x] Backend/acceptance/persistence failures keep the pending marker armed while the whole project
  rolls back to the pre-action checkpoint, and clear it only after rollback succeeds. Rollback failure therefore stays fail-closed.
- [x] Temporary verification checkpoints are removed best-effort after success/recovery.
- [x] Added Part 34.8 source validator, fail-closed regression script and CI coverage.
- [ ] Fresh Rust compile/test in this runner; Cargo/Rust remains unavailable.
- [ ] Real Node 24.19.0 + sealed OmniRoute Android runtime + physical model-driven Jcode file mutation.
- [ ] Command execution/tooling; still intentionally disabled for this slice.
- [ ] Explicit multi-turn loop mode; Part 34.9.

Next: Part 34.9 adds explicit bounded user-requested looping on top of the now-bounded one-action turn.
It must never turn the default chat/action path into an autonomous loop.

### Part 34.10 — minimal honest mobile Alpha UI foundation

Status: **SOURCE UI + NORMAL CHAT BRIDGE WIRED; PREVIEW/AGENT UI ROUTING + PHYSICAL ACCEPTANCE PENDING**

- [x] Replaced the diagnostic-first activity surface with a portrait mobile VibeCoder shell while
  preserving the Part 31 runtime diagnostic harness behind the settings button.
- [x] Added mobile `Chat` / `Preview` tabs, a hamburger old-chat drawer, message composer, Send and
  visible Stop controls, and a compact light-theme phone layout based on the approved mockup.
- [x] The old-chat drawer reads existing app-private Rust conversation JSON read-only; it does not
  invent sample chats and does not mutate the Rust-owned conversation store.
- [x] Saved conversations render their real user/assistant messages with bounded file/message reads.
- [x] Added a real Preview navigation surface but kept it an explicit placeholder until the local
  preview runtime is connected; no fake browser URL, deployment, build or device preview is shown.
- [x] Initial 34.10 checkpoint kept Send fail-closed until a real JNI controller existed; Part 34.10.2 now supersedes that boundary with the durable normal-chat bridge.
- [x] Kept runtime diagnostics executing in the background for existing ADB proof automation.
- [x] Android controller JNI bridge for durable create/send/cancel normal-conversation actions; restored chats resume by persisted identity.
- [ ] Live local preview runtime and WebView/dev-server bridge.
- [ ] Fresh Android compile in this runner and physical UI acceptance.

#### Part 34.10.1 — deep UI / compile hardening audit

- [x] Strict Java 17 stub compile with `-Xlint:all -Werror`; fixed the OmniRoute installer FileLock warning.
- [x] Added CI strict-Java compile gate against the real pinned Android 36 `android.jar`.
- [x] Moved old-chat filesystem/JSON reads off the main thread and made Activity teardown race-safe.
- [x] Aligned Java's chat-file ceiling with Rust's 16 MiB persistence contract.
- [x] Sorts all safe chat candidates newest-first before applying the 50-chat display cap.
- [x] Hardened canonical UUID + filename/JSON identity validation and pending-session display checks.
- [x] Bounded large-chat TextView rendering without modifying persisted history.
- [x] Added Android 15+/target-36 system-bar/cutout/IME inset handling.
- [x] Added Android 13+ overlay-priority Back handling for the custom old-chat drawer.
- [x] Re-ran C JNI syntax, JSON/XML/YAML/Python/shell parsing, Cargo manifest/lock static consistency,
  and every Part 34.3–34.10 source/regression validator available in this runner.
- [ ] Full Gradle/AGP Android compile, NDK link, Cargo compile/test and physical-device UI proof; this
  runner still lacks the Android SDK/NDK, Gradle and Rust toolchains.

#### Part 34.10.2 — automatic OmniRoute runtime + real normal-chat UI bridge

Status: **SOURCE WIRED; REAL ANDROID/RUST/OMNIROUTE ROUND-TRIP PROOF PENDING**

- [x] App startup now installs/reuses the sealed OmniRoute APK asset automatically, starts/reuses
  the package-owned Node/OmniRoute service, and requires exact-model/no-hidden-reroute readiness.
- [x] End users do not manually install an OmniRoute runtime ZIP; missing/unverified APK assets stay
  fail-closed with the composer disabled. Provider/account authentication remains a separate model-
  availability concern and is not fabricated by the shell.
- [x] Added Rust Android app-controller FFI for bootstrap, durable New Chat, one normal model Send,
  and scoped Stop/cancel.
- [x] Wired mobile New Chat / Send / Stop through Java -> JNI -> Rust; no fake assistant bubble path remains.
- [x] Added cooperative cancellation around profile/catalog/inference network work with 50 ms bounded
  polling, while preserving a racing sent user message durably before cancellation cleanup.
- [x] Removed the extra uncancellable pre-send catalog call; the selected exact bootstrap model is
  fresh-verified again inside the cancellable core turn.
- [x] JNI chat JSON now decodes standard UTF-8 through Java's UTF-8 String constructor instead of
  passing arbitrary model bytes to modified-UTF-8 `NewStringUTF`.
- [x] Added active-chat selection clearing and recovery-state composer blocking so failed/pending
  restored chats cannot accidentally send into a previously selected hidden conversation.
- [x] Activity teardown requests real native cancellation and lets JNI/Rust durable cleanup finish
  instead of assuming Java thread interruption cancels native work.
- [x] Re-ran strict Java 17 warnings-as-errors against local Android stubs and Clang C/JNI signature
  compile with a generated NativeBridge header.
- [ ] Automatic UI intent routing into the already-built Jcode coding-action controller; current
  Send is intentionally the normal-conversation path only.
- [ ] Provider credential/setup UI when the fresh OmniRoute catalog exposes no usable model.
- [ ] Live Preview runtime.
- [ ] Fresh Cargo/Rust + Gradle/NDK compile and physical-device normal-chat round trip.

- Part 34.10.2 final hardening: fresh OmniRoute catalog selection now rejects model IDs that the durable conversation contract cannot accept, and app-open catalog readiness gets a bounded 4x/250ms startup-only warm-up retry (no inference retry/fallback).


#### Part 34.10.4 — first GitHub compile-log repair

Status: **SOURCE REPAIR COMPLETE; SECOND GITHUB COMPILE PENDING**

- [x] Analyzed the first real GitHub Actions failure logs rather than changing unrelated runtime code.
- [x] Fixed `vibecoder-core` using `tokio::time::timeout` without declaring Tokio as a direct dependency; aligned the local package dependency list in `Cargo.lock`.
- [x] Removed the compiler-reported unnecessary `mut` binding in the first agent-action turn.
- [x] Confirmed the Node Android failure was an `obj.host` V8 object being compiled by `aarch64-linux-android29-clang++`.
- [x] Kept the exact reviewed Node 24.19.0 source archive unmodified and split host GCC/G++ from Android NDK target Clang explicitly at make time.
- [x] Added generated-Makefile and build-log fail-closed verification so any future Android compiler use for `obj.host` is rejected distinctly.
- [x] Bound Node cross-build evidence to the host/target split verifier source hash.
- [x] Added a regression proving the observed old host-toolchain failure is rejected and a correct host/target split is accepted.
- [ ] Second real GitHub Actions compile to expose the next genuine compiler/linker result.
- [ ] Full Alpha APK build/package proof and physical-device acceptance.

#### Part 34.10.6 — second GitHub compile-log deep-audit repair

Status: **SECOND COMPILE FAILURES REPAIRED; NEXT ANDROID RECOMPILE PENDING**

- [x] Analyzed `logs_85916610910.zip` and separated first-party errors from upstream/tooling warnings.
- [x] Confirmed the strict Java 17 compile gate passed in the supplied CI evidence.
- [x] Confirmed pinned Jcode v0.73.0 built for Android ARM64 and passed ELF/16 KiB verification before the app-host Rust failure.
- [x] Imported `vibecoder_process_contract::ProcessRuntime` for OmniRoute service cancellation calls.
- [x] Removed the unused `last_profile` readiness assignments while retaining two consecutive full attestations.
- [x] Materialize Node v24.19.0 `out/Makefile`/GYP recipes before generated-makefile inspection or sanitization.
- [x] Added a fail-closed host-only sanitizer for the evidence-proven `-mbranch-protection=standard` leak into `obj.host` recipes.
- [x] Guard every generated `*.target.mk` hash against sanitizer mutation.
- [x] Preserve verbose Node build commands and host/target compiler log verification.
- [x] Repaired stale OmniRoute Android runtime-profile SHA-256 authority and added explicit Part 34.3 regression gates to CI.
- [x] Classified Jcode, Node configure, SDK-manager, Gradle cache and JKS messages instead of blindly mutating pinned dependencies/signing identity.
- [ ] Third external Android CI compile has not yet been observed in this checkpoint.
- [ ] Full Alpha APK package proof is not yet claimed.
- [ ] Physical-device Alpha acceptance is not yet claimed.

### Part 34.10.7 — Latest CI compiler/linker repair

- [x] Latest clean-root CI crossed source/checksum validation and the strict Java gate.
- [x] Jcode Android payload cross-build produced and verified an Android AArch64 payload before the workspace compile failed.
- [x] Repaired workspace Rust E0425 in `vibecoder-process-local`: runtime-service arguments now use `char::is_control` directly.
- [x] Removed the VibeCoder-owned `GatewayChatMessage` production unused-import warning by importing it only in tests.
- [x] Node 24.19.0 cross-build reached the Android linker with verified host/target toolchain separation (710 host compiles, 2099 target compiles).
- [x] Root Node linker failure identified: vendored zlib's Android CPU detector calls `android_getCpuFeatures`, while the NDK cpufeatures implementation was not linked into zlib.
- [x] Added a deterministic post-integrity Node zlib GYP patch that includes the pinned NDK `sources/android/cpufeatures/cpu-features.c` implementation and include directory.
- [x] Added fail-closed checks for the NDK cpufeatures source/header and regression coverage for patch ordering/double-application.
- [ ] Post-repair Android CI recompile still required; no APK or Node binary success is claimed by this checkpoint.

### Part 34.10.8 - deep source/compile-contract audit

- [x] Re-ran the full Part 31 + Part 34.3-34.10 source/regression suite against the 34.10.7 baseline.
- [x] Audited all 26 Rust workspace members for missing path dependencies, undeclared local-crate imports, missing modules/includes, feature mismatches, orphan callback helpers, and prior trait-import regression classes; no new source compile contract defect was found.
- [x] Added `verify_node_android_cpufeatures_integration.py` after GYP generation and before compilation. It requires exactly one zlib Android target makefile to contain the NDK `cpu-features.c` source and rejects any host-makefile leakage.
- [x] Added fail-closed regression coverage for missing target integration and host-graph leakage.
- [ ] Fresh Rust/Android compile is not claimed in the audit runner because Cargo/Rust and Android SDK/NDK are unavailable here; GitHub CI remains the external compiler proof gate.
