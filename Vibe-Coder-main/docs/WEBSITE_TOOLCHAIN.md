# Part 19 website toolchain detection

Part 19 provides the read-only website-project analyzer used by the Part 20 build pipeline. It does not spawn Node/package-manager processes, resolve absolute executable paths, or trust ambient `PATH`.

## Targeted root metadata

Toolchain detection no longer recursively enumerates the project tree. It does not recursively enumerate the project tree. A dependency install can create a very large `node_modules` tree, so recursive listing would make a valid post-install project look truncated. Instead the analyzer probes only a fixed set of root metadata files through `WorkspaceRuntime::regular_file_exists`, then safely reads the exact files whose bytes matter.

`package.json` is bounded to 1 MiB. The selected lockfile is bounded to 8 MiB. Both are SHA-256 fingerprinted so Part 20 can reject approval/start if either changes after pipeline preparation. Symlink, special-file, and hard-link aliases remain rejected by the workspace boundary.

Supported package-manager signals:

- npm: `npm-shrinkwrap.json` takes precedence over `package-lock.json`
- pnpm: `pnpm-lock.yaml`
- Yarn: `yarn.lock`
- Bun: `bun.lock` or `bun.lockb`; having both is rejected as ambiguous
- `packageManager` may select npm/pnpm/Yarn/Bun when no lockfile exists

Different lockfile families or a `packageManager`/lockfile disagreement fail closed. There is no silent npm default. It does not silently default to npm. The exact `packageManager` declaration is preserved as metadata so Part 20 can safely distinguish Yarn generations where required.

## Framework and build intent

The analyzer classifies static HTML, Next.js, Angular, Vite, Vue, React, and generic Node projects. It reports whether a string-valued `build` script exists, but never returns or authorizes the script body. Runtime tool ids are fixed logical registry ids only.

`engines.node` remains advisory metadata. Part 20 does not claim general npm-semver compatibility verification against a packaged Android Node runtime; that remains part of runtime provisioning/bring-up.
