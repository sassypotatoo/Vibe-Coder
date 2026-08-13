# Security invariants

These constraints are architectural, not optional polish for the public version.

1. **Project containment:** file/tool requests must not escape the active project workspace.
2. **Permission boundary:** destructive or high-impact operations must support deny/allow decisions;
   the application must not silently convert a missing permission capability into blanket approval.
3. **Secrets:** API/provider credentials never belong in prompts, project files, normal logs, or
   serializable domain models. Configuration refers to secret environment/key-store names instead.
4. **Runtime separation:** model routing, agent decisions, and command execution are separate trust
   boundaries. A model asking for a command does not itself authorize execution.
5. **No fake success:** build/test/command completion states must come from real executor results.
6. **Capability negotiation:** optional Jcode capabilities are checked before use instead of assumed.
7. **Fail closed:** when containment, permission state, or runtime identity cannot be verified, the
   requested execution must not proceed.
8. **Recovered proprietary reference:** recovered Claude Code source stays outside the product tree
   and is never copied into shipping code.

9. **No reasoning dependency:** provider/Jcode reasoning deltas are not surfaced or persisted; product behavior must be based on explicit outputs/tool events, not private chain-of-thought.
10. **Turn cancellation integrity:** one attached Jcode connection has at most one active VibeCoder turn; cancel, completion, future-drop cleanup, and disconnect cannot race into contradictory state.
11. **Transient tool output:** raw tool output may contain project secrets and is runtime data, not safe persistence/logging material by default. The later persistence layer must apply secret/redaction policy before storage.
12. **Permission replay resistance:** permission request ids are tied to one verified session and one connection generation; stale, duplicate, cross-session, and post-turn responses are rejected.
13. **Narrow session grants:** `AllowSession` is kept in memory as an exact action+description match and is never widened to Jcode `AllowAlways` without an upstream scope guarantee.
14. **No capability fiction:** the pinned Jcode bridge's missing `permissions` capability remains visible as unsupported; the application must rely on later workspace/process isolation rather than pretending interactive approval exists.
15. **Permission cancellation independence:** waiting for a permission-response acknowledgement must not hold the turn-control lock; explicit cancellation remains independently deliverable.
16. **Permission event bounds:** malformed control characters, oversized fields, duplicate ids, and excessive permission-request counts fail closed instead of growing unbounded in-memory state.
17. **Undeliverable interactive permission prompts fail closed.** If no event consumer exists or the callback panics while receiving a permission prompt, deny and cancel instead of leaving the agent turn blocked on an unseen request.
18. **Connection generation is immutable during a turn.** Public connect/recover, reconnect, and disconnect paths are blocked while a Jcode turn is active, preventing recovery from silently replacing the transport generation that owns permission/session state.

19. **Session-scoped model catalogs:** a model list discovered for one Jcode session must never be reused as authorization for another session.
20. **Exact model identity:** Jcode model ids are passed back verbatim; VibeCoder does not strip suffixes, synthesize provider prefixes, or guess aliases.
21. **Provider corroboration:** a caller-supplied provider is accepted only when current Jcode route/runtime metadata can verify it unambiguously.
22. **Model switch integrity:** model changes are blocked during active turns and are corroborated with runtime info after upstream acknowledgement.
23. **No capability fiction for models:** absence of a dedicated hello token is not treated as proof either way; model-selection support is marked true only after an operational catalog probe succeeds on the current connection generation.

24. **Fresh model catalog without owner-runtime churn:** model discovery/selection opens a fresh sidecar API connection to the same live Jcode socket, subscribes before target-session attach, and waits for the post-attach model probe. The manager-owned connection remains alive so an SDK-owned ephemeral `JCODE_HOME` is not deleted. A previous attachment's cached catalog is never an authorization source, including when the target catalog is empty; the owner generation is rechecked before any authorized mutation. After a model mutation, a second fresh sidecar independently corroborates the active model/provider before execution continues.
25. **Gateway transport security:** plain HTTP is accepted only for loopback OmniRoute endpoints; remote gateways require HTTPS.
26. **No URL credentials:** OmniRoute base URLs may not contain user-info, query strings, fragments, or an endpoint-specific path that could hide credentials/routing mistakes.
27. **No implicit credential hops:** OmniRoute redirects and ambient system/environment proxies are disabled for the credential-bearing client. Any future explicit proxy support requires a reviewed configuration boundary.
28. **Ephemeral gateway auth:** Bearer values are borrowed per request, redacted from Debug, never serialized by VibeCoder, and never attached to the upstream unauthenticated HEAD availability probe.
29. **Bounded gateway responses:** HTTP response bodies are streamed into a bounded buffer; Content-Length is only an early rejection hint, not the sole size control.
30. **Availability is not authentication:** OmniRoute `HEAD /v1/models` cannot establish health/auth/catalog access. Semantic readiness must come from the authenticated GET path in Part 8.
31. **Credential-scoped catalog truth:** gateway health and discovery use the same credential-scoped `GET /v1/models`; an unauthenticated availability response can never upgrade the gateway to ready.
32. **Coding-only gateway catalog:** specialty-only media/embedding/rerank/moderation models are not exposed as coding choices. Multi-capability rows are accepted only when they explicitly advertise `chat` or `responses`, or are generic/chat rows.
33. **Exact gateway model identity:** usable OmniRoute model ids and `owned_by` values are preserved verbatim. Duplicate usable ids, malformed envelopes, and ambiguous catalog data fail closed rather than being silently normalized.
34. **No raw gateway error persistence:** health details are stable VibeCoder codes only; raw HTTP response bodies and raw transport/server error prose are not copied into product state.
35. **Phone-local is not unauthenticated-local:** the selected Android local-first architecture does not imply trusting every process that can reach a loopback port. Part 10 must give the local gateway a proper secret/credential boundary suitable for Android secure storage rather than assuming environment variables are the final mobile secret store.

