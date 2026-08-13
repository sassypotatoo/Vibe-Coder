# Part 34.7 — OmniRoute ↔ Jcode model/tool bridge

## Status

Source lane complete. A real Android model-driven Jcode tool turn is **not** yet claimed because the packaged Node 24.19.0 + OmniRoute runtime chain still needs external build/device proof, and this runner has no Rust/Cargo toolchain for a fresh compile.

## Authority chain

VibeCoder keeps model routing authority outside Jcode:

1. Core re-attests the running OmniRoute deterministic runtime profile.
2. Core resolves the configured exact model from the fresh OmniRoute catalog.
3. The Android Jcode runtime is launched as a private packaged instance with a fixed model gateway bridge to `http://127.0.0.1:20128/v1`.
4. Jcode's model catalog must report the exact model id through the `OpenAI-compatible` transport provider.
5. Jcode switches the session to the exact id and a fresh runtime-info probe corroborates the active transport provider.
6. Core keeps the OmniRoute catalog's upstream-provider identity as the selected `ModelRef`; transport-provider metadata is never confused with upstream ownership.
7. One Jcode turn runs. Tool events are observable. Existing routing fallback remains permitted only before observable response/tool progress; no Jcode-internal provider fallback is allowed.

## Reviewed Jcode source contract

Pinned Jcode authority is version `0.73.0`, archive SHA-256 `dd6efc76c253a4a5d9ea35ec640f80980b898f1f98a6db0671d0efefa8b141f2`.

The reviewed source confirms:

- localhost OpenAI-compatible model transport (`JCODE_OPENAI_COMPAT_API_BASE`),
- local/no-auth compatible mode,
- exact model selection through the harness SDK,
- harness capabilities including `sessions`, `streaming`, and `session_files`,
- built-in file-editing tools,
- `JCODE_TOOL_PROFILE=minimal`, whose reviewed set includes `bash`, `read`, `write`, `edit`, `multiedit`, `apply_patch`, `patch`, `agentgrep`, and `ls`,
- `JCODE_DISABLED_TOOLS` filtering.

VibeCoder therefore launches the Part 34.7 bridge with `JCODE_TOOL_PROFILE=minimal` and `JCODE_DISABLED_TOOLS=bash`. The resulting Part 34.7 surface is file-oriented only. `command_tools` remains false.

## Hidden inference disabled

The bridge launch environment additionally fixes/disables:

- `JCODE_OPENAI_COMPAT_API_BASE=http://127.0.0.1:20128/v1`
- `JCODE_OPENAI_COMPAT_LOCAL_ENABLED=1`
- `JCODE_OPENROUTER_ALLOW_NO_AUTH=1`
- `JCODE_RUNTIME_PROVIDER=openai-compatible`
- `JCODE_INITIAL_PROVIDER_EXPLICIT=1`
- `JCODE_OPENROUTER_NO_FALLBACK=1`
- `JCODE_AUTO_POKE=off`
- `JCODE_RUN_AUTO_POKE=0`
- `JCODE_AUTOREVIEW_ENABLED=false`
- `JCODE_AUTOJUDGE_ENABLED=false`
- `JCODE_TOOL_PROFILE=minimal`
- `JCODE_DISABLED_TOOLS=bash`
- `JCODE_NO_TELEMETRY=1`
- `NO_PROXY=no_proxy=127.0.0.1,localhost`

No arbitrary endpoint is persisted in the bridge configuration.

## Bounded single-turn execution

Pinned Jcode may perform multiple model/tool continuations inside one agent turn. VibeCoder therefore caps the Part 34.7 bridge at **32 observed tool starts per normal turn**. A 33rd tool start sets a fail-closed safety marker, synchronously cancels the Jcode session turn, and returns an agent error. This is an inner-turn safety bound, not the explicit multi-turn loop mode planned for Part 34.9.

## Permission boundary

The Jcode SDK blanket `auto_approve` path remains disabled. The reviewed 0.73.0 harness does not advertise the `permissions` capability; an unexpected permission request therefore fails closed through the existing VibeCoder adapter logic rather than being silently accepted.

## Capability boundary

`file_tools=true` is reported only when both are true:

- the VibeCoder OmniRoute bridge is configured, and
- the live Jcode handshake advertises `session_files`.

Unbridged/arbitrary Jcode runtimes do not gain this assertion. `command_tools=false` remains explicit for Part 34.7.

## Not claimed

- real Android model + Jcode tool round trip,
- command execution through the model bridge,
- fresh Rust compile in this runner,
- Node/OmniRoute Android runtime proof,
- autonomous looping.

Part 34.8 owns the first real agent action acceptance turn after the runtime/device gate is available.
