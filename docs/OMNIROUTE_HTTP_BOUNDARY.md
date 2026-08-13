# OmniRoute HTTP boundary — Part 7

Part 7 adds only the transport/configuration boundary. It does **not** yet claim that OmniRoute is
healthy or map `/v1/models` JSON into VibeCoder models; those semantics are Part 8.

## Reviewed upstream facts (OmniRoute 3.8.50)

- The public OpenAI-compatible API is reachable under `/v1` (Next.js rewrites it to `/api/v1`).
- `GET /v1/models` returns the OpenAI-style model catalog and may require a valid client API key.
- Client API keys are accepted as `Authorization: Bearer <key>`.
- `HEAD /v1/models` is an unconditional 200 availability probe in the reviewed source. It does not
  validate Bearer authentication or prove that catalog access succeeds.
- OmniRoute's rewrites accept accidental `/v1/v1/...` paths, but VibeCoder deliberately does not rely
  on that forgiveness. A configured API base is canonicalized to one `/v1/` suffix.

## URL policy

`OmniRouteConfig` rejects:

- relative or hostless URLs,
- schemes other than HTTP/HTTPS,
- username/password user-info,
- query strings and fragments,
- port 0,
- endpoint URLs such as `/v1/models` instead of an API root,
- ambiguous duplicate-slash base paths,
- plain HTTP to any non-loopback host.

A bare origin such as `https://router.example` becomes `https://router.example/v1/`. A reverse-proxy
subpath is allowed when the configured path already ends in `/v1`, for example
`https://router.example/omniroute/v1`.

Plain HTTP is accepted only for loopback (`localhost`, IPv4 loopback, or IPv6 loopback). Remote
OmniRoute services require HTTPS.

## Authentication boundary

As of Part 10 the transport client stores **neither a secret value nor a secret reference**. Persisted `credential_ref` lives in the application config/secrets layer and is resolved only for one request.

When a caller already has ephemeral auth material, `RequestAuth::Secret(&str)` (the transport alias of `GatewayCredential::Secret`) can attach it as a Bearer token to one
request. The token:

- is borrowed rather than stored in the client,
- is not serializable,
- is redacted from `Debug`,
- is shape-validated before becoming an HTTP header,
- is never sent by the unauthenticated HEAD availability probe.

## HTTP transport policy

The reqwest client:

- uses bounded connect/overall timeouts,
- follows no redirects,
- disables environment/system proxies,
- sends a fixed VibeCoder user-agent,
- reads response bodies incrementally with a hard maximum size,
- converts transport failures to stable error codes without persisting raw reqwest error text.

Disabling redirects prevents a configured endpoint from forwarding a Bearer request to a second
location. Disabling implicit proxies prevents environment proxy settings from silently becoming an
additional credential-bearing network hop. Explicit proxy support, if ever needed, must be a future
reviewed configuration rather than ambient process state.

## Part 8 semantic layer

Part 8 now implements `ModelGateway` on top of this transport using credential-scoped
`GET /v1/models`. Status/content type/JSON envelope are validated and only chat/responses-capable
entries map into provider-neutral `ModelRef` values. See `OMNIROUTE_CATALOG.md`.

## Part 23 runtime profile

The adapter now reads `GET /v1/vibecoder/runtime-profile` through the same bounded, authenticated HTTP
boundary. The response must match the pinned OmniRoute version, execution-profile identifier, profile
digest, and exact-model guarantees compiled into VibeCoder. Unknown JSON fields, redirects, oversized
bodies, inconsistent identifiers, or any disabled exact-model guarantee fail closed.

This endpoint attests the installed deterministic-routing patch contract. It is checked before every
backend task attempt and complements the fresh catalog and Jcode active-identity checks; it does not
replace either one.
