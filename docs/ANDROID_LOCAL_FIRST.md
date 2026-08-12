# Android local-first execution policy — chosen before Part 8

VibeCoder's private version targets **Option A: one Android device, no mandatory remote build or
agent server**.

## What "local" means

The final Android installation may use more than one local process/service, but those processes run
on the same phone and communicate through loopback or other Android-local IPC. A local service is not
a rented/cloud server.

Target placement:

- Android UI: local on device. Part 28 adds a temporary diagnostic shell; the production UI remains deferred to the final UI stage.
- VibeCoder core/orchestrator: local on device.
- Jcode agent runtime and project/session state: local on device, subject to Android ARM64 runtime
  bring-up proving the reviewed Unix/socket assumptions work on Android.
- Project workspaces/files: local on device.
- OmniRoute-compatible model gateway: local on device and reached through loopback by default.
- Website tooling/preview: local on device.
- JDK/Gradle/Android SDK/build tools: local on device; their Android packaging/execution must be
  proven before the product claims local APK generation.
- Remote AI model APIs: network access is expected when Claude/OpenAI/Gemini/etc. are selected.
  A future local-model option may remove even this network dependency.

## No fake portability claims

The currently reviewed OmniRoute 3.8.50 application requires a supported Node runtime. The reviewed
Jcode integration also expects a Jcode executable plus Unix-domain socket support. Neither fact is
silently treated as "already solved on Android". The current backend contracts intentionally isolate
those runtime boundaries so later Android ARM64 packaging can either:

1. package compatible local runtimes, or
2. replace a runtime implementation behind the same VibeCoder contract if that is cleaner.

A mandatory remote/cloud backend is **not** the fallback architecture. A local incompatibility is a
concrete blocker to solve or replace.

## Why the OmniRoute adapter still accepts remote HTTPS

Remote HTTPS remains supported as a transport capability for development, diagnostics, or an optional
future mode. It does not change the product target. The example/default remains the local loopback
API root `http://127.0.0.1:20128/v1`.

## Local secret storage note

Part 10 removed the early `api_key_env` placeholder. The phone-local default is an application-level `credential_ref` using `app_secure_store`; process environment variables remain an explicit development/test source only.
The local loopback gateway should still be authenticated; "same phone" is not treated as equivalent
to "trusted caller".


## Part 10 phone-local secret path

The checked-in default uses `app_secure_store`, not an environment variable. The final Android adapter will back this contract with Android Keystore-protected app-private storage. Environment lookup remains an explicit dev/test source only and cannot resolve a secure-store reference.


## Part 11 local project storage

Project roots are now concretely phone-local. The Android/platform integration will pass one existing app-private directory to `LocalWorkspaceRuntime`; VibeCoder creates `vibecoder/projects/<ProjectId>` beneath it. No remote workspace service and no user-supplied absolute project root is required. Shared/external storage import/export remains a later explicit boundary rather than becoming the agent's working root.

## Part 14 local command authorization

Command authorization is also phone-local. `vibecoder-command-policy` does not send command requests
to a remote executor: it validates a structured request and issues a local allow-once envelope only
after policy/approval checks. Part 14 itself still spawns no child process; Part 15 now provides the
separate Unix/Android lifecycle executor while strong process isolation remains later work.

## Part 15 local process runtime

The phone-local design now has a real Unix/Android process-execution boundary. After explicit command
approval, VibeCoder can resolve a provisioned runtime tool beneath a distinct package-installed
executable-code root (or an explicitly allowed project-relative executable), start it with a
clean environment, capture bounded stdout/stderr, and cancel/timeout its process group. No cloud
build host is introduced by this layer.

This is source-level Android/Unix runtime design, not proof that the final Jcode/Node/JDK/Gradle/SDK
binaries are already packaged or executable on a physical Android device. Strong OS sandboxing also
remains later work.

## Part 16 app-private persistence

Project/session registry state remains on-device under the platform-supplied app-private directory.
Only stable ids and non-secret preferences are persisted. Restart never trusts a stored filesystem
root or live Jcode attachment; the workspace root is re-derived from ProjectId and the Jcode session
is reattached/corroborated before it regains authority.


## Part 26 Android W^X correction and package-code root

The earlier Part-15 wording that placed executable runtime tools under writable app-private
`vibecoder/runtime` was a host-Linux assumption and is no longer the Android design. Android apps
targeting API 29+ cannot directly execute files from writable app home. Part 26 splits writable
runtime data from package-installed code and requires explicit device evidence before Jcode, Node or
other native tools count as Android-ready. See `docs/PART26_ANDROID_RUNTIME_PACKAGING.md`.

## Part 27 Android host split

`vibecoder-android-host` is the first UI-free `cdylib` boundary intended for the Android shell. It
accepts app-private data, JNI-library, and child-executable roots separately. This matters because a
JNI library may be loaded directly from an APK while Jcode/Node need an actual package-owned
filesystem path for process execution. No writable-data copy or PATH fallback is accepted as a
substitute. Part 27 implements probes but does not claim that an APK or physical device has passed them.

## Part 28 diagnostic Android shell

A minimal `arm64-v8a` Android shell now exists under `android/`. It is a local diagnostic surface,
not the production UI. The shell reads the packaged runtime inventory, crosses a tiny JNI boundary,
and asks `vibecoder-android-host` for measured readiness. Missing native/runtime payloads remain
visible blockers. No remote backend is introduced by the shell, and this diagnostic APK source does
not yet request Internet permission; gateway networking is a later on-device integration step.

## Reviewed async and asset-evidence boundary

The Android host now owns an explicit current-thread Tokio runtime with I/O/time drivers for future synchronous JNI callers that need to drive async agent, gateway, or core operations. This avoids relying on an ambient runtime that the Android UI process does not create automatically. The executor rejects nested synchronous `block_on` use from an already-active Tokio context.

The diagnostic shell also measures `apk_asset` presence through `AssetManager` and passes that evidence through the Rust FFI v2 snapshot path. This closes the earlier structural gap where DataBundle/JavaArchive components could never receive package-presence evidence, while preserving separate service and runtime-binding proof requirements.
