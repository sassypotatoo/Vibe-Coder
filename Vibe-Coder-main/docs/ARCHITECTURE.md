# Architecture — Part 1

## Decisions

### 1. Backend first; UI last
No production UI code belongs in the first 50% unless a minimal temporary test surface becomes
necessary. The Android UI remains separate from the backend/runtime boundaries, but the selected
private-product architecture is phone-local-first: core, agent, gateway, workspace, and build
runtimes are intended to run on the same Android device, potentially in separate local processes.
No mandatory remote agent/build server is assumed.

### 2. Jcode is an adapter, not the domain model
The inspected Jcode v0.73.0 source has a deliberately stable, versioned harness API and a Rust SDK.
That is the intended integration point. VibeCoder must not import Jcode TUI types or internal
`jcode-protocol` messages into application/domain code.

Expected later adapter mapping:

- `create_session` -> `AgentRuntime::create_session`
- `run`/turn streaming -> `AgentRuntime::run_turn`
- `respond_to_permission` -> `AgentRuntime::respond_to_permission`
- `list_models` -> `AgentRuntime::list_models`
- `set_model` -> `AgentRuntime::set_model`
- cancel -> `AgentRuntime::cancel`

Capability negotiation is mandatory because the Jcode SDK explicitly documents that clients must
check advertised capabilities before depending on them.

### 3. OmniRoute is an HTTP gateway service
The inspected OmniRoute v3.8.50 source exposes OpenAI-compatible `POST /v1/chat/completions` and
`GET /v1/models` surfaces. VibeCoder treats it as a replaceable service boundary. Provider secrets
must not leak into domain objects or logs.

The application keeps a separate `ModelGateway` contract for health/model discovery. Inference may
flow Jcode -> OmniRoute directly once the adapter is wired, avoiding a wasteful proxy-through-core
hop.

### 4. Workspace execution is independent
An agent being allowed to request a shell command is not the same thing as that command being safe
to run. Project filesystem/process isolation, resource limits, snapshots, and build environments
belong behind `WorkspaceRuntime`. Later parts will implement these controls before autonomous builds
are trusted.

### 5. Claude Code recovered source is reference-only
It is not a dependency, is not vendored, and must not be copied into VibeCoder. The clean foundation
uses the MIT-licensed Jcode and OmniRoute projects as upstreams.

## Dependency direction

`vibecoder-domain` has no dependency on any runtime implementation. Contract crates depend only on
the domain. `vibecoder-core` depends on the contracts. Future adapters depend on contracts plus their
specific upstream/client libraries. No adapter types are allowed to flow back into the core.

## Part 2 addition — Jcode transport ownership

`vibecoder-agent-jcode` now owns the Jcode SDK boundary. The raw `JcodeClient` is not exported.
Connection configuration/state are VibeCoder-owned types and the upstream SDK remains behind them.
Only `jcode-sdk` and `jcode-harness-api` are vendored, minimizing coupling to Jcode internals.

The default runtime mode is private/SDK-owned. Session-to-project working-directory mapping remains
out of scope until Part 3 so workspace containment cannot be bypassed by transport configuration.

## Part 3 addition — session attachment is a project-security boundary

`vibecoder-agent-jcode::JcodeAgentRuntime` now implements the session subset of `AgentRuntime`.
The provider-neutral contract gained `resume_session(project, session_id)` because safely resuming a
persisted agent cannot be represented as a bare transport attach: the runtime must prove that the
session still belongs to the project requested by the application.

Jcode's reviewed attach reply identifies the session but currently omits its working directory. The
adapter therefore corroborates the id with the public `list_sessions()` metadata and verifies the
canonical working directory before storing an in-memory binding. Unverified attachments are closed
fail-closed. A connection generation change invalidates the old attached-state assumption and forces
reattachment before a stateful operation such as cancel.

## Part 4 addition — turns are a concurrent transport operation

