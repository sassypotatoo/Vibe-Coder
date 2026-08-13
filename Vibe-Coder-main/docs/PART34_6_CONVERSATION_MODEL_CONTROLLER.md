# Part 34.6 — Durable conversation controller to real OmniRoute model

Status: **source lane complete; real Android conversation turn pending the external Node/OmniRoute runtime proof**.

This slice connects the existing durable multi-chat controller to the provider-neutral one-shot
`ModelGateway::chat_completion` primitive added in Part 34.5. It is deliberately a model-only turn.
Jcode/tool execution remains on the pre-existing agent path and is not merged into this controller
until Part 34.7.

## Turn authority

One call to `run_persisted_model_conversation_turn` performs exactly one turn:

1. Re-open and verify the project and conversation under the project lifecycle gate.
2. Reject incomplete session creation or an already-pending turn.
3. Require the persisted chat to remain bound to the current Jcode runtime identity; the session is
   retained for the later tool bridge but is not invoked in this model-only turn.
4. Append the user message and persist `turn_pending=true` before any network inference.
5. Re-fetch the deterministic OmniRoute runtime profile and a fresh credential-scoped model catalog.
6. Require one unambiguous exact model id from that catalog.
7. Build a bounded contiguous recent-history suffix ending in the just-persisted user message.
8. Issue exactly one non-streaming `chat_completion` request. There is no VibeCoder retry, model
   fallback, Jcode tool call, or autonomous loop in this path.
9. Require response/request model identity agreement and a response small enough to persist.
10. Append the assistant response, clear `turn_pending`, and commit by conversation CAS.

If inference fails, the already-durable user message remains visible and the controller clears the
pending marker by CAS. If that cleanup cannot be committed, recovery stays fail-closed instead of
pretending the turn completed.

## Bounded context

The model-only controller sends at most 64 messages, at most 128 KiB per message, and at most
256 KiB of conversation text total. It selects a contiguous recent suffix and removes a leading
orphan assistant message if truncation lands between a user/assistant pair. The latest message must
remain the user message for the current turn.

Output is explicitly bounded to 1..8192 requested tokens, and the returned assistant text must fit
the existing 256 KiB persisted-message limit. Before the user message is committed, the controller
reserves one assistant-message slot and enough remaining conversation-text capacity for a worst-case
256 KiB persisted answer, so a successful inference cannot predictably overflow the durable schema.

## Credential handling

The primary API accepts the already-borrowed `GatewayCredential`. A companion `_resolved` API
resolves an optional `SecretReference` into a short-lived `SecretValue` and keeps the same
non-persistence guarantees. No credential is added to conversation state or debug output.

## Proof boundary

This source slice does **not** claim a physical Android model turn. The current runner still lacks
the real Node 24.19.0 Android + built OmniRoute production runtime chain. Therefore the authoritative
flags remain `real_android_conversation_turn_proven=false` and `fresh_rust_compile=false` here.
