# Jcode turn execution and streaming — Part 4

Part 4 maps Jcode's reviewed harness SDK turn surface into provider-neutral VibeCoder events.

## Execution model

1. A session must already be created/resumed and bound to its canonical project root.
2. Jcode must advertise both `sessions` and `streaming`.
3. Attachment/startup is serialized, then the long-running SDK `run()` call moves to a dedicated blocking worker thread.
4. A cloned SDK handle remains available for `cancel()` while the worker is waiting for model/tool output.
5. Exactly one turn may run on the single attached Jcode bridge connection at a time. While it is active, create/resume/reconnect/disconnect operations that could change the attachment are rejected before they touch Jcode.
6. The worker marks execution finished before delivering its result to the async caller. A turn-control gate serializes explicit cancellation against normal finish/drop cleanup, so one turn cannot be both independently completed and cancelled by racing tasks. Cancel requires the exact original connection generation and verified attachment; it never reconnects or reattaches underneath an active turn.
7. Explicit disconnect/reconnect is rejected while a turn is active.
8. If the async `run_turn` future is dropped, a lease uses the same control gate, performs best-effort synchronous upstream cancellation only when the worker is still running, then clears the local active-turn marker.

## Event mapping

Mapped: assistant text, message accepted, tool start/done, background progress, session status, token usage, permission request, and turn completion.

Deliberately not exposed: Jcode `ReasoningDelta` / `ReasoningDone`. VibeCoder does not depend on or surface provider-private chain-of-thought. Tool-input fragments and unrelated/unknown events are also ignored in this part.

## Result mapping

The final result preserves assistant text, normalized tool-call outputs/errors, token usage, and cancellation state. Raw provider reasoning is not copied into the result.

## Cancellation semantics

A cancel is recorded locally only after Jcode acknowledges the cancel request. This prevents an unsuccessful cancel attempt from causing a subsequent genuine provider failure to be mislabeled as user cancellation. If the turn finishes between that acknowledgement and the local mark, completion wins the race and the cancel call still succeeds.

Part 5 now mediates permission requests when the connected harness truthfully advertises `permissions`. The pinned Jcode 0.73.0 bridge does not advertise that capability. If any runtime emits a permission event without advertising support, the request is immediately denied and the turn is best-effort cancelled. Malformed/replayed permission events also fail closed. Part 6 now adds verified session-scoped model discovery/selection; turn-time model requests use that adapter path before execution.

Raw tool outputs remain transient runtime data in this stage. They are preserved for agent/build diagnostics but are not declared safe for persistence; Part 10/16 secret and persistence work must redact or classify them before storage.