Jcode's synchronous SDK `run()` is executed on a dedicated blocking worker. The adapter keeps a separate cloned SDK handle for cancellation and an in-memory one-turn registry because the reviewed bridge attaches one session per connection. A turn-control gate serializes cancellation against normal completion/drop cleanup. The worker marks itself finished before delivering its result to the async caller, so a late cancel cannot silently target an already-finished model turn. Session create/resume and transport reconnect/disconnect are rejected while a turn is active, before any operation can change the bridge attachment. Cancellation is pinned to the original connection generation and never reconnects or reattaches underneath the turn.

Streaming is capability-gated from the Jcode handshake. Provider/private reasoning events are deliberately ignored; VibeCoder retains only user-visible assistant text, tool lifecycle/results, status/progress, usage, and other application-safe events.

The pinned Jcode 0.73.0 bridge advertises `streaming` but not `permissions`. Part 4 therefore refuses a permission-capable newer/shared bridge until Part 5, and additionally auto-denies + cancels an unexpected permission request if a runtime violates its advertised capability contract.

## Part 5 addition — permission prompts are capability-gated and generation-bound

The adapter now derives `RuntimeCapabilities.permissions` directly from the connected Jcode
handshake. The pinned 0.73.0 bridge truthfully reports this capability as absent, so VibeCoder does
not invent an interactive approval flow for that runtime.

When a compatible server advertises `permissions`, permission events enter an in-memory broker bound
to the verified session and exact connection generation that emitted them. Responses after reconnect,
turn completion, duplicate request ids, or another session fail closed.

VibeCoder's `AllowSession` decision is intentionally narrower than Jcode's `AllowAlways`. Because the
reviewed API does not define how long or how broadly `AllowAlways` persists, VibeCoder never sends it.
Instead, it records an in-memory exact `(session, action, description)` grant and answers each future
matching prompt with another upstream single-use `Allow`. Workspace/process isolation remains a
separate mandatory boundary, especially for the pinned bridge that emits no permission prompts.


## Part 6 addition — model selection is operationally verified per transport generation

Jcode's model catalog is session-scoped. The provider-neutral `AgentRuntime::list_models` therefore
accepts a `SessionId`; a global catalog would be semantically wrong and could present one session's
routes as available to another.

The reviewed 0.73.0 bridge exposes `list_models`, `get_runtime_info`, and `set_model`, but its hello
capability array contains no dedicated `model_selection` token. VibeCoder does not invent one. The
adapter reports `RuntimeCapabilities.model_selection=true` only after a real catalog request succeeds
on the current connection generation. A reconnect invalidates that verified state automatically.

Discovery preserves exact upstream model ids and uses runtime route metadata only to corroborate a
provider when exactly one available route maps that id. Ambiguous provider identity remains `None`
rather than being guessed. Selection first verifies the requested id against a fresh catalog, sends
Jcode's `set_model`, then corroborates the active session/model (and provider when requested) with
`get_runtime_info`. Model changes are rejected while a turn is active. A `RunTurnOptions.model`
override means "select this persistent session model before starting the turn"; it is not a temporary
per-request route.

Jcode does not atomically accept a model during `create_session`, so `CreateSessionOptions.model` is
rejected instead of creating a session and then potentially hiding an orphan if the subsequent model
switch fails. Callers create the session first, then discover/select its model.


### Part 6 cache-race hardening

The reviewed Jcode bridge keeps model catalog state per API connection and starts an asynchronous
model probe on attach without first synchronously emptying the previous catalog. Its cache updater also
does not overwrite a prior non-empty model list when a fresh catalog contains an empty model list, and
empty route/model/provider fields may be omitted. Waiting for `ModelInfo` on a reused connection is
therefore not a complete cache reset.

VibeCoder uses a stricter model-operation path without restarting the owner runtime: open a fresh
sidecar API connection to the same live Jcode socket, subscribe to target-session events before attach,
wait for that session's fresh `ModelInfo`, then query/authorize the model catalog. Each sidecar gets a
fresh server-side `BridgeState`, so a previous session's catalog cannot become the target session's
authorization set even when the target catalog is empty. The manager-owned connection remains alive,
which is essential for default private launches whose ephemeral `JCODE_HOME` is owned and removed by
the launched client on drop. After sidecar discovery VibeCoder rechecks the owner generation; model
mutation is performed on the verified owner connection that will execute the turn. A successful
switch is then corroborated through a second fresh sidecar, so stale model/provider fields on the
reused owner bridge are never accepted as post-switch proof.

