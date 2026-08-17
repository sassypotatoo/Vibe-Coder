# Part 34.10.15 — Node on-demand delivery boundary

## Problem
The Node 24.19.0 Android/V8 cross-build consumes hours of GitHub Actions time and has repeatedly timed out while still actively compiling. Tying that job to every VibeCoder app build blocks iteration.

## Decision
The base VibeCoder APK no longer requires Node. Jcode and OmniRoute stay on their existing architecture. Node is a separately proven Android ARM64 artifact staged only into the `node_runtime` dynamic-feature module for Play App Bundle builds. Google Play delivers that module on demand during first-time runtime setup.

This intentionally does **not** download a raw executable into writable app-private storage. Android API 29+ forbids that execution model.

## User flow
1. Install/open base VibeCoder.
2. First setup shows Jcode and OmniRoute present and Node 24.19.0 required.
3. `SplitInstallManager` requests `node_runtime`.
4. UI reports real Play download bytes/percentage and supports cancellation.
5. After Play reports INSTALLED, VibeCoder resolves the package-owned Node executable and starts the existing local runtime.

## CI change
Normal Android diagnostic/alpha builds no longer depend on the multi-hour Node proof job. The old proof lane remains available only through `.github/workflows/node-runtime-proof.yml` and should be run only when Node/runtime compatibility changes.

## Non-goals
- No Jcode migration.
- No agent-routing redesign.
- No writable-home executable fallback.
- No claim that Node device execution is proven until a real Play-delivered split is tested on device.
