# Part 18 — build-job abstraction and normalized results

Part 18 adds a provider-neutral build lifecycle on top of the already-authorized Part 15 local process runtime. It does **not** choose a package manager, Gradle task, Node command, or Android SDK tool. Those decisions belong to later pipeline parts.

A build has a fresh `BuildId`, one managed `ProjectId`, and an explicit target (`Website` or `Android`). The normalized lifecycle is `Queued -> Running -> Succeeded | Failed | Cancelled | TimedOut`. Cancellation and timeout remain distinct terminal states rather than being flattened into a generic failure.

`RunningBuildJob` wraps one bounded `RunningProcess`. Live stdout/stderr events remain bounded by Part 15 and Debug output redacts their contents. `BuildResult` retains bounded raw process evidence only in memory so Part 21 can later parse compiler/build errors; Part 18 does not persist raw output.

Build results support bounded normalized diagnostics and bounded artifact metadata. Artifact paths are strict UTF-8 with canonical relative spelling and reject absolute/traversal/control/bidi/backslash/internal-temp ambiguity. Duplicate artifact paths are rejected. Part 18 does not verify artifact bytes. `BuildArtifact` may carry a strictly formatted lowercase SHA-256 value recorded by a later discovery layer, but that metadata is not verification authority. Process exit code 0 means only that the process reported success, not that an APK/site bundle was discovered or integrity-verified.

Part 18 intentionally does not implement website toolchain detection, package-manager selection, website build orchestration, Android build orchestration, build-error parsing, or artifact discovery.

A build descriptor is created before process start, so `Queued` is a real representable state rather than a post-hoc label. The descriptor is move-only and consumed by Core when the approved process starts, preventing accidental reuse of one build id across multiple process starts through the build API.