## Part 7 addition — OmniRoute is a strict outbound HTTP boundary

`vibecoder-gateway-omniroute` now owns URL normalization, HTTP transport construction, ephemeral
Bearer injection, redirect/proxy policy, and bounded response collection. Provider-neutral core code
does not import reqwest/url types.

The configured base is an API root, not an arbitrary endpoint. A bare origin is normalized to
`/v1/`; an existing reverse-proxy path is accepted only when it already ends in `/v1`. Plain HTTP is
loopback-only and remote gateways require HTTPS. URL user-info, query strings, fragments and port 0
are rejected.

Part 10 keeps both secret values and persisted credential references outside the client object. A `credential_ref` is resolved by the secrets layer; the resulting token is borrowed for a single request through a redacting `RequestAuth` value.

The reviewed OmniRoute `HEAD /v1/models` route returns 200 without running catalog authentication,
so VibeCoder exposes it only as a raw availability primitive. It is explicitly not gateway-health or
auth proof. Part 8 now uses credential-scoped `GET /v1/models` for semantic health/catalog mapping.


## Part 8 addition — credential-scoped gateway truth

The provider-neutral gateway contract now accepts a borrowed `GatewayCredential` per operation. It
is non-serializable and Debug-redacted, so core can request authenticated health/catalog data without
forcing any gateway implementation to persist a plaintext API key. Part 10 will resolve secret
references into this ephemeral value.

OmniRoute health and catalog discovery share one truth source: bounded `GET /v1/models`. Only
chat/responses-capable rows become `ModelRef`s; specialty-only rows are ignored. Exact model IDs and
`owned_by` identity are preserved, duplicate usable IDs fail closed, and malformed catalog/status/
content-type conditions map to stable gateway codes rather than raw server bodies.


## Part 9 addition — routing is explicit policy, not random model selection

`vibecoder-routing` owns provider-neutral primary/fallback policy. A policy resolves only against a
fresh credential-scoped gateway catalog and preserves exact model ids plus corroborated providers.
No adapter may silently substitute a model that is not present in the resolved ordered route list.

Automatic fallback is deliberately conservative. Only configured target/provider transient classes can advance; a local gateway outage stops,
and only before assistant output or tool activity begins. Authentication, access, invalid-request,
cancellation, protocol, and unknown failures stop. Once observable progress exists, replay on another
model is forbidden because coding tools may already have produced side effects. Execution of this
policy is deferred to the later task/repair orchestration stages; Part 9 establishes the safe config
and decision model only.

### Part 9 deterministic-routing runtime boundary

The routing crate resolves only an explicit ordered model plan. It does not authorize inference by
itself. OmniRoute 3.8.50 has internal model-changing layers, so the same-phone bundled runtime must
enter a VibeCoder deterministic profile before execution is wired. A hash-pinned patch already
disables the confirmed emergency budget fallback; the remaining internal routers are tracked as a
hard prerequisite for later end-to-end orchestration. No cloud server is introduced by this
boundary.


## Part 10: config/secret boundary

Persisted application config now contains only secret references. `vibecoder-config` owns strict schema loading; `vibecoder-secrets` owns short-lived resolution. OmniRoute transport owns neither the reference nor plaintext. Android production targets an `AppSecureStoreBackend` implemented with Keystore-protected app-private storage; the platform implementation remains later work.


## Part 11 managed local workspace

The private phone-local runtime now has a concrete `vibecoder-workspace-local` adapter. The Android/platform layer supplies its existing app-private directory; VibeCoder owns the physical project layout beneath `vibecoder/projects/<ProjectId>`. Models and callers do not supply filesystem roots. `ProjectRef` roots are treated as untrusted cached data and are re-derived/reverified from project identity.

Part 11 path resolution is canonical and symlink-aware for current filesystem state, but it is not a process sandbox and not a durable file-open capability. Part 12 must enforce containment again during actual I/O.


## Part 12 workspace I/O seam

`vibecoder-workspace-local` now owns safe Unix/Android file operations beneath its app-private managed root. Core callers use provider-neutral `WorkspaceRuntime` methods; they do not pass absolute filesystem authority. This seam is intentionally separate from Jcode built-in tool enforcement and from later command/process isolation.


