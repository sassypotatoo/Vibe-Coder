# Part 34.9 — Explicit bounded agent loop

Status: **Source lane complete; real Android explicit-loop execution remains unproven.**

## Purpose

Normal VibeCoder behavior remains single-shot: one user request produces one model/agent turn and
then stops. Part 34.9 adds a separate, one-shot `ExplicitAgentLoopGuard` that must be constructed by
the caller only after the user explicitly asks for repeated work such as "keep fixing until the
requested change is complete". The normal Part 34.8 action API contains no outer loop.

## Bounds

The default explicit policy is 4 outer turns and 96 total observed Jcode tool calls. The hard source
limits are 8 turns and 256 total tool calls. Every inner Jcode turn still retains the Part 34.7/34.8
32-tool-call ceiling. A guard is scope-bound to one project + conversation and is one-shot.

The loop also tracks verified project-tree SHA-256 values. Reaching the configured repeated-tree
occurrence threshold stops the loop instead of allowing an A -> B -> A workspace oscillation to run
until the turn budget is exhausted.

## Durable conversation semantics

The original user message is committed exactly once and `turn_pending=true` remains durable across
the complete loop. Intermediate assistant responses are not written into conversation history.
Only the final completion response, or a deterministic stop message after a safe rollback, is
persisted before the pending marker is cleared.

## Iteration acceptance

Each iteration still uses the Part 34.8 live Jcode file-tool observer and exact file-tool allowlist.
The model must finish with exactly one machine-control line:

- `VIBECODER_LOOP_STATUS=complete`
- `VIBECODER_LOOP_STATUS=continue`

The marker is removed before any assistant text is persisted. A malformed, duplicated, or missing
marker fails closed.

`continue` requires at least one successful mutation tool and a different verified project-tree
SHA-256 from the immediately previous iteration. `complete` still requires at least one successful
file tool, but may be read-only after inspection when the requested state is already satisfied.

## Checkpoint and rollback behavior

An immutable pre-loop checkpoint is created before the first inner turn. Any protocol/error failure rolls the
whole workspace back to that checkpoint. Cancellation, turn-budget exhaustion, tool-budget
exhaustion, or repeated workspace state are normal non-success terminal reasons and also restore the
pre-loop tree before clearing `turn_pending`.

Part 34.9 also repairs a Part 34.8 recovery bug: the public rollback API correctly rejects every
pending conversation, so a failed action turn could not use it while its own crash marker was armed.
Core now has a private rollback path that permits exactly one nominated pending conversation only
when its persisted project, conversation id, runtime id, and session id still match. Any other
pending chat continues to block rollback. This bypass is not exposed to callers.

## Cancellation

`request_explicit_agent_loop_cancel` prevents any later iteration. `cancel_active_explicit_agent_loop_turn`
sets that same flag first and then corroborates the persisted project/session binding before asking
Jcode to cancel the active turn. A racing successful turn therefore cannot authorize another loop
iteration after cancellation was requested.

## Current capability boundary

This source slice deliberately keeps Jcode `bash`, browser, and MCP tools disabled. Completion is
therefore based on live file-tool inspection and mutation evidence, not an external compile/test verifier. Command/test-driven loop completion must be added only after the command-tool permission
boundary is wired; Part 34.9 does not pretend a file inspection is a real build/test pass.

`auto-poke`, `autoreview`, and `autojudge` remain disabled. The explicit VibeCoder loop controller,
not hidden Jcode autonomy, owns continuation and stop decisions.

## Physical proof still pending

No source-only validator can prove the real Android Node 24.19.0 + OmniRoute + Jcode runtime path.
`real_android_explicit_loop_proven=false` and the fresh Rust compile flag remain false until that
external build/device gate is run.

Next source stage: **Part 34.10 minimal Alpha UI and acceptance wiring.**