36. **Gateway outage is not a model fallback:** when the same phone-local gateway is unavailable, switching configured models is forbidden because every route depends on that gateway.
37. **Explicit routing only:** automatic inference routing may use only the exact ordered models in a resolved policy; an adapter must never silently choose another catalog model.
38. **Provider pins are corroborated:** a configured provider must exactly match fresh gateway catalog ownership when specified; aliases are not guessed.
39. **Fallback is transient-only:** authentication, access denial, invalid requests, cancellation, protocol failures, and unknown failures cannot trigger automatic model fallback.
40. **No fallback after observable progress:** once assistant output or tool activity starts, a failed coding turn cannot be replayed automatically on another model because tool side effects may already exist.
41. **Bounded route policy:** route count, identity length, duplicate targets, duplicate triggers, and catalog ambiguity are validated before execution.
42. **Resolved route truth is runtime-only:** resolved route policies are not deserializable/persistable authorization objects; they must be rebuilt from a fresh gateway catalog.
43. **Attempt progress is monotonic:** response/tool progress flags are private and can only transition from not-started to started, preventing orchestration code from accidentally re-enabling fallback after side effects.
44. **Automatic route order cannot be fabricated:** attempt state starts at the policy primary, is move-only, and only a successful fallback decision can issue the next route state.
45. **Opaque gateway combos are not exact models:** OmniRoute catalog rows owned by `combo` are filtered from VibeCoder's coding catalog until their internal targets can be inspected and pinned; hidden multi-model strategies cannot satisfy explicit-route guarantees.
46. **Gateway and agent model namespaces are not assumed identical:** an OmniRoute catalog model cannot be passed directly into Jcode execution until the active Jcode session independently corroborates that route identity.

47. A catalog-resolved model route is not sufficient proof of exact inference execution.
48. The bundled OmniRoute runtime must apply only hash-pinned VibeCoder patches; fuzzy patching is forbidden.
49. OmniRoute 3.8.50 emergency budget fallback must be code-level disabled in the bundled runtime; an environment flag alone is insufficient because a DB feature-flag override has higher precedence.
50. Exact inference execution stays disabled until every reviewed OmniRoute model-changing layer is either bypassed, disabled, or pinned and the selected route is separately corroborated in Jcode.


## Part 10 secret/config invariants

- Persisted config must never contain plaintext API keys, passwords, bearer/access tokens, client secrets, or private keys.
- Duplicate JSON object keys are rejected; last-key-wins ambiguity is forbidden.
- `SecretValue` is non-serializable/non-cloneable, Debug-redacted, bounded, and zeroized on drop.
- Secure-store references never silently fall back to environment variables.
- Gateway transport config stores no credential reference/value.
- Errors emitted by config/secret loading are stable codes and do not echo input/secret values.

## Part 11 workspace invariants

51. **Workspace-root authority belongs to VibeCoder.** New-project requests generate a fresh private project id and never accept an arbitrary physical root or caller-fixed creation identity.
52. **App-private root is the trust anchor.** The local workspace derives fixed `vibecoder/projects/<ProjectId>` descendants from one canonical platform-supplied app-private directory.
53. **Managed roots reject symlinks.** The product root, projects root, and project root cannot be existing symlinks and must canonicalize beneath their expected canonical parent.
54. **Serialized `ProjectRef` is not trusted.** Its root must exactly match the path derived from its project id and pass fresh canonical verification before session or workspace operations.
55. **Tool paths are project-relative.** Absolute paths, parent traversal, separator ambiguity, existing symlink components, and canonical escapes fail closed.
56. **Containment resolution is not a durable capability.** A resolved `PathBuf` does not authorize later I/O; Part 12 must re-check containment during actual read/write operations to reduce check/use races.
57. **Logical containment is not process isolation.** The Part 11 workspace does not claim shell/process sandboxing; command execution remains disabled until later process-policy/isolation stages.
58. **No file-I/O capability fiction.** Part 11 kept file I/O disabled; Part 12 enables `read_write_files` only for the new Unix/Android operation-time primitives, not for unconstrained Jcode/shell tools.

59. **Hard-link aliases are not solved by canonical paths.** Part 12 regular-file primitives reject `st_nlink != 1`, and atomic writes never truncate an existing inode in place. Later command isolation must prevent an agent shell from manufacturing aliases outside these APIs.

