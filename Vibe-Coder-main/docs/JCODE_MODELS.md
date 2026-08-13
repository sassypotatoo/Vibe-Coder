# Jcode model discovery and selection — Part 6

## Reviewed upstream behavior

The pinned Jcode 0.73.0 bridge implements `list_models`, `get_runtime_info`, and `set_model`. Its
`hello_ok.capabilities` list does **not** contain a dedicated `model_selection` capability token. The
bridge requires a session attachment for all three model operations.

## VibeCoder behavior

- Model discovery is always session-scoped: `list_models(session_id)`. `VibeCoderCore` exposes this as `list_session_models`, with `set_session_model` for selection.
- The adapter requires a verified session/project binding and exact current connection generation.
- Model catalogs are fetched through a **fresh sidecar API connection** to the same live Jcode socket; they are not persisted or reused across sessions.
- Exact model ids returned by Jcode are preserved verbatim, including any route/context suffix.
- Provider labels come from available Jcode runtime routes only when one provider can be identified
  unambiguously for that exact model id. Otherwise `provider=None`.
- Duplicate or malformed model ids fail closed rather than being silently filtered.
- `set_model` verifies that the id exists in a fresh catalog before asking Jcode to switch.
- If a caller supplies a provider, the provider must match unambiguous route metadata.
- After Jcode acknowledges the switch, VibeCoder calls `get_runtime_info` and verifies the same
  session reports the requested model active. A requested provider is corroborated again.
- Model changes are rejected while a turn is active.
- `RunTurnOptions.model` selects that model before the turn and leaves it as the session's persistent
  model. It is not an ephemeral one-turn override.
- `CreateSessionOptions.model` remains rejected because the reviewed Jcode API cannot atomically
  create a session with a model. Creating first and then failing a hidden model switch could orphan a
  session whose id was never returned to the caller.

## Capability reporting

`RuntimeCapabilities.model_selection` begins false. After a real `list_models`/model operation
succeeds, the adapter records the current connection generation as operationally verified. The flag
then reports true only while that exact connected generation remains active; reconnecting makes it
false until re-probed.

## Cross-session cache hardening

The reviewed Jcode 0.73.0 bridge stores `available_models`, `available_routes`, and current
model/provider on each API connection. Its attach path starts a new model probe but does not
synchronously clear the previous attachment's cache. More importantly, `note_models()` intentionally
keeps the prior `available_models` value when a fresh catalog contains an empty model list, and empty
route/model/provider fields may also be absent on the legacy event. Waiting for `ModelInfo` on a reused
connection is therefore not enough to prove that every cached catalog field belongs to the new session.

VibeCoder fails closed instead of guessing. Before every model discovery, model switch, or turn that
requests a model, the adapter keeps the manager-owned Jcode connection alive and opens a **fresh
sidecar API connection** to the same live socket. A new API client receives a fresh server-side
`BridgeState`. VibeCoder subscribes to target-session events **before** attaching the verified session
on that sidecar and waits (within the configured request timeout) for that session's post-attach
`ModelInfo`. Only after that fresh probe does it call `list_models`/`get_runtime_info` and authorize a
switch.

Keeping the owner connection alive is mandatory in the default private mode: when Jcode is launched
with no explicit `JCODE_HOME`, the SDK owns an ephemeral home and removes it when that launched owner
is dropped. Restarting/reconnecting the owner merely to clear a catalog cache could therefore destroy
the private runtime state. The sidecar avoids that failure mode while still starting with an empty
model cache.

For `set_model` and `RunTurnOptions.model`, the first fresh sidecar catalog is the authorization
source, but the actual `set_model` mutation is sent through the verified manager-owned connection that
will run the turn. VibeCoder then opens a **second fresh sidecar** and corroborates the active exact
model (and caller-supplied provider, when present) from fresh target-session runtime metadata. This
avoids treating provider/model fields retained on the reused owner bridge as post-switch proof. The
owner transport generation is rechecked after discovery and again after verification; if it changed,
the operation fails closed. The extra local socket work is intentional correctness-first isolation.

## Part 23 active-identity corroboration

Backend task execution now treats model selection and model identity as two separate checks. Before
each inference attempt, VibeCoder authorizes the requested `ModelRef` against a fresh sidecar catalog,
sets it on the manager-owned connection, and then uses another fresh sidecar to corroborate the active
model and provider. The owner transport generation is rechecked around this sequence so a reconnect
cannot silently carry authorization into a different runtime generation.

The selected `ModelRef` is also passed explicitly in `RunTurnOptions`. Immediately before inference,
the Jcode adapter repeats its fresh-catalog and active-identity checks. A missing provider, an identity
mismatch, stale session metadata, or a generation change fails closed rather than guessing from a
model name or retained bridge state.
