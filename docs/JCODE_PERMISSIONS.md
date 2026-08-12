# Jcode permissions and capability negotiation — Part 5

## Reviewed upstream reality

The pinned Jcode 0.73.0 harness bridge advertises `sessions`, `streaming`, and several lifecycle/file
capabilities, but **does not advertise `permissions`**. Its own bridge source states that this path
never emits permission prompts and rejects `permission_response` requests. VibeCoder therefore
must not claim interactive Jcode permissions when connected to that exact bridge.

`RuntimeCapabilities.permissions` is now derived from the actual `hello` capability list. A future
or shared Jcode-compatible server that truthfully advertises `permissions` can use the permission
broker; the pinned bridge continues to report `false`.

## Permission request binding

A permission request is accepted only when it belongs to the currently verified session and active
connection generation. The broker keeps pending requests in memory and rejects:

- malformed/empty request ids;
- malformed tool/action names;
- oversized/invalid descriptions;
- duplicate request ids within one active turn;
- responses after the turn has completed;
- responses after reconnect/connection-generation change;
- responses for another session;
- double responses to the same request.

Pending requests are cleared when the owning turn ends or is abandoned.

## Decision mapping

Provider-neutral decisions are:

- `AllowOnce`
- `AllowSession`
- `Deny`

`AllowOnce` maps to Jcode's single-use `Allow`. `Deny` maps to `Deny`.

`AllowSession` is deliberately implemented **inside VibeCoder**, not as Jcode `AllowAlways`. The
reviewed harness schema exposes `AllowAlways` but does not define whether that grant is scoped to a
single agent session, process, project, or longer-lived policy. Mapping VibeCoder's narrower
`AllowSession` to an underspecified broader upstream decision would be unsafe.

Instead, after the current request is acknowledged with one upstream `Allow`, VibeCoder remembers
an in-memory exact-match grant for `(session id, action, description)`. A later permission request
with the same tuple is automatically answered using another single-use upstream `Allow`. Different
commands/descriptions still require a new decision. Grants are not persisted in this checkpoint.

## Fail-closed behavior

If a server emits a permission request without advertising `permissions`, VibeCoder denies the
request and best-effort cancels the turn. The same happens for malformed/replayed permission events,
if an automatic session-grant response cannot be acknowledged safely, or if there is no live event
consumer capable of receiving the interactive prompt. Callback panics are treated as failed delivery:
the request is denied and the turn is cancelled rather than left waiting indefinitely.

Malformed or oversized permission request ids are never reflected back in a permission response;
VibeCoder skips the deny packet and cancels the turn directly.

The pinned Jcode bridge currently cannot exercise the interactive permission path. This is not
papered over with fake capability flags. Later workspace/process isolation remains mandatory because
an agent runtime without permission prompts must still be physically constrained from escaping the
project/build sandbox.

## Cancellation concurrency

Permission-response preparation verifies the active turn, session binding, capability, and exact
connection generation, then clones the already-connected SDK handle. The session gate is released
before waiting for the permission-response acknowledgement. This is intentional: an explicit turn
cancel must remain independently deliverable even if a permission response stalls or races the
cancel. Reconnect/session-attachment mutation is already forbidden while the turn is active.