60. **File authority is fd-relative at operation time.** Unix/Android file primitives re-enter the managed app-private/project tree through directory descriptors with `O_NOFOLLOW`; a Part 11 resolved path is never reused as authority.
61. **Only regular single-link files are readable.** Symlinks, directories, special files, and multi-link regular files fail closed; reads are bounded twice against a 16 MiB hard ceiling.
62. **Writes replace, never write through.** Atomic writes create an owner-private same-directory temp inode, sync it, and `renameat` it into place. Existing multi-link or read-only targets fail closed.
63. **Private modes by construction.** New files are forced to `0600`; new directories to `0700`. Existing file replacement preserves owner-only mode (including execute) while stripping group/other access.
64. **File primitives are not process isolation.** Part 12 does not claim safety against an untrusted concurrent same-uid process that can rename already-open directories. Command execution stays disabled until later process isolation/policy closes that boundary.
65. **Workspace I/O capability does not imply Jcode tool confinement.** `read_write_files=true` describes the VibeCoder workspace API only; Jcode built-in file tools are not yet proven to pass exclusively through it.
66. **Atomic temp namespace is reserved.** Public workspace paths cannot target `.vibecoder-tmp-*`; those names belong only to the internal same-directory replacement mechanism.

## Part 13 edit/search invariants

67. **Exact edit means exact.** A text edit commits only when the requested old UTF-8 fragment has exactly one possible occurrence; overlapping occurrences are treated as ambiguity.
68. **Patches are all-or-nothing.** Multi-hunk patches validate and apply every bounded hunk in memory before one atomic file replacement; a failed hunk commits zero partial changes.
69. **Patch target freshness is rechecked.** Immediately before rename, the original target inode, owner mode, and full contents are corroborated again; changed targets fail closed.
70. **Patch temp files keep Part 12 hard-link defense.** The temporary inode must remain a regular single-link inode before content is written.
71. **Discovery never grants authority.** Listed/searched relative paths are observations only; later reads/edits re-enter through the fd-relative operation-time boundary.
72. **Project walking never follows symlinks.** Android/Linux listing uses `fstatat(... AT_SYMLINK_NOFOLLOW)` plus `O_NOFOLLOW` directory opens; symlink/special/hard-linked entries are skipped.
73. **Search is resource bounded.** Walk entries/depth, listed files, per-file bytes, total bytes, query size, result count, and preview length are bounded.
74. **Search does not expose app-private absolute paths.** Results contain validated project-relative UTF-8 paths only.
75. **Search/edit APIs are not Jcode confinement.** Jcode built-in tools and future command processes are not considered sandboxed until their execution envelope/isolation explicitly routes authority through reviewed boundaries.


## Part 14 command-policy invariants

76. **A command request is not shell text.** Authorization accepts a structured program plus bounded argv; there is no generic shell-command-string field.
77. **Runtime executable authority is explicit.** Runtime tools are opaque ids from a VibeCoder allowlist, not PATH lookups or caller absolute paths; common shell interpreter ids are rejected.
78. **Workspace executables remain relative authority.** Their paths reject absolute paths, parent traversal, control/backslash ambiguity, oversized components, and the internal temp namespace; Part 15 must still reverify them immediately before spawn.
79. **Eligibility never auto-runs.** Every eligible Part 14 command requires an explicit allow-once or deny decision; deny-all is the default policy.
80. **Approval scope is exact.** Pending requests bind session id plus project id. A scope mismatch cannot consume or authorize another pending request.
81. **Approval display is not execution authority.** `allow_once` requires the returned approval payload to exactly echo the broker-retained validated command, and execution still uses the broker copy. A correctly scoped deny may clear a tampered display payload because denial grants no authority.
82. **Execution envelopes are intentionally non-persistable.** `CommandExecutionEnvelope` is private-field, non-cloneable, and non-serializable; the Part 15 executor must consume it by ownership.
83. **Child environment authority is not caller controlled.** The Part 14 envelope specifies a runtime-managed clean environment, no ambient inheritance contract, and no caller/model stdin.
84. **Command diagnostics redact arguments.** `CommandSpec` Debug output never prints argument contents; approval serialization is ephemeral display data and must not carry provider secrets.
85. **Approval is not sandboxing.** Command arguments and executable behavior can still request powerful filesystem/network effects. No command/process-isolation capability is advertised until the Part 15 executor enforces the next boundary.
86. **Pending approval state is bounded and ephemeral.** At most 64 commands globally and 8 per session are pending, duplicate identical pending requests are rejected, collision-safe insertion never overwrites an existing request, and session cleanup can revoke them. Part 14 does not yet bind command approval to a Jcode connection/turn generation.
87. **Approval display rejects direction-control spoofing.** Command arguments, paths, and session scope reject control characters and Unicode bidi-direction controls before an approval object is created.

88. **Command scope is corroborated, not caller-asserted.** Core verifies the managed project and asks the agent runtime to prove the session is currently bound to that project before a command request is accepted.
89. **Allow-once rechecks scope.** Immediately before issuing an execution envelope, Core repeats workspace verification plus agent session/project corroboration; Jcode additionally requires the binding to be attached on the current connection generation. Denial does not require a live agent because denial grants no authority.

