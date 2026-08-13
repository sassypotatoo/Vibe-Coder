# Jcode transport and connection lifecycle — Part 2

## Upstream seam

VibeCoder uses Jcode's intentionally public `jcode-sdk` + `jcode-harness-api` boundary. Only those
crates are vendored from the pinned Jcode 0.73.0 archive. Jcode TUI, internal protocol, provider,
and agent-runtime implementation crates are not copied into the product tree.

The harness API is NDJSON over a transport and currently advertises protocol major version `1`.
`JcodeClient` performs the mandatory `Hello` handshake and exposes server identity plus additive
capability strings. VibeCoder does not recreate that wire protocol.

## Supported lifecycle modes

### Private (default)

`JcodeClient::launch` starts an SDK-owned private Jcode runtime with an isolated state/runtime
location. This is the preferred base for later project isolation. Part 2 intentionally leaves the
session working directory unset; Part 3 will map a verified VibeCoder project root into session
creation rather than allowing transport configuration to bypass workspace containment.

### Shared

`JcodeClient::connect` can attach to a shared runtime. Autostart is allowed only with Jcode's default
socket. The current upstream SDK's autostart path starts the default socket, so VibeCoder rejects
`custom socket_path + ensure_runtime=true` instead of starting one runtime and dialing another.

## State machine

```text
Disconnected
    |
    | connect
    v
Connecting ----failure----> Faulted
    |                          |
    | handshake success       | retry/reconnect
    v                          |
Connected <-------------------+
    |
    | explicit disconnect
    v
Disconnected

Connected --reader/socket closes--> Faulted(retryable)
```

Each successful connection increments a monotonic `generation`. Later asynchronous work can pin the
generation it started on and refuse to apply a result from a stale connection after reconnect.

## Ownership rule

Private mode defaults to `inherit_logins=false`; later secret/provider work must opt in explicitly rather than silently copying host credentials.

The raw `JcodeClient` is private to `vibecoder-agent-jcode`. Public callers receive only sanitized
connection snapshots. This prevents external code from cloning the SDK handle and accidentally
keeping a private runtime alive after VibeCoder reports it disconnected.

## Failure normalization

Jcode SDK stable error codes are classified into:

- retryable transport;
- runtime unavailable;
- protocol mismatch;
- invalid configuration;
- remote failure;
- fatal transport.

Raw Jcode SDK error prose is also excluded from persisted lifecycle state because startup failures may contain captured process stderr or host paths. Persisted failures keep only the stable SDK code, a VibeCoder-owned generic message, class, and retryability.

Provider secrets and request payloads are not part of the persisted connection state.