## Part 14 addition — command authorization is not execution

`vibecoder-command-policy` is a provider-neutral trust boundary between an agent asking for a command and the future local process executor. The policy accepts structured program/argv requests only. Runtime tool ids must be explicitly registered by VibeCoder; workspace executables are project-relative and separately policy-gated. The backwards-compatible core constructor installs a deny-all command policy.

Every eligible request requires an allow-once or deny decision. Pending requests are bounded, memory-only, and bound to session plus project. Core corroborates that scope against the managed workspace and the agent runtime's current verified session/project binding before request creation, then repeats that corroboration before `allow_once`. For Jcode, the check also requires the binding to remain attached on the current connection generation. Approval UI data is not execution authority: the broker retains the validated command internally and emits a private, non-cloneable/non-serializable `CommandExecutionEnvelope` only after a matching decision. Part 15 must consume that envelope, re-resolve executable/project authority at operation time, construct a clean child environment, and provide process lifecycle/limits. Consequently `WorkspaceCapabilities.commands` and `process_isolation` remain false in Part 14.

## Part 15 addition — execution is a separate local runtime

Part 15 introduces `ProcessRuntime` rather than putting `std::process::Command` inside the command
policy or workspace contract. `CommandExecutionEnvelope` remains the only bridge from explicit
approval to execution and is consumed by ownership. The local executor reconstructs app-private
project/runtime roots at operation time, resolves runtime tools from a trusted id-to-relative-path
registry, and never uses ambient PATH lookup as executable authority.

`VibeCoderCore` keeps the process runtime optional and fail-closed. Immediately before start it again
verifies the managed project reference and current agent session/project binding, then transfers the
envelope to the executor. This makes approval, filesystem truth, agent-session truth, and process
lifecycle four distinct boundaries rather than one overpowered object.

The local supervisor uses a clean environment, closed caller stdin, nonblocking bounded output,
timeout/cancel state, and a dedicated Unix process group. Process-group lifecycle control is not
strong sandboxing: approved code still has the app UID's authority, can access network unless later
restricted, and hostile descendants may deliberately escape their process group. Those capabilities
remain false rather than being inferred from successful spawn.

## Part 16 — App-private project/session persistence

VibeCoder now has a separate persistence boundary rather than serializing live runtime objects.
`vibecoder-persistence-contract` defines a narrow versioned project record and `ProjectStateStore`;
`vibecoder-persistence-local` stores one record per project under the Android app-private root.
Project roots, live connection generations, resolved model catalogs, command approvals and process
handles remain runtime-only. Core restart flows load ProjectId, reopen the managed workspace, then
ask the agent adapter to re-corroborate the persisted session against that project.

Updates use monotonic revision compare-and-swap. Session creation persists an incomplete marker
before touching Jcode, preventing a crash window from becoming a false "no session" state. Part 17
will layer checkpoint/snapshot metadata and rollback over this durable project identity.

## Part 17 — Immutable checkpoints and atomic rollback

`vibecoder-checkpoint-contract` defines provider-neutral checkpoint metadata, limits, capabilities, and rollback results. `vibecoder-checkpoint-local` stores immutable project-tree copies outside the project workspace and hashes deterministic path/type/owner-execute/content records with SHA-256. Snapshot publication is source/copy/source corroborated.

Rollback clones the immutable snapshot into a reserved sibling and uses Android/Linux `RENAME_EXCHANGE` for one atomic live-name transition. Core requires agent/process quiescence and holds a project-scoped lifecycle permit across the replacement window so a new process start, direct workspace mutation, removal, or session create/resume cannot cross it. Command authorization epochs are invalidated before replacement and again after commit; Core then reopens the project and force-refreshes a persisted Jcode session. The checkpoint layer is separate from project/session persistence so checkpoint metadata cannot become workspace authority.


## Part 18 build-job boundary

