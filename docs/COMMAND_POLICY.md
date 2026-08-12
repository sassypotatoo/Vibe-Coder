# Part 14 — command request policy and execution envelope

Part 14 adds command authorization only. It does **not** spawn a process. Part 15 now consumes this
boundary and supplies the separate local process lifecycle, output capture, cancellation, timeout,
runtime-tool resolution, and execution-time filesystem checks described in `PROCESS_EXECUTION.md`.

## Structured command requests

Commands are represented as a program plus an argument vector and a project-relative working
directory. There is no generic shell command string field.

Two program authorities exist:

- `RuntimeTool { tool_id }`: an opaque id that Part 15 resolves through a trusted VibeCoder
  runtime-tool registry. Part 26 binds actual native runtime tools to package-installed code rather than writable app data; the id is never treated as a PATH lookup or an absolute path.
- `WorkspaceExecutable { relative_path }`: a project-relative executable such as `gradlew`. Part 15
  resolves and reverifies this path under the managed project root immediately before execution.

Absolute paths, `..`, backslash separator ambiguity, control characters, oversized path components,
and VibeCoder's internal temporary namespace are rejected. Arguments are bounded to 64 entries,
4096 bytes each, and 32 KiB total; control characters and Unicode bidi-direction controls are rejected so approval surfaces cannot be
spoofed with hidden newlines/tabs or direction overrides.

Common shell interpreter runtime ids such as `sh`, `bash`, `cmd.exe`, `powershell`, and `pwsh` are
rejected even if someone tries to place them in the trusted runtime-tool allowlist. This prevents
the runtime-tool contract from quietly degenerating into `shell -c <model string>`. This does not
make arbitrary programs harmless: `node -e`, a project script, Gradle tasks, and many other tools can
still perform powerful actions. That risk belongs to explicit approval plus later strong
process/runtime isolation; Part 15 provides lifecycle control but deliberately does not claim a
kernel sandbox.

## Fail-closed policy

`CommandPolicyConfig::deny_all()` is the default used by the backwards-compatible core constructor.
The phone runtime must explicitly provide a policy that names allowed runtime-tool ids and whether
workspace executables are even eligible to request.

Eligibility never means automatic execution. Every eligible request becomes a pending approval and
requires exactly one user/application decision: `allow_once` or `deny`. There is no Part 14
"always allow" grant. Pending state is memory-only, limited to 64 globally and 8 per session, and
can be revoked when a session/turn is abandoned. Request-id insertion is collision-safe and never
replaces an existing pending request. Part 14 binds approval to session + project, not
yet to a Jcode connection/turn generation; later orchestration must revoke pending requests when a
turn ends or is cancelled until that stronger binding is added.

Requests are bound to both `SessionId` and `ProjectId`. A decision from another scope fails closed
without consuming the rightful request. For `allow_once`, the approval object returned by the UI/IPC
must echo the exact validated command retained by the broker; a modified command payload is rejected.
Execution still uses the broker-retained command rather than trusting returned display data. A deny
decision is allowed to clear the correctly scoped request even if its display payload changed, because
denial grants no execution authority.

## Core session/project corroboration

The policy crate binds a pending request to a session id and project id, but VibeCoder Core does not
treat those caller-supplied ids as proof. Before a command can even become pending, Core freshly
verifies the managed `ProjectRef` and asks the active agent runtime to corroborate that the session is
still bound to that exact project. The Jcode adapter checks its verified in-memory binding, canonical
project root, current connection generation, and current attachment. `allow_once` repeats both the
workspace and agent-binding checks immediately before the broker can issue an execution envelope.
A deny can still clear a correctly scoped pending request even if the runtime has since disconnected.

This is still not a per-turn-generation proof. Part 14 pending requests must be revoked when the
originating turn is abandoned/cancelled; later orchestration can strengthen the binding to a concrete
turn/tool-call lifecycle.

## Execution envelope

An allow-once decision returns `CommandExecutionEnvelope`. Its fields are private and it implements
neither `Clone` nor `Serialize`/`Deserialize`. Part 15 consumes it by ownership into private-field
`AuthorizedCommand` material with no inverse constructor. The envelope records a runtime-managed
clean environment policy, never requests ambient environment
inheritance, never enables caller/model-provided stdin, and never authorizes shell-string parsing.

The envelope is still **not** a filesystem or sandbox capability. Arguments can instruct a powerful
tool to touch paths or networks outside the intended project if the future process has such OS
permissions. Part 15 now re-opens the project by id, resolves runtime/workspace executable authority
at operation time, defines a clean child environment, and enforces bounded output/time lifecycle.
Strong OS isolation, network restriction, same-UID race elimination, and Jcode built-in tool
confinement remain unresolved. `WorkspaceCapabilities.commands` remains a workspace-specific flag;
process execution is advertised through the separate `ProcessRuntimeCapabilities` boundary.

## Logging and secrets

`CommandSpec` uses a custom `Debug` implementation that redacts argument contents. Approval
serialization intentionally includes arguments because a user/application must be able to inspect
what it is approving, so approval objects are ephemeral UI/IPC data and must not be treated as a
safe place to persist secrets. Provider/API credentials must continue to flow through the secret
resolver rather than command arguments.
