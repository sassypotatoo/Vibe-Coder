# OmniRoute authenticated health and model catalog — Part 8

Part 8 turns the hardened Part-7 transport into real `ModelGateway` semantics.

## Source truth from OmniRoute 3.8.50

The reviewed `GET /v1/models` route emits an OpenAI-style envelope:

- top-level `object: "list"`,
- top-level `data` array,
- model rows with `id`, `object: "model"`, and usually `owned_by`,
- optional `name`, `type`, and `supported_endpoints` metadata.

The same catalog can contain chat models and specialty-only embedding, image, audio, rerank,
moderation, video, or music rows. Some multi-capability rows may advertise chat/responses plus a
specialty type. The reviewed route may require API-key auth and returns HTTP 401 for missing/invalid
credentials when catalog auth is enabled.

The upstream `HEAD /v1/models` remains an unconditional availability probe and is not health truth.

## Credential lifetime

`GatewayCredential` is provider-neutral, borrowed, non-serializable, and Debug-redacted. Core passes
it into one gateway operation; the gateway does not retain it. Part 10 will resolve configured secret
references into this short-lived credential. This change prevents the old `ModelGateway::health()`
shape from forcing a concrete gateway to persist an API key merely so core can call it later.

## Health truth

`OmniRouteClient::health` uses bounded `GET /v1/models` with the supplied credential.

Ready means all of the following are true:

1. transport completed,
2. HTTP status is 200,
3. content type is JSON,
4. the response is a valid OpenAI list envelope,
5. at least one usable chat/responses model remains after filtering.

Stable health states distinguish authentication required, rejected credentials, access denial, rate
limiting, missing endpoint, invalid response, no usable model, and transport/server unavailability.
Raw server bodies and raw reqwest error prose are never copied into health detail.

## Coding-model filter

VibeCoder only exposes models usable for conversational/coding inference:

- `supported_endpoints` containing `chat` or `responses` => usable even when the row also has a
  specialty type,
- a missing/empty `supported_endpoints` list falls back to `type`,
- missing `type` or `type: "chat"` => usable,
- specialty-only types/endpoints => filtered out.

For usable rows, exact upstream `id`, optional `name`, and exact `owned_by` provider identity are
preserved. VibeCoder does not synthesize aliases or infer a provider from the model-id prefix.

Duplicate usable model IDs are rejected rather than picking one arbitrarily. Catalog and individual
text fields have explicit bounds, malformed JSON/envelopes fail closed, and successful non-JSON
responses are rejected.

## Phone-local target

For the selected Option-A architecture, this API is expected to be reached at a local loopback URL; the gateway and VibeCoder run on the same Android phone. Part 8 does not claim that OmniRoute's Node runtime is already packaged for
Android; that remains an explicit local-runtime bring-up problem, not a reason to introduce a
mandatory cloud server.

## Part 9 deterministic-routing hardening

The reviewed OmniRoute catalog can expose routing combos as model-shaped rows with
`owned_by: "combo"`. Those rows are now filtered from VibeCoder's coding catalog. A combo can hide
multiple model targets and an internal routing strategy, so treating it as an exact `ModelRef` would
violate the Part 9 explicit-route guarantee. Direct catalog models remain available unchanged.
