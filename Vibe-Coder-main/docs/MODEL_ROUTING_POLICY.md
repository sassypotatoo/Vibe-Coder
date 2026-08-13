# Model routing policy — Part 9

Part 9 adds a provider-neutral, explicit model route policy. It does **not** execute inference or
silently let VibeCoder choose any model advertised by OmniRoute.

## Deterministic route order

A policy contains one primary model plus at most seven fallback models. Every model id is preserved
exactly as reported by the fresh credential-scoped gateway catalog. If a policy also pins a provider,
that provider must exactly match the catalog `owned_by` value. Missing models, provider mismatches,
duplicate route ids, duplicate fallback triggers, malformed identities, and ambiguous gateway
catalog ids fail closed.

OmniRoute combo aliases are filtered from VibeCoder's coding catalog because an `owned_by: "combo"` row can hide internal multi-target routing. Until combo targets can be inspected and pinned explicitly, accepting one would violate deterministic model identity.

VibeCoder never chooses a model outside this ordered policy:

1. primary
2. fallback 1
3. fallback 2
4. ...

A primary-only policy is valid. Default transient triggers simply remain dormant when there is no
fallback target.

## Safe fallback classes

Only these failure classes are eligible for automatic fallback:

- rate limited
- timeout
- provider unavailable
- model unavailable

A local gateway outage, authentication failures, access denial, invalid requests, cancellation, protocol errors, and unknown failures are never automatic-fallback reasons. A gateway outage cannot be repaired by choosing another model behind that same gateway.

The policy can disable any of the eligible transient classes. Configuring fallback targets with an
empty `fallback_on` list is rejected because it is almost certainly an accidental dead policy.

## Before-response-only boundary

Automatic fallback is currently allowed only while an attempt is pristine. Once assistant output or
any tool activity has started, VibeCoder stops instead of replaying the request on another model.

This matters for a coding agent: a model may have already started editing files or running a command
before a later transport failure. Replaying the same turn on a fallback model could duplicate or
contradict those side effects. Part 9 therefore encodes `before_response_only` as the only supported
automatic boundary.

## Catalog freshness

`VibeCoderCore::resolve_model_route_policy` obtains a fresh credential-scoped gateway catalog and
resolves the policy against it. Resolution returns concrete `ModelRef`s from that catalog, not guessed
aliases. Resolved policies and attempt-progress state are runtime-only and are not deserializable, so stale persisted data cannot bypass fresh-catalog validation. Attempt progress is monotonic and move-only: callers start at the policy primary, can only mark response/tool activity started, and receive the next attempt state from a successful fallback decision. They cannot construct or clone an arbitrary route index. Part 23 consumes this state through the authority-free backend task machine.

## Phone-local architecture

This policy is pure Rust application logic and does not require a remote routing server. For the
selected private architecture, the OmniRoute-compatible gateway remains a same-phone loopback
runtime; external network traffic is only for the configured AI providers.

## Agent catalog remains a separate trust boundary

A model resolved from OmniRoute's gateway catalog is **not** automatically treated as the same
identity in Jcode's session-scoped model catalog. Part 23 requires the exact id and a non-empty exact
provider in both fresh catalogs. It then selects the model and obtains a second fresh Jcode active
model/provider probe. Finally `run_turn` repeats fresh selection and post-switch verification just
before inference. Missing ids may advance only to the next explicitly configured fallback while the
attempt is pristine; ambiguous/mismatched provider identities fail closed.

## OmniRoute execution truth: runtime attestation is mandatory

Part 9 found that OmniRoute 3.8.50 can mutate a direct chat request's model inside its own
runtime. The confirmed paths include emergency budget fallback (enabled by default), guardrail
reroutes, pre-request hook overrides, task-aware routing, web-search routing, reasoning routing,
and auto/combo resolution. Therefore a catalog-resolved `ResolvedModelRoutePolicy` is only a
validated VibeCoder plan. It is **not** proof that the current OmniRoute runtime will execute that
exact model.

Part 23 completes the bundled exact-model profile. The hash-pinned patch rejects drift caused by
no-thinking/header/context/CC aliases, guardrails, pre-request hooks, task-aware routing, web-search
routing, reasoning routing, auto/combo resolution, connection defaults/rules, background redirects,
custom aliases, effort variants, family fallbacks, and emergency budget fallback. Credential refresh,
same-model account failover, and a retry that preserves the exact model remain allowed.

The patched runtime adds `GET /v1/vibecoder/runtime-profile`. The OmniRoute adapter strictly parses
and pins its schema, upstream version, profile id, digest, and exact-routing booleans. Core requests
this attestation before the fresh model catalog and refuses inference without it. The patch applicator
verifies the original archive/file SHA-256 values, applies exact zero-fuzz hunks, validates every
result hash before writing, and is idempotent only for the complete reviewed result.

This is application/runtime identity evidence, not strong process attestation. A malicious process
with the same app UID or control of loopback routing remains outside the claim until Android process
isolation and packaged-runtime identity are established.
