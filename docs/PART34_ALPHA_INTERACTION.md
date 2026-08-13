# Part 34 — Alpha interaction controller

Part 34 introduces the durable conversation boundary needed for VibeCoder to behave like a normal
assistant by default while retaining explicit agent actions. It does not manufacture an Android
Alpha success while the phone-local model-gateway payload is absent.

## Interaction rule

One invocation of `VibeCoderCore::run_persisted_conversation_turn` means exactly one user turn:

1. reopen the managed project by `ProjectId`;
2. load one conversation by `(ProjectId, ConversationId)`;
3. reject incomplete session creation or an interrupted pending turn;
4. re-corroborate the persisted Jcode session against the current runtime and project;
5. append the user message and atomically commit `turn_pending = true`;
6. execute the existing single `run_backend_task` boundary;
7. on success, append the assistant text and atomically clear `turn_pending`;
8. on a normal error/cancellation, clear `turn_pending` while preserving the user message;
9. return to the caller. There is no automatic re-prompt.

A repeated autonomous workflow therefore requires a separate explicit bounded loop controller. A
normal question or action request cannot accidentally become an endless agent loop merely because
it used a tool.

## Multiple chats per project

The Part-16 project registry is retained for backwards compatibility. Part 34 adds a separate
conversation registry. Each conversation owns:

- a stable random `ConversationId`;
- its owning `ProjectId`;
- one corroborated Jcode runtime/session binding;
- a compare-and-swap revision;
- an optional bounded title;
- a dense ordered user/assistant transcript;
- `session_creation_pending` and `turn_pending` crash markers.

This gives one project multiple independent chats without pretending that one legacy project session
is every conversation.

## App-private storage boundary

Android/Linux local conversation files live under:

`<app-private>/vibecoder/state/conversations/`

The filename is the canonical pair:

`<project-uuid>--<conversation-uuid>.json`

The local adapter retains the Part-16 secure-file rules: fixed app-private descendants, no caller
absolute path authority, `O_NOFOLLOW`, app-UID ownership checks, private regular files, hard-link
rejection, bounded reads, temporary-file + rename publication, parent `fsync`, strict canonical UUID
spelling, bounded directory traversal, and compare-and-swap revisions.

Current bounds are 512 conversations per project, 4,096 messages per conversation, 256 KiB per
message, 12 MiB total message text, and 16 MiB encoded conversation state.

## Crash semantics

`session_creation_pending` is committed before creating a Jcode session. If the app dies after the
runtime side changes but before persistence finishes, the conversation remains visibly incomplete
and cannot be resumed as trusted authority.

`turn_pending` is committed after the user message but before inference. Session resume and that
pending commit run under the same project lifecycle gate used by rollback; backend execution then
reacquires the gate. Therefore a rollback cannot race an unmarked resumed turn during the handoff.
If the process dies while a model/tool turn may have changed the project, restart refuses to silently
continue that chat. Part 34 intentionally does not auto-clear the ambiguity. A later recovery UX must
show the interrupted state, reinspect/reconcile the workspace, and require an explicit recovery
decision.

## What is not claimed

The current Part-31 Android runtime inventory still reports Node/OmniRoute as not ready because the
Node Android executable and reviewed OmniRoute 3.8.50 bundle are not packaged in this source
checkpoint. Consequently this Part-34 source slice does **not** claim:

- a successful real AI reply in the APK;
- a local OmniRoute service round trip;
- a model/provider credential flow in the Android UI;
- a physical-device chat persistence/restart proof;
- general autonomous looping.

No dummy assistant, echo response, or hard-coded success path is added. The next runtime step is to
satisfy the existing Node/OmniRoute readiness contract, then wire the Android Alpha surface to these
Core APIs and test the real end-to-end path.

## Checkpoint rollback interaction

Part 17 rollback originally knew only the legacy single persisted project session. Part 34 extends
that boundary: before atomic workspace replacement, Core enumerates every persisted conversation,
rejects half-created sessions or pending turns, validates runtime identity, and rejects duplicate
conversation session ids. After the rollback commits, every conversation session is force-refreshed
through `AgentRuntime::refresh_session_after_workspace_replacement`. The unchanged path string is
never accepted as proof that Jcode still targets the replaced directory identity.


## Uninstall is not persistence

This store is intentionally under Android app-private storage. It survives ordinary process death,
app restart, and device reboot as long as Android retains application data. It does **not** make
projects/chats survive uninstall or **Clear storage**. Android removes the app-private data directory
in those cases. An uninstall-safe product needs a separate reviewed backup/export/restore boundary
(for example a user-selected Storage Access Framework destination and/or an authenticated encrypted
cloud backup). That future backup must never turn external storage into live workspace authority.
