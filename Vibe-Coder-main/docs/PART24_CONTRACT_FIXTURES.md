# Part 24 — Static integration fixtures and failure contracts

Part 24 converts the Part 23 security claims into versioned test inputs and Rust test targets. It
does not add production authority and does not claim compiled execution before the Part 25 gate.

## Fixture layers

1. `runtime_profiles.json` feeds raw status, content type, and body bytes into the real OmniRoute
   runtime-profile interpreter. It covers the exact accepted attestation plus digest/version/flag
   drift, unknown fields, malformed JSON, bad media type, empty bodies, and stable HTTP failures.
2. `omniroute_reroute_contracts.json` maps the complete audited mutation-path set to concrete guards
   in the hash-pinned patch. Coverage must exactly equal the patch metadata; duplicate, missing, or
   unbound paths fail static validation.
3. `task_state_contracts.json` drives exact catalog/active-identity checks, progress observation,
   configured fallback, unsafe failure stopping, and cancelled completion in the authority-free
   state machine.
4. `backend_task_contracts.json` drives Core with provider-neutral fakes. Cases cover success,
   configured fallback, hidden-reroute rejection before catalog use, gateway/Jcode identity drift,
   duplicate session models, cancellation, prose-backed agent failure, and active-process exclusion.

## Stale authority contract

The Core integration test creates a real Part 14 allow-once envelope before a backend turn, then
confirms the envelope cannot start after the turn's authorization epochs advance. Its event handler
also creates a pending approval while `run_turn` is active and confirms the post-turn invalidation
removes it. The fake process runtime records zero starts, so an assertion cannot accidentally launch
a command.

## What static validation proves

The Part 24 validator parses every fixture with strict expected keys, checks unique case names and
required coverage, binds fixture profile identifiers/digests to the production constants and patch,
checks that each fixture is included by a Rust test target, and scans the new Rust sources for
structural delimiter balance. It also reuses all Part 1–23 provenance and security validation.

Static validation does not prove type checking, linking, async execution, or platform behavior. Part
25 must run the first full Rust compile/test loop and fix compile-time or executable failures before
the 50% milestone can be claimed.
