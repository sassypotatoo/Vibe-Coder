# Part 16 — Project/session persistence

Part 16 adds an app-private persistence boundary for project identity and resumable agent metadata.
The physical workspace root is **not persisted as authority**. A loaded `ProjectId` is reopened through
`WorkspaceRuntime::open_project`, which derives and re-verifies the canonical app-private path.
Likewise, a persisted `SessionId` is only a resume hint; `AgentRuntime::resume_session` must
corroborate the current runtime's project working directory before the session becomes live authority.

## Stored state

Each project has one bounded JSON record under the fixed internal directory
`vibecoder/state/projects/<project-uuid>.json`. The record may contain:

- schema version and monotonic revision,
- `ProjectId`,
- crash-safe session-creation pending marker,
- agent runtime id + session id,
- preferred exact model id/provider,
- explicit model route policy.

The schema has no fields for API keys, secret values, project absolute roots, command approvals,
process handles/output, tool output, prompts, reasoning, resolved model catalogs, or connection
generations. Unknown JSON fields are rejected by typed deserialization.

## Crash and concurrency behavior

State files are bounded to 256 KiB and are replaced by private temp-file + `fsync` + `renameat` +
parent `fsync`. Reads and writes re-enter the fixed state directory with directory fds and
`O_NOFOLLOW`; state symlinks, hard links, wrong-owner files, and group/other-readable state files are
rejected. A process-local mutation gate serializes store operations.

Every state record has a monotonic `revision`. Updates use compare-and-swap semantics; a stale
revision fails with `project_state_revision_conflict` instead of silently overwriting a newer update.

Session creation first commits `session_creation_pending=true`. Only after Jcode returns a session id
is the final binding committed. If the app dies during that window, restart sees an incomplete state
and refuses automatic resume. Recovery/reconciliation of such upstream orphan sessions is later
reliability work; Part 16 does not invent a session id.

## Model preference semantics

The persisted model is a **preference**, not proof of the currently active Jcode model. Applying it
still goes through Jcode's fresh session/catalog/provider verification. Persisted route policy is also
configuration only; resolved gateway routes remain runtime-only and must be rebuilt against a fresh
catalog.

## Deliberate limitations

- The Android platform adapter must still supply the real app-private directory.
- Same-UID hostile cross-process mutation is not claimed isolated before later process sandbox work.
- An upstream Jcode session can exist if the final persistence commit fails after successful creation;
  the pending marker keeps restart fail-closed, but automatic orphan reconciliation is not Part 16.
- Checkpoint implementation is separate: Part 17 stores snapshot/rollback data outside this project/session state schema.
