# Part 21 — build failure capture and first repair turn

Part 21 adds `vibecoder-build-repair`, an authority-free layer above the normalized Part-18 build result. It accepts only terminal `Failed` builds. Success, cancellation, timeout, queued, and running states are not eligible for automatic repair turns.

## Bounded evidence

The repair layer keeps at most 32 normalized diagnostics and a 32 KiB textual evidence excerpt. Individual evidence lines are capped, ANSI terminal sequences and bidi controls are stripped, common credential-bearing lines are replaced with a redaction marker, absolute-path-shaped tokens are replaced, embedded evidence delimiters are neutralized, and oversized single lines fail-redact before evidence is sent to the model. Raw stdout/stderr remains transient in the build result and is not persisted by Part 21.

The evidence fingerprint is SHA-256 over sanitized normalized failure data. It deliberately excludes the build id so equivalent failures in later rebuilds can have the same fingerprint. Part 21 does not make retry decisions from that fingerprint; Part 22 owns retry budgets and repeated-error stopping.

## One repair turn

Core verifies the project, requires zero active controlled project processes, requires the Jcode workspace to be quiescent, and freshly corroborates the session/project binding. It revokes stale project command approvals and creates a `BeforeBuildRepair` checkpoint before the agent can modify files.

The same-project lifecycle permit remains held while the repair turn runs, preventing controlled process starts and direct Core workspace mutations from overlapping this repair operation. The generated prompt treats build evidence as untrusted data, requests the smallest relevant source/configuration fix, and explicitly instructs the agent not to run the next build.

Part 21 performs exactly one repair turn. It does not rebuild, retry, declare the repair successful, or automatically roll back a failed repair. Those loop decisions belong to Part 22. Jcode built-in tool isolation remains subject to the previously documented runtime limitations until the later end-to-end tool-routing work.