## Part 15 process-execution invariants

90. **Approval and spawn remain separate.** An allow-once decision only creates a move-only envelope; `VibeCoderCore::start_authorized_project_command` is a distinct execution-time boundary.
91. **Execution rechecks scope.** Immediately before start, core re-verifies the managed project plus current Jcode session/project binding and rejects envelope/session/project mismatch.
92. **Executable authority never comes from ambient PATH.** Runtime tools resolve only through an explicit trusted id-to-app-private-relative-path registry; workspace executables remain project-relative.
93. **Execution-time path checks are repeated.** Working directories and executables reject parent traversal, symlinks, non-regular executable targets, hard-link aliases, and missing owner execute permission immediately before spawn. These checks are not claimed race-proof against hostile same-UID mutation.
94. **Child environment is runtime-managed clean.** The executor uses `env_clear`, supplies only runtime-owned `HOME`/`TMPDIR`, closes caller stdin, and does not accept caller/model environment variables.
95. **Output memory is bounded twice.** Final stdout/stderr capture has explicit per-stream ceilings and live output uses a separately bounded event queue; slow consumers cannot create an unbounded backlog.
96. **Captured output is Debug-redacted.** Debug formatting reveals byte counts and lifecycle metadata, never stdout/stderr contents.
97. **Cancellation and timeout own a process group.** Each Unix/Android child enters a new process group; cancel/timeout sends TERM and then bounded-grace KILL to that group.
98. **Post-spawn setup failure cannot knowingly orphan the child.** Pipe/supervisor setup failures synchronously terminate and wait the spawned child/process group before returning an error.
99. **Process concurrency is bounded.** At most four active local processes and two per project are admitted by the Part 15 registry.
100. **Lifecycle control is not strong isolation.** Approved code still has app-UID authority; network, argument semantics, same-UID pathname races, process-group escape, and Jcode built-in command-tool confinement remain unresolved.

101. **Output draining cannot monopolize lifecycle checks.** Each stdout/stderr drain is capped to eight 16 KiB chunks per supervisor poll, so a continuously noisy process cannot indefinitely starve timeout/cancellation observation.

102. **Process-runtime Debug does not expose app-private absolute paths.** `LocalProcessRuntime` redacts its app/runtime roots and reports only non-sensitive runtime-tool counts.

103. **Timeout starts at child spawn.** Supervisor scheduling delay does not extend the configured process timeout; the deadline is derived from an `Instant` captured immediately after successful spawn.
104. **Group-signal failure has a direct-child fallback.** If TERM/KILL delivery to the command process group fails, the executor invokes direct-child kill rather than allowing timeout/cancellation to become best-effort only.

105. **Termination keeps the process-group id owned through escalation.** After TERM is requested, the direct child is not reaped during the grace window; the leader PID therefore cannot be reused before group SIGKILL is sent.

106. **Normal successful leader exit is not a no-daemon guarantee.** A descendant deliberately detached/backgrounded by an approved executable may outlive the tracked command; Part 15 guarantees process-group cleanup on cancellation, timeout, and supervised errors, but does not claim kernel-enforced daemon prevention.

## Part 16 persistence invariants

107. **Persisted project identity is not filesystem authority.** State stores `ProjectId`, never an absolute workspace root; reopen derives and verifies the managed root again.
108. **Persisted session identity is not runtime authority.** A stored session id must be resumed and project-corroborated before use after process restart.
109. **Secrets and volatile agent output are not project state.** API keys, secret values, prompts, reasoning, raw tool/process output, command approvals, execution envelopes, and connection generations are excluded from the schema.
110. **Project state replacement is crash-oriented atomic.** Local state uses private temp file, file sync, rename, and parent-directory sync.
111. **State paths fail closed on aliasing.** Operation-time state access rejects symlinks, multi-link regular files, wrong-owner files, and non-private state-file modes.
112. **State updates are revision guarded.** Stale compare-and-swap revisions cannot silently overwrite newer project metadata.
113. **Session creation has a durable ambiguity marker.** `session_creation_pending` is committed before upstream session creation and blocks automatic resume until cleared by a verified outcome.
114. **Persisted model/route data is preference/configuration only.** It never substitutes for fresh Jcode model verification or a fresh gateway catalog.
115. **Persistence is not a process sandbox.** Same-UID hostile cross-process mutation remains outside the guarantees of Part 16.

## Part 17 checkpoint/rollback invariants

