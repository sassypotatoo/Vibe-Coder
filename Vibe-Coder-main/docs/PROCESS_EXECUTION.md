# Part 15 — phone-local process lifecycle

Part 15 turns a Part 14 allow-once command envelope into one real local child process. Authorization
and execution remain separate: approving a command does not spawn it, and the executor cannot create
its own approval envelope.

## Runtime boundary

`vibecoder-process-contract` defines the provider-neutral process lifecycle. `vibecoder-process-local`
is the Unix/Android local implementation. `VibeCoderCore` keeps process execution disabled unless a
`ProcessRuntime` is explicitly attached after runtime provisioning.

A runtime tool is registered as an opaque tool id plus a path **relative to a platform-supplied
package-installed executable-code root**. Writable app-private `vibecoder/runtime` is data-only.
The executor never performs ambient `PATH` lookup. Workspace
executables remain project-relative and are allowed only when Part 14 policy explicitly enabled that
class of command.

At start time the executor consumes `CommandExecutionEnvelope`, re-checks the project id and clean
environment policy, reconstructs the managed project root, re-checks the working directory, and
re-resolves the executable. Runtime/workspace executable paths reject traversal, symlinks, special
files, suspicious hard-link aliases, and files without the owner execute bit.

`VibeCoderCore::start_authorized_project_command` performs an additional execution-time managed
workspace check and current Jcode session/project-binding corroboration before handing the envelope
to the process runtime. Approval therefore is not treated as a durable filesystem/session proof.

## Child environment

The child is started with:

- structured argv only; no shell-string API,
- `stdin` connected to null,
- `stdout` and `stderr` piped separately,
- `env_clear()` so the app's ambient environment is not inherited by contract,
- runtime-managed `HOME` and `TMPDIR` inside the writable app-private runtime-data directory,
- the verified project-relative working directory.

Part 15 intentionally does not add model/caller environment variables. Later build-tool provisioning
can define trusted runtime behavior without converting agent input into environment authority.

## Lifecycle, timeout, and cancellation

On Unix/Android the child calls `setpgid(0, 0)` before exec, giving each approved command its own
process group. The supervisor polls the direct child and nonblocking stdout/stderr pipes. Cancellation
or timeout sends `SIGTERM` to the process group, holds the group leader unreaped for the bounded
grace window so its PID/process-group id cannot be recycled, then sends `SIGKILL` to the group before
reaping. This also covers descendants that ignored TERM after closing inherited output pipes. If
process-group signaling unexpectedly fails, the executor falls back to killing the direct child so
lifecycle enforcement cannot silently vanish.

The timeout clock starts immediately after successful child spawn, not when the supervisor thread
happens to get CPU time. The default timeout is 10 minutes. Valid request timeouts are 1 second
through 30 minutes. At most four processes may be active globally and two per project. Process ids and running handles are
runtime-only and non-serializable.

Cancellation is explicit. Dropping a `RunningProcess` observation handle does not cancel the child,
so an Android UI lifecycle change does not silently kill a build. `ProcessRuntime::cancel` owns that
authority.

## Bounded output

Default final capture is 4 MiB for stdout and 4 MiB for stderr; each stream has a hard 16 MiB ceiling.
Pipes are nonblocking and read in 16 KiB chunks, with at most eight chunks drained from each stream
per supervisor poll so continuous stdout/stderr cannot starve timeout or cancellation checks. Final
capture records truncation rather than growing without bound.

Live output uses a separate bounded 256-event queue. If the consumer is slow, events are dropped and
`event_queue_overflowed` is recorded; final bounded capture remains independent. `Debug` for process
results and output events reports byte counts only and never prints captured contents.

## Fail-closed cleanup

A child is reserved in a bounded active-process registry before spawn. Spawn failure releases the
reservation. If pipe setup or supervisor-thread creation fails after spawn, the child process group
is killed and waited synchronously rather than being orphaned. Output-read/wait failures also force
cleanup before an error is returned.

## Explicit limitations

Part 15 is lifecycle control, **not a kernel sandbox**:

- an approved executable still runs with the VibeCoder app UID's OS permissions;
- command argument semantics are not sandboxed;
- network access is not blocked here;
- a hostile same-UID concurrent mutator can still race pathname lookup between verification and
  `execve` through `std::process::Command`;
- a malicious descendant can deliberately create a new session/process group and escape the
  original process-group termination boundary;
- a descendant deliberately detached/backgrounded during an otherwise normal successful leader exit
  can outlive the tracked command; Part 15 guarantees group cleanup for cancellation/timeout/error,
  not a kernel-enforced no-daemon policy;
- Jcode built-in command tools are not yet redirected through this executor;
- Android ARM64 package layout/execution of Jcode, Node/OmniRoute, JDK, Gradle, or Android SDK tools
- Android ARM64 packaging/execution remains unproven until the Part 26 package and device probes pass.
Android API 29+ also blocks `WorkspaceExecutable` from the writable project tree; project scripts must be inputs to a trusted package-installed interpreter/runtime tool rather than direct process executables.
  is still unproven. Part 26 defines the inventory/proof boundary but does not fabricate device proof.

Those limitations are tracked rather than disguised as completed isolation.

## Part 27 Android interpreter and package-root refinement

The process layer now allows a trusted runtime-tool definition to carry a bounded fixed argv prefix.
This does not invoke a shell: the package-owned executable remains argv[0], the fixed prefix is
inserted next, and project-authorized args follow. This is the required execution shape for tools
such as npm, whose CLI is JavaScript data interpreted by packaged Node rather than an independently
executable writable file.

On Android, direct workspace executables remain rejected. The Android host also no longer assumes
that `ApplicationInfo.nativeLibraryDir` necessarily exposes every child runtime as an extracted
filesystem file. A separately supplied package-owned child-executable root is required and is
rechecked as non-writable before spawning.

## Reviewed cancellation-grace contract

`PROCESS_TERMINATION_GRACE_MS` is now part of the process contract and is fixed at 250 ms for the current Unix/Android local runtime. After SIGTERM, the supervisor intentionally keeps the process-group leader unreaped through this window before SIGKILL escalation so the process-group id cannot be reused while descendants may still exist. Callers should therefore expect cancellation/timeout completion to include this bounded safety delay even when the direct child exits immediately after SIGTERM.
