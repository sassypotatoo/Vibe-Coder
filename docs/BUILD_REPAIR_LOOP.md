# Part 22 — Build repair/rebuild loop guards

Part 22 adds bounded control around the Part-21 one-turn repair boundary. The guard is authority-free:
it cannot edit files, approve commands, start processes, create checkpoints, or call a model. Core keeps
all authority and uses the guard only to decide whether another repair/rebuild step is allowed.

## Retry budget

The default policy allows at most **3 repair attempts** for one loop. Configuration is bounded to
1–8 attempts. Reaching the budget stops before another checkpoint or agent turn is created.

## Repeated-error stop

Part 21 already produces a deterministic SHA-256 failure fingerprint that excludes BuildId. Part 22
tracks consecutive occurrences. The default stops on the **second identical fingerprint**, before a
second repair turn for the same unchanged failure. The occurrence limit is configurable from 2–4.
A different fingerprint resets the consecutive-repeat counter but does not reset the global retry budget.

## State machine

`build result -> guarded repair -> fresh rebuild preparation -> build result`

Only terminal build results are accepted. Success, cancellation, and timeout stop the loop. Failed builds
may authorize one move-only repair permit. A repair permit cannot be replayed. A completed non-cancelled
repair allows one move-only rebuild permit; Core then performs a fresh Part-19 inspection and prepares a
new Part-20 website pipeline. That pipeline still requires Part-14 allow-once approval for every stage.

## Cancellation

The loop owns a cloneable atomic cancellation signal. Requesting loop cancellation immediately invalidates
outstanding project command approvals and prevents later guarded repair/rebuild boundaries. Core also
provides explicit helpers to cancel an active Jcode repair turn or the active guarded website rebuild
process. A naturally completed build result is observed before the cooperative cancellation flag, so a
concurrently completed success is not relabeled as cancelled.

## Non-goals

- The guard is not durable state and is not persisted across app restarts.
- It does not bypass command approval.
- It does not auto-select a new model or route.
- It does not provide a kernel sandbox.
- Android ARM64 runtime execution remains unproven until later integration/testing.
