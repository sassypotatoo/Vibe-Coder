# Part 34.8 — first bounded agent action turn

## Status

Source lane complete. A real Android model-driven workspace mutation is **not** yet claimed because
packaged Node 24.19.0 + the sealed OmniRoute Android runtime still need external build/device proof,
and this runner does not provide Rust/Cargo for a fresh compile.

## Purpose

Part 34.8 is the first controller path where one persisted user request is allowed to cause a real
Jcode file mutation. It is still one outer turn:

`user -> durable pending marker -> OmniRoute/Jcode turn -> bounded file tools -> final answer -> STOP`

No automatic outer retry/continue loop is introduced here. Explicit multi-turn looping belongs to
Part 34.9.

## File-tool authority

The reviewed Jcode 0.73.0 bridge remains in `minimal` tool profile with `bash` disabled. The only
accepted Part-34.8 tool names are:

- `read`
- `write`
- `edit`
- `multiedit`
- `apply_patch`
- `patch`
- `agentgrep`
- `ls`

Command tools remain disabled. In addition to the launch environment restriction, the Jcode adapter
now checks every live `ToolStart` against the exact allowlist and synchronously cancels the turn if an
unexpected tool appears. The existing 32-tool-start cap remains active.

## Durable/safety sequence

1. Validate the prompt and require bridged file-tool capability with command tools still false.
2. Re-open and verify the project + persisted conversation/session binding.
3. Require process/agent workspace quiescence.
4. Create an immutable `BeforeAgentChange` checkpoint.
5. Persist the user message and `turn_pending=true` before model/tool activity.
6. Run exactly one existing backend task through the attested OmniRoute -> Jcode bridge.
7. Independently observe tool start/finish events while still forwarding presentation events.
8. Require a bounded, internally consistent tool transcript from the final `TurnResult`.
9. Require at least one successful file tool and at least one successful mutation tool.
10. Create an ephemeral `AgentChangeVerification` checkpoint and require its project-tree SHA-256 to
    differ from the pre-action checkpoint. A tool claiming success without an actual tree change is
    not accepted as an action.
11. Persist the non-empty final assistant response and clear `turn_pending`.
12. Best-effort remove the two temporary checkpoints.
13. Return. There is no outer continuation.

## Failure rollback

Any backend failure/cancellation, invalid tool transcript, missing successful mutation, post-action
verification failure, unchanged workspace, or assistant-persistence failure enters the same recovery
boundary:

- keep `turn_pending=true` armed while recovery runs,
- roll the whole project back to the immutable pre-action checkpoint,
- refresh persisted Jcode session bindings through the existing rollback path,
- clear the pending marker by CAS only after rollback succeeds,
- remove temporary checkpoints best-effort.

If rollback itself fails, `turn_pending=true` was never cleared, so the ambiguous workspace cannot be treated as a clean conversation on the next turn.

## Tool transcript acceptance

The controller requires:

- 1..32 final tool calls,
- safe bounded call ids,
- only the exact file-tool allowlist,
- unique call ids,
- one observed start and one matching observed finish per final tool result,
- matching tool name and success/error state between live events and final result,
- at least one successful mutation from `write`, `edit`, `multiedit`, `apply_patch`, or `patch`,
- a non-empty persistable final assistant response.

Raw tool output is not persisted into conversation state.

## Not claimed

- fresh Rust compile in this runner,
- physical Android model -> Jcode -> file mutation proof,
- command/shell execution through the model,
- autonomous multi-turn looping,
- final Alpha UI.

Part 34.9 owns explicit bounded loop mode. Part 34.10 owns the minimal Alpha UI/device acceptance
surface after the external runtime gates can be exercised.
