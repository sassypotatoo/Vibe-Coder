# Test fixtures

`part24/` contains versioned, non-secret contract inputs for the backend-task boundary:

- `runtime_profiles.json` drives the real strict OmniRoute runtime-profile interpreter.
- `task_state_contracts.json` drives the authority-free task-state transition tests.
- `backend_task_contracts.json` drives provider-neutral Core integration tests.
- `omniroute_reroute_contracts.json` binds every audited hidden reroute to its required patch guard.

Fixtures grant no runtime authority and contain no credentials, absolute device paths, or real model
responses. Rust execution begins with the Part 25 full compile; Part 24 validates fixture structure,
coverage, and source wiring without pretending that static checks are compiled test results.
