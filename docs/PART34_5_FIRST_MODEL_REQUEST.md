# Part 34.5 — first exact-model request through local OmniRoute

## Status

Source lane complete. A real Android model response remains pending the external Node 24.19.0 +
OmniRoute production bundle/device acceptance chain.

## Scope

Part 34.5 adds the first reusable model-inference operation to the provider-neutral gateway contract
and the OmniRoute adapter. The request is intentionally narrow:

- exact model identity selected from a fresh credential-scoped `/v1/models` catalog;
- runtime-profile attestation is re-checked immediately before catalog/inference;
- one `POST /v1/chat/completions` request;
- `stream=false`;
- text-only system/user/assistant history;
- no tool declarations or tool-call acceptance;
- explicit bounded `max_tokens`;
- no VibeCoder retry or fallback to another model;
- bounded JSON request/response bodies and stable error codes only.

This is the first inference primitive, not the conversation-controller integration. Part 34.6 owns
turn persistence and user-visible response delivery. Streaming belongs with that controller/event
integration rather than being falsely claimed by this one-shot diagnostic.

## Privacy boundary

The Android physical proof accepts an exact model id and uses a fixed benign diagnostic prompt.
Neither the prompt nor returned assistant text is included in the persisted diagnostic JSON. The
report records only bounded proof metadata: exact requested/observed model identity, response byte
count, finish reason, token usage when supplied by OmniRoute, and whether exactly one inference call
was issued.

The local gateway bearer credential remains borrowed and ephemeral. The diagnostic APK mode uses
anonymous local-gateway access only; provider-account configuration remains owned by OmniRoute.

## Exact-model proof

The host refuses inference unless the running service still carries the pinned VibeCoder routing
attestation and the requested model id exists exactly in a fresh usable catalog. If the OpenAI-style
response includes `model`, a different observed id fails the proof rather than being silently
accepted. The VibeCoder client itself performs no automatic retry/fallback in this step.

## Explicitly not claimed

- No real model response has been produced in this runner.
- No streaming response path is claimed yet.
- No tool call or Jcode action is permitted by this inference operation.
- No Android secure-store implementation is claimed.
- No controller/conversation persistence integration is claimed; that is Part 34.6.
