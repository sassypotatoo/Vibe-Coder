# Secret references and strict configuration — Part 10

## Persisted rule

VibeCoder configuration is not a secret store. Persisted JSON may contain only a `SecretReference` such as:

```json
{
  "source": "app_secure_store",
  "name": "omniroute.api_key"
}
```

The token itself must never be written into project/config JSON, checkpoints, logs, debug strings, or model-routing metadata.

## Phone-local production path

`app_secure_store` is the default private Android source. The Rust `AppSecureStoreBackend` contract is designed for an Android adapter backed by Android Keystore-protected app-private storage. That platform adapter is not falsely marked implemented in Part 10.

`environment` exists only for explicit development/testing through `EnvironmentSecretResolver`. An environment resolver refuses `app_secure_store` references; there is no silent fallback from the secure phone store to process environment variables.

## Resolved values

`SecretValue` is deliberately non-serializable and non-cloneable. Its `Debug` representation is `[REDACTED]`. It has an 8 KiB bound and uses `zeroize` when dropped. This reduces retention of the owned buffer; it does not claim to erase copies that an OS, HTTP stack, or remote provider may make.

The core resolves a reference immediately before a gateway operation, borrows the UTF-8 value into `GatewayCredential`, awaits the request, and then drops the local `SecretValue`. OmniRoute transport configuration no longer stores even the reference.

## Config loader

`vibecoder-config` accepts at most 256 KiB JSON and returns stable sanitized errors. It rejects:

- malformed JSON;
- duplicate object keys;
- excessive nesting;
- unknown typed fields;
- common plaintext credential field names such as `api_key`, `password`, `access_token`, `bearer_token`, `client_secret`, and `private_key`;
- invalid secret references;
- invalid Jcode/OmniRoute/routing configuration.

Part 11 owns canonical workspace-root creation and containment.