116. **Snapshots are real project copies, not metadata-only promises.** Published checkpoints contain a private complete regular-file/directory tree.
117. **Checkpoint storage is outside agent-visible project roots.** A project edit cannot normally mutate its own restore point.
118. **Snapshot publication is integrity-gated.** Pre-copy live-source digest, copy digest, copied-tree recheck, and post-copy live-source digest must agree before publish.
119. **Unsafe filesystem aliases fail closed.** Symlinks, hard-linked regular files, special files, invalid path components, and internal temp namespaces are rejected.
120. **Snapshot resources are bounded.** Per-project checkpoint count, traversal depth, file count, and total bytes have hard ceilings.
121. **Rollback does not mutate the immutable checkpoint.** A private staging clone is made first.
122. **The live project name changes atomically.** Android/Linux rollback requires `renameat2(..., RENAME_EXCHANGE)`; no multi-rename fallback is accepted.
123. **Rollback is integrity-verified after exchange.** Mismatch triggers exchange-back; failed recovery is surfaced as a fatal rollback error.
124. **Active local processes block checkpoint and rollback.** Open cwd/fds must not silently keep operating on a replaced tree.
125. **An active Jcode turn blocks checkpoint and rollback.** The controlled agent must be quiescent before snapshot/replacement.
126. **Rollback invalidates command authorization epochs.** Pending and already-issued pre-rollback command authority cannot silently carry into the restored workspace.
127. **The workspace is reopened after rollback.** Same path text is not sufficient identity after directory exchange.
128. **Persisted Jcode sessions are force-refreshed.** Attachment is cleared and a fresh attach/list-sessions project-root corroboration is required.
129. **Checkpoint metadata contains no secret fields or tool/process output.** It stores only ids, reason, time, file/byte counts, and the integrity digest.
130. **Part 17 is not strong same-UID process isolation.** A malicious same-UID mutator remains outside this checkpoint's guarantees until later isolation/orchestration work.
131. **Same-project lifecycle transitions are serialized.** Process startup, checkpoint/rollback, direct workspace mutation, project removal, and session create/resume cannot cross the same project replacement window through Core.
132. **Rollback invalidates authorization both before and after replacement.** Approvals issued during the rollback window cannot survive into the restored workspace.
133. **Committed rollback is not reported as failed for cleanup debt.** Exchange-parent sync failure must recover before returning an error; after a verified committed exchange, stale old-tree cleanup may be retried at initialization.


## Part 18 build-job invariants

134. **Build identity does not create execution authority.** A build job wraps one command already authorized by Part 14 and started through Part 15; it cannot fabricate an execution envelope.
135. **Build output remains bounded and Debug-redacted.** Part 18 inherits Part 15 capture/event ceilings and does not add an unbounded duplicate log buffer.
136. **Timeout and cancellation stay distinct.** They are not collapsed into a generic build failure, preserving correct repair/retry decisions later.
137. **Artifact paths are not filesystem authority.** Artifact metadata is project-relative and rejects absolute paths, parent traversal, controls, backslash ambiguity, and VibeCoder internal temp names.
138. **Process success is not artifact proof.** Exit code zero alone never produces a verified artifact claim.
139. **Artifact digest metadata is not verification authority.** A recorded SHA-256 field must be lowercase and well-formed, but Part 18 does not claim it has verified the referenced bytes.
140. **Part 18 does not parse compiler errors or persist raw output.** Error extraction belongs to Part 21 and persistence remains separately scoped.

141. **Build ids exist before process start and descriptors are consumed on start.** Queued identity is real, and one descriptor cannot be accidentally reused for two build starts through Core.

142. **Build artifact/diagnostic paths are strict UTF-8.** Lossy conversion is rejected so different filesystem byte names cannot collapse into the same displayed metadata path.

143. **Normalized build metadata rejects bidi spoofing.** Diagnostic codes/messages and artifact/diagnostic paths reject Unicode direction-control characters before later UI use.
144. **Artifact result paths are unique.** One normalized build result cannot contain duplicate artifact paths that make downstream selection ambiguous.

145. **Artifact/diagnostic paths use canonical relative spelling.** Redundant separators, dot components, and trailing separators are rejected rather than allowing multiple metadata spellings for one path.

## Part 19 website toolchain invariants

- Website toolchain detection is read-only and creates no process-execution authority.
- Package-manager selection fails closed on multiple lockfile families or packageManager/lockfile disagreement.
- Toolchain reports never expose or authorize the package `build` script body.
- Package-manager runtime tool ids are fixed logical registry ids, not executable paths and not ambient PATH lookups.
- Toolchain detection probes a fixed root-metadata set and does not recursively enumerate `node_modules`.
- The exact inspected `package.json` and selected lockfile bytes are SHA-256 fingerprinted for later drift checks.
- `engines.node` remains advisory until packaged-runtime compatibility verification is implemented.


## Part 20 website build-pipeline invariants

146. **A website pipeline is not execution authority.** It can only emit a structured command that still requires Part-14 allow-once approval and Part-15 operation-time trusted-tool resolution.
147. **Prepared pipelines are move-only.** One pipeline state cannot be cloned and replayed through the intended API.
148. **Unlocked dependency installation fails closed.** Part 20 does not silently create or rewrite dependency graphs without a committed lockfile.
149. **Dependency install scripts are disabled by default.** Enabling them is an explicit policy choice and does not create a strong-sandbox claim.
150. **Approval is bound to exact package metadata.** The full Part-19 report includes SHA-256 of `package.json` and the selected lockfile.
151. **Toolchain drift is checked twice.** Core re-inspects before approval and again under the same-project lifecycle gate immediately before spawn.
152. **Build start requires agent quiescence.** The controlled Jcode workspace cannot be in an active turn while the approved package/build command is being rebound and started.
153. **The authorized command must equal the current stage command.** A valid envelope for another command cannot be substituted into the website pipeline.
154. **Package-manager discovery does not depend on recursive `node_modules` traversal.** Post-install file-count growth cannot invalidate root toolchain detection merely by exceeding project-list bounds.
155. **Build-process success is not artifact verification.** A succeeded pipeline records process success only; site-bundle discovery/integrity remains later work.
156. **Node engine compatibility is not falsely claimed.** `engines.node` remains advisory until the provisioned Android runtime can be version-corroborated with npm-compatible range semantics.