The build layer sits above the Part 15 process runtime and below toolchain-specific orchestration. It does not gain executable-selection authority: callers still need a Part 14 allow-once envelope and Part 15 operation-time execution checks. `RunningBuildJob` normalizes process events/results into build identity and lifecycle while preserving bounded raw process evidence for later diagnostic parsing. Artifact metadata is not inferred from exit status; later pipelines must discover, hash, and then attach verified project-relative artifacts.


## Part 20 addition — website build orchestration

`vibecoder-web-build-pipeline` sits above the read-only Part-19 detector and the existing command/process/build boundaries. It is a move-only state machine, not an executor: every current-stage command still needs Part-14 allow-once authorization and Part-15 trusted runtime resolution. Core binds the pipeline to SHA-256 fingerprints of the inspected package manifest and selected lockfile, rechecks them before approval, then rechecks again under the project lifecycle gate with the Jcode workspace quiescent immediately before spawn. Locked dependency installs are supported; unlocked installs fail closed. Process success advances pipeline state but does not create artifact-verification authority.


## Part 21 addition — build repair orchestration

A provider-neutral `vibecoder-build-repair` layer converts one terminal failed build into bounded sanitized evidence plus a deterministic failure fingerprint. It grants no filesystem, command, process, checkpoint, or model authority. Core owns the actual repair boundary: verify project/session scope, ensure controlled processes and the agent are quiescent, invalidate stale approvals, create a `BeforeBuildRepair` checkpoint, then run exactly one Jcode repair turn. The next build and retry/rollback policy remain Part 22 concerns.

## Part 22 addition — bounded repair/rebuild loops

`vibecoder-build-loop` is an authority-free state machine layered above Part-21 failure evidence. It owns only retry accounting, consecutive fingerprint detection, move-only repair/rebuild permits, and an atomic cancellation signal. Core remains the authority boundary: it runs the checkpointed Jcode repair turn, invalidates command approvals on cancellation, prepares a fresh Part-20 website pipeline after repair, and delegates active agent/process cancellation to the existing verified runtimes. A successful dependency-install stage does not terminate the loop; only the final website pipeline success does. Loop state is intentionally transient and is not persisted as authorization.

## Part 23 addition — backend task and deterministic model identity

A provider-neutral Part-23 gateway profile contract separates model-catalog truth from runtime
execution semantics. The OmniRoute adapter pins the complete bundled profile, while Core stays free
of provider-specific tokens and requires only validated deterministic exact-model semantics.

`vibecoder-task-orchestration` adds the authority-free task state machine. Core retains every real
boundary: project lifecycle exclusion, workspace/session checks, gateway requests, Jcode catalog and
active-model probes, actual `run_turn`, and command-approval invalidation. This keeps task transition
logic testable without making a state object an agent, network, process, or filesystem capability.

## Part 24 addition — fixture-driven integration contracts

Part 24 adds a test-only integration layer above the existing provider-neutral contracts. Versioned
JSON fixtures are consumed by the real OmniRoute profile interpreter, the real backend-task state
machine, and a Core harness whose fake agent/gateway/workspace/process adapters grant no external
authority. The harness records call order and counts, forwards normalized events through Core, and
keeps process start fail-closed.

The hidden-reroute fixture is intentionally tied to the patch metadata and patch text rather than a
second routing implementation. Its path ids must exactly cover the audited mutation set. Source-level
validation checks this wiring in Part 24; Part 25 compiled and executed it in the clean workspace test run.

## Part 25 addition — compiled workspace baseline

Part 25 establishes the first executable baseline without changing the phone-local-first or UI-last
architecture. The repository now commits one root `Cargo.lock`, pins the minimum Rust language and
toolchain boundary to 1.88, and treats clean `--locked` workspace compilation as the reproducibility
gate for later work. Build outputs stay outside the checkpoint tree and are never packaged.

The compile loop exposed and repaired integration drift at existing boundaries: Core cancellation
now discards the invalidation count when its API promises `Result<()>`; build-loop test crates name
their direct process-contract dependency; and a project-root command working directory has one
canonical spelling (`.`) so approval payload equality cannot diverge from execution payload equality.

This host compile is not Android runtime attestation. It proves the Rust workspace and host-side
contract tests, not Android cross-compilation, packaged Jcode/Node/JDK execution, physical-device
behavior, same-UID process isolation, or production UI.
