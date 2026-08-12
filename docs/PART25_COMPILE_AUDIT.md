# Part 25 first full compile audit

Part 25 is the planned 50% milestone: the source-only checkpoints through Part 24 are compiled,
tested, linted, and locked for the first time. This audit distinguishes what executed successfully
from what the runner could not execute.

## Toolchain and dependency graph

- Rust: `rustc 1.88.0 (6b00bc388 2025-06-23)`
- Cargo: `cargo 1.88.0 (873a06493 2025-05-10)`
- Rustfmt: `rustfmt 1.8.0-stable (6b00bc3880 2025-06-23)`
- Clippy: `clippy 0.1.88 (6b00bc3880 2025-06-23)`
- Workspace members: 24
- Root lockfile: present, 224 package records (24 workspace packages plus 200 dependency records)

The official `rustup-init` download used for the task-local toolchain was verified before execution
with SHA-256 `4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10`.

## Executed validation

The following commands completed successfully against the checkpoint source:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
python3 scripts/validate_checkpoint.py
```

The final Cargo test command used a previously absent external target directory. It compiled the
complete 24-member root workspace and passed 124 tests with zero failures and zero ignored tests.
That count includes the Part-24 adapter, state-machine, fixture, and provider-neutral Core contract
tests.

## Compile-loop repairs

The first compile exposed integration issues that source-only validation could not prove:

- Core's repair-loop cancellation method returned the authorization invalidation count where its
  public contract promised `Result<()>`; the count is now deliberately discarded.
- `vibecoder-build-loop` and `vibecoder-build-repair` tests imported
  `vibecoder-process-contract` without declaring it as a direct development dependency; both
  manifests now declare the dependency.
- command-policy normalized the project-root working directory to an empty path while downstream
  pipeline requests used `.`; root normalization now returns the canonical `.` spelling, keeping
  approved and executed command payloads equal.
- the runner rejected Unix-domain socket creation before the workspace special-file test reached
  product code. The equivalent non-regular-file boundary is now tested with a FIFO, covering both
  read and atomic-write rejection without weakening the product rule.
- formatting and warning-denied Clippy findings were resolved across all workspace targets. Narrow
  lint exceptions remain only where preserving a public unboxed API or an intentionally explicit
  resource-supervision signature is safer than a compile-milestone redesign, and each has a source
  reason.

One reused incremental target later emitted missing-crate artifacts. Repeating the same build from
an empty target directory passed, identifying that event as contaminated incremental state rather
than a source-graph failure. The final recorded result always comes from the clean target.

## Separately pinned Jcode crates

The two vendored public Jcode crates are deliberately excluded from root workspace membership and
were executed separately from a disposable copy:

- `jcode-harness-api`: 16 passing tests.
- `jcode-sdk`: 27 passing tests across unit, client-behavior, lifecycle, and structured-output
  suites.
- Total executed and passed: 43.

Two additional SDK lifecycle tests could not create their fixture Unix sockets because the runner
returned `EPERM` on bind:

- `global_events_discovers_existing_and_new_sessions_then_closes_children`
- `global_events_reports_bounded_queue_overflow`

Those tests are environment-blocked and are not reported as passed. The pinned vendored source was
not changed or bypassed. They should be rerun in a Unix environment that permits local socket
binding.

## Claims deliberately not made

This milestone does not prove Android cross-compilation, physical-device behavior, Android ARM64
packaging/execution of Jcode or Node/OmniRoute, JDK/Gradle/SDK availability, strong same-UID process
isolation, the Android secure-store adapter, or production UI. The private product remains
phone-local-first with no mandatory remote build/agent server.
