# Part 23 — End-to-end backend task orchestration

Part 23 connects the previously separate prompt, model-routing, Jcode, tool-event, and turn-result
boundaries. The public entry point is `VibeCoderCore::run_backend_task`; the state decisions live in
the authority-free `vibecoder-task-orchestration` crate.

## Required order

1. Core holds the project lifecycle permit, re-verifies the managed project/session, requires zero
   controlled processes, and checks agent quiescence.
2. The gateway adapter fetches the running deterministic-profile attestation. Static configuration
   and `/v1/models` are not accepted as execution proof.
3. Core resolves the explicit route policy against a fresh credential-scoped gateway catalog.
4. For the selected attempt, a fresh session-scoped Jcode catalog must contain the same exact model
   id and non-empty provider.
5. Jcode selects that model on the owner connection and returns model/provider observed through a
   second fresh target-session sidecar probe.
6. The task enters `Running`, and `run_turn` receives the same `ModelRef`; Jcode therefore performs
   another fresh catalog selection and active-identity verification immediately before inference.
7. Normalized events update monotonic attempt progress before they reach the optional caller event
   handler. A successful `TurnResult` becomes the Debug-redacted `BackendTaskOutcome`.

## Fallback and replay boundary

A missing selected id in Jcode's fresh catalog is `ModelUnavailable` and may advance to only the next
configured route while the attempt is pristine. Text deltas, background progress, and any tool start
or finish permanently block automatic replay. Cancellation, protocol/config/auth failures, provider
identity ambiguity, and unknown failures stop.

Jcode's current adapter error variant carries stable prose rather than a typed timeout/rate-limit
class. Part 23 deliberately maps such agent errors to `Unknown`; it never searches error strings to
manufacture an automatic-fallback reason. The state machine supports the existing safe typed route
classes when a later adapter contract can supply them honestly.

## Deterministic gateway profile

The complete OmniRoute 3.8.50 bundle patch is source-hash pinned. It rejects every audited
model-changing layer and publishes a fixed runtime-profile endpoint. The Rust adapter accepts only
the reviewed schema/version/profile/digest and exact-routing flags. Unpatched, partially patched,
wrong-version, malformed, redirected, oversized, or unavailable responses fail before inference.

The profile still allows same-model credential refresh, account failover, and request retry. These
operations may change credentials or transport attempts but not provider/model identity.

## Authority and confidentiality

The task state crate depends only on domain, routing, and UUID types. It has no network, gateway,
agent, workspace, process, command, checkpoint, persistence, or secret dependency. It stores no
prompt and its outcome Debug implementation withholds assistant text and tool output. Core revokes
project command approvals before and after every turn attempt.

The runtime profile is not cryptographic process attestation. Same-UID malicious process isolation,
loopback endpoint ownership, and Android packaging identity remain explicit runtime work; production
UI and the first full compile remain deferred.

## Part 24 contract coverage

Versioned fixtures now drive the strict profile interpreter, task-state transitions, and a
provider-neutral Core harness. They cover the exact success path, explicit pristine fallback,
gateway/Jcode id and provider mismatch, hidden-reroute rejection, duplicate catalog identities,
both cancellation result forms, prose-backed agent failure, active-process exclusion, event replay
guards, and command-authority invalidation around a turn. These are source-validated Rust test
targets in Part 24; Part 25 remains responsible for their first compiled execution.
