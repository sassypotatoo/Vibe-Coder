# Part 34.10.15 — Direct Node setup download

## Goal
Keep the proven Alpha APK path fast: Jcode and OmniRoute stay on the working baseline, while Node 24.19.0 is not bundled into the base APK and is not compiled by normal app CI.

## Runtime delivery
A separately built Android ARM64 Node runtime is packaged as a signed same-package `node_runtime` split APK and published as a fixed GitHub Release asset. The VibeCoder first-run setup downloads that fixed runtime package, verifies its package identity, version, signing certificate and embedded Node payload, then installs it into the existing VibeCoder package with Android `PackageInstaller.MODE_INHERIT_EXISTING`.

This does **not** copy a raw executable into writable app-private storage. The executable remains package-installed native code.

## User flow
1. Install and open the VibeCoder Alpha APK.
2. The setup screen shows Jcode and OmniRoute already present and Node 24.19.0 required.
3. Tap **Download & Set Up Node.js**.
4. VibeCoder downloads the fixed signed runtime package and shows real byte/percentage progress.
5. Android may request one-time permission/confirmation to install the runtime split.
6. After installation, VibeCoder resolves the package-owned Node executable and continues startup.

## CI
Normal Android diagnostic/Alpha builds do not compile Node. The dedicated Node runtime workflow only builds/publishes the fixed runtime package when that release asset is missing or the runtime packaging version is intentionally changed.

## Non-goals
- No Jcode migration.
- No agent-routing redesign.
- No writable-home executable fallback.
- No store-delivery integration during development.
