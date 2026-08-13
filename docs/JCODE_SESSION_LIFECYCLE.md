# Jcode session lifecycle — Part 3

Part 3 maps provider-neutral VibeCoder session operations onto the reviewed Jcode 0.73.0 harness
SDK without exposing Jcode protocol objects to the core.

## Supported in this checkpoint

- Create a Jcode session rooted at a verified project directory.
- Resume/reattach an existing Jcode session only after proving that its persisted working directory
  is the same canonical project root VibeCoder expects.
- Cancel the active turn for a session that has already been created or safely resumed by this
  process.
- Reattach a known session after a transport generation change before sending cancel.
- Serialize session attachment changes so one connection cannot race between two sessions.

Turn execution/streaming, permission responses, and model selection remain fail-closed until Parts
4, 5, and 6.

## Why attach replies are not enough for project verification

The reviewed Jcode harness bridge returns `ApiEvent::Attached` for create/attach, but its translated
`SessionInfo` currently contains `working_dir: None`. VibeCoder therefore treats the attach reply as
identity only and calls Jcode's public `list_sessions()` API for persisted metadata.

For **resume/reattach**, persisted working-directory metadata is mandatory, absolute, canonicalized,
and must equal the expected VibeCoder project root. Missing, duplicated, relative, inaccessible, or
mismatched metadata fails closed and closes the local SDK connection.

For a **newly created** session, VibeCoder already supplied the canonical working directory in the
create request. The reviewed Jcode bridge explicitly forwards that directory into the subscribe. A
new session record may still be mid-write, so creation uses `list_sessions()` as corroboration: if
working-directory metadata is available it must match, but a temporarily absent field does not make
a correctly-rooted new session fail nondeterministically.

## Session/project binding

Part 3 keeps an in-memory registry:

`session id -> project id + canonical project root + connection generation`

Only one session is considered attached on the single Jcode SDK connection at a time. A connection
reconnect changes generation, so the old attachment is not trusted. The next operation must reattach
and verify the session again.

Part 16 now persists only the stable project/session identity outside this runtime registry. The
in-memory map remains intentionally generation-scoped because connection attachment is live authority
and must never be restored from disk. On restart, core loads the persisted session id and calls
`resume_session`, which rebuilds this registry only after Jcode working-directory corroboration.

## Runtime readiness

The provider-neutral `AgentRuntime` now separates a passive `capabilities()` snapshot from active
`ensure_ready()`. `VibeCoderCore::preflight()` calls `ensure_ready()` so a disconnected Jcode adapter
can establish its real handshake before capability checks. This avoids treating "not connected yet"
as "feature unsupported" while preserving passive status queries that do not unexpectedly launch a
runtime.

## Cancellation rule

Jcode's `cancel` request is stateful and only valid for the session currently attached to the
connection. VibeCoder therefore refuses to cancel an unknown session id. For a known session after a
reconnect or session switch, it reattaches and re-verifies the project first, then sends cancel.

## Security decisions

- Project roots must exist, be directories, be absolute, canonicalizable, and UTF-8 before Jcode is
  allowed to create a session there.
- Incoming and upstream-returned Jcode session ids are limited to 128 ASCII alphanumeric/`_`/`-`
  characters before they reach stateful session operations.
- A session id is not treated as authorization to operate on a project.
- Existing in-memory bindings cannot be silently rebound to a different `ProjectId` or root.
- Provider/model overrides on session creation remain rejected until the dedicated model-selection
  mapping exists.
- Raw upstream error prose is still excluded from ordinary domain errors.