## Part 21 build-repair invariants

158. **Only failed builds are repair eligible.** Success, cancellation, timeout, queued, and running states do not enter the repair-turn path.
159. **Repair evidence is bounded and transient.** Raw stdout/stderr is not persisted; the model receives only capped sanitized evidence.
160. **Common credential-bearing build lines are redacted.** Known authorization/token/password/secret markers cause the entire line to be withheld from repair evidence.
161. **Absolute-path-shaped evidence tokens are redacted.** App-private physical paths are not deliberately copied into repair prompts.
162. **Build evidence is untrusted prompt data.** The repair prompt explicitly states that evidence is data, not instructions.
163. **Repair requires a rollback point.** Core creates a `BeforeBuildRepair` checkpoint before the repair turn begins.
164. **Repair scope is freshly corroborated.** Project verification, zero controlled active processes, Jcode quiescence, and session/project binding checks happen before the turn.
165. **Same-project controlled lifecycle operations cannot overlap the repair turn.** The lifecycle permit spans checkpoint creation and the agent turn.
166. **Stale command approvals are revoked around repair.** Project command authorization is invalidated before checkpoint/repair and again after the turn attempt.
167. **Part 21 performs one repair turn only.** Rebuild/retry budgets, repeated-failure stopping, and automatic rollback decisions remain Part 22.
168. **Failure fingerprints exclude build identity.** Equivalent sanitized failures can be detected across rebuilds without treating a fresh BuildId as a new root cause.

169. **Evidence delimiters cannot be injected by build output.** Embedded repair-evidence delimiter strings are neutralized before prompt construction.
170. **Oversized single log lines fail-redact.** A single multi-kilobyte line is replaced instead of copied into repair evidence or forcing unbounded per-line sanitization.

## Part 22 repair-loop guards

- Repair retry budgets are hard-bounded; the default is three repair attempts and the maximum is eight.
- Repeated identical failure fingerprints stop before another repair turn by default on the second occurrence.
- A different failure resets only the consecutive-repeat count, never the total repair-attempt budget.
- Repair and rebuild permits are move-only scoped state, not execution authority.
- Loop cancellation invalidates outstanding project command approvals before later stages can proceed.
- Active repair-turn cancellation delegates to the verified Jcode session binding; active guarded website rebuild cancellation delegates to the existing process runtime.
- Cancelled and timed-out builds are terminal loop outcomes, not repairable failures.
- A fresh Part-20 pipeline is required after repair; no previous command approval or stale toolchain plan is reused.
- Part-22 loop state is transient and is not persisted as durable authorization.

## Part 23 backend-task and exact-model invariants

171. **A gateway catalog is not execution attestation.** Core requires a freshly fetched, adapter-validated deterministic runtime profile before resolving and executing a task.
172. **The bundled profile rejects model mutation.** Every audited OmniRoute alias/router/hook/combo/default/background/effort/family/emergency model-changing path either preserves the exact model or stops before provider work.
173. **Partial patching is not accepted.** All original source hashes, all exact hunks, all resulting hashes, and the fixed runtime-profile endpoint must agree as one patch set.
174. **Gateway and agent identities are independent.** The exact id and non-empty provider must match in a fresh gateway catalog, a fresh Jcode session catalog, and a fresh post-selection active probe.
175. **Inference rechecks model identity.** `run_turn` repeats fresh selection and post-switch corroboration immediately before the actual model turn.
176. **Observable progress forbids replay.** Assistant text, background progress, or any tool activity permanently blocks automatic fallback for that attempt.
177. **Unknown prose is never a transient failure class.** Agent error strings are not searched for rate-limit, timeout, or model-unavailable hints.
178. **Task state grants no authority.** The state crate has no agent/gateway/network/workspace/process/command/secret dependency and cannot execute its own transition decisions.
179. **Task content is not diagnostic metadata.** Prompt text is not retained; assistant text and tool output are withheld from task Debug output.
180. **Project authority is held for the complete task.** Project verification, process exclusion, agent quiescence/session binding, model corroboration, inference, and approval invalidation occur under one lifecycle permit.
181. **Same-model account handling is not a model reroute.** Credential refresh, account failover, and retries are permitted only while provider/model identity remains unchanged.
182. **Runtime profile is not process attestation.** Same-UID endpoint spoofing and Android packaged-runtime isolation are not claimed solved.

## Part 24 fixture and failure-contract invariants

