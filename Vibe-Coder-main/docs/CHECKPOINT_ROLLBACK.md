# Part 17 — Project checkpoints and rollback

## Storage and authority

Checkpoints live under the Android app-private root at `vibecoder/checkpoints/<project-id>/<checkpoint-id>/`. Each published checkpoint contains `metadata.json` plus an immutable `tree/` copy. Physical paths are derived by the local adapter; a checkpoint id or persisted metadata record is never accepted as a caller-selected filesystem path.

## Snapshot publication

The local store pins traversed Android/Linux directories with `O_DIRECTORY | O_NOFOLLOW` and walks through live directory descriptors. Regular files must have one link. Symlinks, hard-linked regular files, special files, invalid/control path components, and VibeCoder internal temporary namespaces fail closed. The store uses source/copy/source corroboration: it hashes the live source before copy, computes the copied-tree digest while copying, independently re-hashes the copied tree, then hashes the live source again. Publication requires the pre-copy source, copied tree, copied-tree recheck, and post-copy source to agree. The checkpoint is renamed from an unpublished temporary directory to its final UUID name only after that agreement.

Limits are explicit: at most 64 checkpoints per project, 100,000 regular files per checkpoint, 4 GiB total regular-file bytes, and traversal depth 128. ENOSPC or any copy/integrity failure leaves no published checkpoint.

## Rollback

The checkpoint itself is not exchanged or consumed. It is first cloned into a reserved sibling under `vibecoder/projects`. Android/Linux then performs `renameat2(..., RENAME_EXCHANGE)` between the live project name and that staging name. The live project path therefore never has a missing-name window. There is deliberately no unsafe multi-rename fallback.

After exchange the projects directory is synced before the replacement is accepted. A sync failure attempts an immediate exchange-back and parent sync; a failed recovery is surfaced as `checkpoint_rollback_recovery_failed`. The restored live tree is then re-hashed, and a mismatch also triggers exchange-back. Once exchange, parent sync, and restored-tree verification have succeeded, rollback is committed. Failure to delete the old tree from the reserved staging name is cleanup debt rather than a false rollback failure; initialization will retry canonical rollback-staging cleanup.

## Cross-component quiescence

Core uses a project-scoped lifecycle permit so checkpoint/rollback cannot race a new local process start, direct workspace mutation, project removal, or session creation/resume for the same project. It also blocks checkpoint creation and rollback while the configured local process runtime already reports an active process or Jcode reports an active turn. Before rollback it invalidates the project's command-authorization epoch, revoking pending requests and making already-issued but not-yet-started envelopes stale.

After a committed rollback Core advances the project authorization epoch again so approvals issued during the rollback window cannot survive into the restored workspace. The workspace is then reopened from `ProjectId`. If a persisted Jcode session exists, the adapter clears cached attachment state, so the persisted session is forcibly reattached using a fresh `attach_session` plus `list_sessions` working-directory corroboration. The unchanged path string is not treated as directory identity.

## Explicit limitations

Part 17 is not a kernel sandbox. A separately malicious same-UID process can still race app-private filesystem operations or detach descendants from normal lifecycle control. Full process isolation and end-to-end orchestration serialization remain later work. Android ARM64 packaging/execution is also not claimed before the scheduled runtime/compile stages.
