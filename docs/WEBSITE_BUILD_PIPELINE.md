# Part 20 — website build pipeline

Part 20 adds `vibecoder-web-build-pipeline`, a move-only state machine above Parts 14, 15, 18, and 19. It creates no new executable authority.

## States

A static HTML project reaches `NoBuildRequired`. A package-managed project advances through:

`AwaitingApproval(DependencyInstall) -> Running(DependencyInstall) -> AwaitingApproval(BuildScript) -> Running(BuildScript) -> Succeeded|Failed|Cancelled|TimedOut`.

If dependency installation is deliberately skipped, the pipeline starts at build-script approval. Each executed stage receives its own normal Part-18 `BuildId`; the pipeline itself has a separate correlation id.

## Install policy

Part 20 only installs dependencies when a supported lockfile exists. Unlocked installs are rejected because they can resolve and persist a new dependency graph. Locked commands are deterministic intent:

- npm: `ci`
- pnpm: `install --frozen-lockfile`
- Yarn: immutable/frozen install according to declared generation
- Bun: `install --frozen-lockfile`

Dependency lifecycle scripts are disabled by default. An explicit policy may allow them, but they remain arbitrary project/dependency code running with the current Part-15 process authority; strong kernel isolation is not claimed.

## Approval and drift binding

The pipeline emits an exact structured `CommandSpec`. It must pass the existing Part-14 allow-once broker. Every stage requires Part-14 allow-once approval. Core re-inspects the toolchain before requesting approval and again after acquiring the same-project lifecycle gate immediately before spawn. The full report must match, including exact SHA-256 fingerprints of `package.json` and the selected lockfile. Start additionally requires the Jcode workspace to be quiescent and the authorized command to exactly equal the current pipeline-stage command.

This prevents a VibeCoder-managed edit from changing the approved package/build script between validation and process start. It is not claimed to be strong isolation against a malicious same-UID process outside the controlled lifecycle.

## Result meaning

Stage results use the Part-18 normalized build lifecycle. Build-process exit 0 advances the pipeline to `Succeeded`, but does **not** claim a deployable website bundle was discovered or integrity-verified. Diagnostic parsing starts in Part 21 and artifact discovery/verification remains later work.

## Runtime limitations

Logical npm/pnpm/Yarn/Bun ids still have to be provisioned in the trusted runtime-tool registry. Part 26 requires any native launcher behind that registry to come from package-installed code, not writable app data. Part 20 does not prove Android ARM64 Node/package-manager packaging, does not use ambient PATH, and does not yet implement general `engines.node` range corroboration.
