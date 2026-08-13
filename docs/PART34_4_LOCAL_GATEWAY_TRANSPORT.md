# Part 34.4 — Android-local OmniRoute gateway transport

## Status

Source lane complete. Physical Android catalog round-trip and the first model request remain unproven.

## Scope

Part 34.4 connects the already hardened `vibecoder-gateway-omniroute` adapter to the exact OmniRoute service session started by the Android host. It deliberately stops before inference.

The fixed API root is:

`http://127.0.0.1:20128/v1`

No caller-supplied gateway URL is accepted by the Android diagnostic transport path.

## Transport proof

Before the catalog probe, the host requires the supervised OmniRoute process to still be active and requires its Part 34.3 runtime attestation to match the pinned upstream/version/profile. The transport then re-fetches the exact runtime profile through the hardened HTTP adapter and performs credential-scoped `GET /v1/models` health discovery.

The sanitized result records only bounded state such as credential mode (`anonymous` or `ephemeral_bearer`), health classification, usable model count, and stable error codes. Bearer bytes are borrowed for one call and are never serialized into the diagnostic response.

`401` with anonymous access is a valid transport classification (`authentication_required`), not proof of a usable model catalog. A ready catalog and an auth-required catalog can both prove that the local HTTP round trip reached OmniRoute; neither is treated as inference.

## Android boundary

The diagnostic APK now declares `android.permission.INTERNET`, which Android also requires for socket networking to loopback. The Rust client still rejects remote plaintext HTTP, redirects, ambient proxies, URL credentials, query/fragment injection, and oversized responses.

JNI exposes a one-shot gateway probe. It intentionally does not implement the usual null-buffer size-query pattern because repeating a network probe could observe different state or consume quota in future authenticated variants. Diagnostic credential input is capped at 8192 bytes.

## Explicitly not claimed

- No Android secure-store implementation is claimed.
- The diagnostic shell uses anonymous access only.
- No `/chat/completions` or `/responses` inference request is sent in Part 34.4.
- No model token usage is claimed.
- No physical Android gateway/catalog proof is claimed until the real Node/OmniRoute APK lane runs on device.

Part 34.5 owns the first real model request.
