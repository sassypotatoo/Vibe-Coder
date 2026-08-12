# First 50% roadmap — 25 small parts

Each part is intentionally small. No full compile occurs until Part 25 is complete.

1. Foundation, contracts, provenance, invariants.
2. Jcode adapter transport boundary and connection lifecycle.
3. Jcode session create/resume/cancel mapping.
4. Jcode streaming event and turn-result mapping.
5. Jcode permission/capability negotiation.
6. Jcode model discovery/selection mapping.
7. OmniRoute HTTP client boundary and URL/auth validation.
8. OmniRoute health and model-catalog mapping.
9. Model route policy and fallback configuration model.
10. Secret reference/config loading without plaintext persistence.
11. Workspace root creation and canonical path containment.
12. Safe file read/write primitives and atomic writes.
13. File edit/patch and search primitives.
14. Command request model, allow/deny policy, and execution envelope.
15. Process lifecycle, cancellation, output capture, and timeout model.
16. Project/session persistence model.
17. Checkpoint/snapshot metadata and rollback contract.
18. Build job abstraction and normalized build result model.
19. Website toolchain detection and package-manager abstraction.
20. Website build pipeline state machine.
21. Build-error capture and agent repair-turn orchestration.
22. Loop guards: retry budgets, repeated-error detection, cancellation.
23. End-to-end backend task state machine: prompt -> agent -> tools -> result, with the bundled OmniRoute deterministic runtime profile enforced and the chosen gateway model independently corroborated in Jcode before inference.
24. Static integration fixtures and failure-path contract tests (source-level where executable), including rejection of hidden gateway reroutes and gateway/Jcode model-identity mismatch.
25. Precompile audit, dependency/toolchain readiness, then **FIRST FULL COMPILE** and compile-fix loop.

Production UI remains outside this list and is implemented at the final stage.

### Part 17 completion note

Implemented immutable app-private tree snapshots, SHA-256 integrity metadata, active process/agent guards, command-epoch invalidation, Android/Linux atomic exchange rollback, and post-rollback Jcode re-corroboration.