183. **Fixtures are inputs, not authority.** Part 24 JSON cannot create sessions, access secrets, start processes, mutate workspaces, or authorize model execution.
184. **Accepted runtime attestation has one exact identity.** The adapter fixture accepts only the pinned gateway/version/profile/digest with both deterministic flags true.
185. **Corrupt profile responses fail closed.** Wrong status/media type, empty or malformed JSON, unknown fields, flag drift, version drift, and digest drift have rejected expectations.
186. **Hidden-reroute coverage is exact.** Fixture ids must equal the full closed mutation-path set in the patch metadata; omissions and extras fail validation.
187. **Gateway and Jcode identity drift reaches zero inference calls.** Catalog or active model/provider mismatch cases require `run_turn_calls` to remain zero.
188. **Only configured pristine fallback is exercised.** The integration fixture advances from a missing primary only to its explicit next route before inference.
189. **Observable progress remains replay-blocking.** Text, background progress, tool start, and tool finish each have a terminal no-replay contract.
190. **Cancellation is terminal.** A cancelled turn result cannot become a successful backend-task outcome or authorize fallback.
191. **Prose errors remain untyped.** An agent error string does not become a transient fallback class even when a fallback is configured.
192. **Authority minted around inference expires.** Both a pre-turn envelope and a pending approval created during the turn are invalid after the turn boundary.
193. **The integration process fake cannot launch.** Its start method records any call and returns an error; all Part 24 cases require zero starts.
194. **Static checks are not compiled results.** Part 24 recorded source/fixture validation only; Part 25 separately records the first compiled and executed test results.

## Part 25 compile-baseline invariants

195. **The dependency graph is locked.** Full workspace build/test commands use the committed root `Cargo.lock` with `--locked`; silent resolver drift is not accepted.
196. **A clean target is the compile truth.** Final workspace tests are rebuilt from an empty external target directory so stale incremental artifacts cannot masquerade as source failures or success.
197. **Warnings are milestone failures.** All workspace targets must pass Clippy with warnings denied, with any narrowly retained boundary documented at its source site.
198. **Root working-directory identity is canonical.** The logical project root is represented as `.` in command policy so approval and execution compare the same payload.
199. **Environment-blocked tests are not counted as passed.** The two Jcode lifecycle tests denied Unix-socket creation by the runner remain explicitly unexecuted; no vendored test is weakened or rewritten to manufacture success.
200. **Host compilation is not Android attestation.** Passing host builds/tests does not claim Android cross-compilation, packaged runtime execution, device isolation, or production UI completion.

## Part 26 — Android packaged-code and readiness invariants

- Writable project files cannot be launched directly as `WorkspaceExecutable` on Android; use a trusted package-installed interpreter/runtime tool.

- **Writable app data is not executable-code authority.** Native runtime tools must not be
  provisioned under writable app-private state merely because desktop Linux permits `execve` there.
- **Package-installed code and writable runtime data are separate roots.** The process runtime
  rejects overlapping roots and Android rechecks that the supplied package-code root is not writable
  by the app process before resolving a native runtime tool.
- **Android readiness requires evidence.** A manifest entry, source compile, file name, or chmod bit
  cannot by itself mark Jcode, Node, Java, aapt2, zipalign, or other native runtime code Android-ready.
  Required probe states must explicitly pass; `NotRun` is blocking.
- **16 KB compatibility is a native-artifact requirement.** Every native artifact in the ARM64
  inventory requires explicit 16 KB page-size compatibility proof before its capability can be ready.
- **Jcode needs a socket proof, not a Unix assumption.** Agent readiness requires a successful
  packaged Jcode execution/version check and Unix-domain-socket round trip on Android.

## Part 27 — Android host and runtime-probe invariants

- **JNI library placement is not automatically child-process authority.** The Android host keeps the
  JNI/native-library root separate from the package-owned filesystem root used for child processes.
- **Package code roots never fall back to writable app data or ambient PATH.** Jcode receives an
  explicit package executable path; Node is registered only from the package executable root.
- **A script asset is not an executable.** Script-based runtime tools require a trusted interpreter
  plus a bounded fixed argv prefix; npm additionally requires runtime-binding evidence before
  Website Build can become ready.
- **Asset presence is not service readiness.** OmniRoute requires a service round trip in addition
  to package presence.
- **Host structural probes are not device execution.** Non-Android execution/version evidence stays
  `NotRun`, even if the ELF is valid ARM64.
- **Android code roots must be non-writable to the app UID.** The host checks both native-library and
  child-executable code roots on Android before runtime initialization.
- **16 KiB proof is structural and fail closed.** Every native artifact must be AArch64 ELF64 and all
  `PT_LOAD` segments must satisfy the required 16 KiB alignment/congruence test before that proof can pass.
- **Probe output pipes cannot extend the timeout through EOF ownership.** Android version probes use
  nonblocking bounded drains and never join a reader waiting for a descendant-held stdout/stderr pipe.
- **A non-writable code directory does not excuse a writable code file.** Android rechecks the exact
  package-native executable/library candidate with the app UID before process/probe authority is used.

## Part 28 — Android shell and provisioning invariants

- **The diagnostic UI reports evidence; it does not mint evidence.** File names or manifest rows cannot
  make a runtime capability ready.
- **Missing packaged runtimes degrade to NOT READY, not bootstrap success.** In particular, absent Jcode
  does not abort the whole snapshot or get replaced by ambient PATH lookup.
- **JNI bootstrap has a fixed ABI and bounded output.** The C bridge accepts at most the bounded inventory
  payload and the Rust host snapshot is capped before copying across FFI.
- **Network-fetched runtime source must be pinned before extraction.** Node source is SHA-256 verified;
  partial downloads are never promoted to the cache entry.
- **Reviewed runtime identities are not silently substituted.** Missing Jcode/OmniRoute reviewed archives
  remain missing; an unrelated public release or guessed URL is not an acceptable replacement.
- **Gradle wrapper generation starts from a verified distribution.** A wrapper JAR is not downloaded from
  an arbitrary mirror or invented when the official binary cannot be retrieved.
- **Package extraction configuration is not execution proof.** The Android host still verifies package
  code roots/files and Part-27 native probes remain required on-device.
- **The first Android screen is not the production UI.** It is temporary diagnostic authority-free
  presentation only; production UI remains deferred until the backend/runtime path is proven.

## Part 29 — Jcode Android executable provenance invariants

- **AArch64 is not an operating-system identity.** A generic Linux AArch64 PIE cannot satisfy the Android
  Jcode payload contract solely because its ELF machine field matches.
- **The Android Jcode build source is immutable.** Runtime builds require the exact `v0.73.0` release
  commit `44ffa55281fad71c02be984c0674d92412210452` and a clean checkout.
- **Reviewed vendored seam and runtime binary provenance are separate.** The historical reviewed archive
  digest corroborates the vendored SDK/harness files; the Android executable is built only from the exact
  upstream Git commit and the public seam is re-hashed against that checkout before compilation.
- **Foreign dynamic loaders fail before exec.** A packaged native executable with `PT_INTERP` other than
  `/system/bin/linker64` is rejected; static PIE may omit `PT_INTERP`.
- **CI is evidence generation, not evidence substitution.** Workflow definitions do not prove an APK or
  Jcode payload exists. Only produced artifacts plus device probes can move readiness to READY.
- **The minimal APK remains independently buildable.** Jcode cross-compilation failure must not erase the
  ability to produce the diagnostic shell that reports Jcode as NOT READY.

## Part 30 Android proof invariants

- **An assembled APK is not device proof.** Signature, 16 KiB alignment, ABI contents and packaged ELF identity are checked before install evidence is accepted.
- **UI appearance is not runtime attestation.** The diagnostic activity writes a bounded app-private machine-readable report and the adb harness evaluates that report.
- **Core readiness requires a real loaded Rust host.** Missing JNI/Rust payloads cannot be converted into optimistic readiness.
- **Jcode readiness requires the private socket round trip.** Version output alone cannot make Agent READY.
- **Device automation uses app-private `run-as`, not exported diagnostic storage.** The diagnostic report is not written to shared/external storage.

- **CI toolchain provenance must match the reviewed toolchain identity.** Part 31 pins the reviewed Android command-line-tools revision in CI and emits APK/native/source hashes after verification.

- **An APK artifact without build evidence is not a reproducible build proof.** Part 31 pins the reviewed Android command-line-tools revision in CI and emits APK/native/source hashes after verification.

- **Generated native payloads are not source files.** Android host/Jcode build outputs must stage under `android/app/build/generated/jniLibs`; generated `.so` files must never be written into the checksummed `src/main/jniLibs` source tree.

- **Build outputs cannot become checksum source authority.** Generated build roots are excluded from `CHECKSUMS.sha256`; CI/local build artifacts must be proven by build evidence and APK verification instead.
- **Minimal/Jcode lanes are artifact-isolated.** The minimal lane clears generated JNI payloads; the Jcode lane preserves only its freshly verified Jcode executable before rebuilding the Android host.

- **Diagnostic signing is stable but never production authority.** Debug APKs use the pinned diagnostic keystore/certificate for repeatable upgrade tests; release signing must never reuse that key. APK verification rejects signer drift.

## Part 31 reviewed-fix invariants

- **Unpinned runtime identity is a readiness blocker.** Presence/execution cannot make an intentionally unpinned component ready.
- **Version diagnostics retain expected and observed identities.** A failed trusted executable version probe must not discard the semantic version it actually observed.
- **APK asset presence is measured, not inferred.** JavaArchive/DataBundle package evidence comes from Android `AssetManager`; service/runtime-binding proof remains independently required.
- **JNI host resolution is cached but local.** The Rust host is resolved once with `RTLD_LOCAL`; VibeCoder does not turn host symbols into a global plugin namespace.
- **Async execution has an owned boundary.** Synchronous Android/JNI code uses the Android host executor rather than ambient Tokio state; nested synchronous `block_on` is rejected.
- **Writable-code checks are sanity guards, not integrity claims.** `access(W_OK)` rejection complements package signing/ownership and is not presented as cryptographic or complete SELinux attestation.
- **Reviewed NDK host tag is frozen.** The pinned Apple-Silicon build path keeps Android NDK host tag `darwin-x86_64`; `darwin-aarch64` must not be introduced without a separately verified NDK contract change.
