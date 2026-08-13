# Part 34.3 — OmniRoute Android packaging

## Scope of this checkpoint

This checkpoint admits the exact reviewed OmniRoute 3.8.50 source archive and proves that
VibeCoder's existing deterministic-routing patch applies to those exact bytes. It does **not** claim
that an Android runtime bundle, APK asset, or service round trip exists yet.

Reviewed source authority:

- archive: `OmniRoute-release-v3.8.50.zip`
- SHA-256: `1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7`
- compressed size: 64,028,571 bytes
- entries: 13,622
- uncompressed payload: 204,459,095 bytes
- package version: 3.8.50
- Node engine: `>=22.22.2 <23 || >=24.0.0 <27`
- VibeCoder runtime profile: `vibecoder-omniroute-exact-model-v1`

The reviewed archive contains source only. It has no `.next`, `.build`, `dist`, `node_modules`, native
`.node` runtime, or ready-to-run standalone server. VibeCoder therefore must build a production
backend-only standalone bundle before APK packaging.

## Admission path

`scripts/prepare_omniroute_android_source.py` fails closed unless all of these remain true:

1. the ZIP bytes match the reviewed SHA-256 and exact reviewed size;
2. the archive has the reviewed root and bounded entry/uncompressed counts;
3. no absolute path, parent traversal, symlink, duplicate normalized path, oversized entry, or
   generated runtime root is present;
4. `package.json`, `.node-version`, `.nvmrc`, and `package-lock.json` retain reviewed identities;
5. the deterministic-routing patch applies only to the hash-pinned targets;
6. every patched target matches its expected post-patch SHA-256.

Preparation writes machine-readable evidence but deliberately leaves runtime/APK/service proof false.

## Android native-dependency finding

The 3.8.50 lockfile does not provide Android ARM64 native platform packages for several optional or
feature-specific dependencies:

- `sharp` platform/libvips packages cover desktop/server OS targets, not Android;
- `sqlite-vec` platform packages cover Linux/macOS/Windows, not Android;
- `onnxruntime-node` declares Windows/macOS/Linux only;
- `wreq-js` declares Windows/macOS/Linux only;
- `better-sqlite3` is an optional native addon.

This does **not** mean the gateway core is impossible on Android. The reviewed source already contains
important degradation/fallback paths:

- Node 24 can use OmniRoute's built-in `node:sqlite` adapter instead of `better-sqlite3`;
- `sql.js` WASM is the final DB fallback and its WASM is explicitly copied into standalone bundles;
- `sqlite-vec` failure degrades memory retrieval to FTS5 keyword search;
- `wreq-js` load failure is represented as unavailable rather than a mandatory startup import;
- `sharp` is dynamically imported only when Cursor image preparation is requested.

The next packaging slice must turn those facts into an explicit Android runtime profile. It must not
ship Linux native addons and hope Android never loads them.

## Required runtime build shape

The reviewed upstream build already supports `OMNIROUTE_BUILD_BACKEND_ONLY=1`, which stubs dashboard
UI leaves while preserving API route handlers. The intended VibeCoder bundle is therefore:

`reviewed ZIP -> exact patch -> npm lock install -> backend-only Next build -> standalone assembly -> Android native-dependency prune/validation -> bundle manifest -> APK asset`

Runtime launch remains loopback-only and must ultimately use the package-owned Node executable with an
app-private OmniRoute data/config directory. `GET /v1/models` plus the VibeCoder runtime-profile
attestation are required before gateway readiness becomes true.

## Not yet claimed

- Node 24.19.0 Android binary execution;
- Android-safe production OmniRoute standalone bundle;
- APK asset extraction/manifest verification;
- loopback service start/stop;
- authenticated `/v1/models` round trip;
- runtime-profile attestation round trip.

## Part 34.3.2 — Android backend runtime profile and bundle sealer

The Android runtime profile is now explicit in
`config/omniroute-android-runtime-profile.json`. It does not claim that the production bundle has
already been built. It defines the only accepted build/runtime shape before that external proof can
become true.

Build authority:

- exact Node runtime: `24.19.0`;
- reviewed OmniRoute source: `3.8.50` plus the existing deterministic-routing patch;
- upstream backend-only build: `npm run build:backend`;
- `OMNIROUTE_BUILD_PROFILE=minimal`;
- `OMNIROUTE_MITM_STUB=1`;
- `VECTOR_STORE_DISABLE_VEC=true`.

Runtime authority:

- entry point: `server-ws.mjs`, retaining upstream trusted peer-IP stamping;
- bind host: `127.0.0.1` only;
- port: `20128`;
- app-private data/config remains required at service-launch time;
- vector memory is intentionally degraded to FTS5 on Android;
- Node 24 `node:sqlite` is the preferred SQLite driver with `sql.js` WASM as the final fallback.

`scripts/prepare_omniroute_android_bundle.py` is a post-build trust boundary. A supported desktop
builder may need host-native packages while Next.js is compiling, but those bytes are not Android
runtime authority. The sealer copies the standalone tree with symlinks dereferenced only after proving each symlink
target remains inside the standalone/prepared-source authority, prunes the reviewed unsupported
feature/native packages, removes the TPROXY native subtree, then rejects any remaining `.node`,
`.so`, `.dylib`, `.dll`, `.exe`, ELF, PE, or Mach-O payload. Unknown native bytes
therefore fail the build instead of quietly entering the APK.

The sealed bundle contains `.vibecoder-omniroute-bundle.json`, including the reviewed source hash,
routing-patch hash, runtime profile, feature degradations, every retained file's SHA-256, total bytes,
file count, and a deterministic tree SHA-256. `scripts/verify_omniroute_android_bundle.py` re-hashes
that tree independently; the producer's manifest is not trusted merely because the producer wrote it.

`scripts/build_omniroute_android_bundle.py` orchestrates source admission -> exact Node preflight ->
lockfile install -> backend-only build -> Android sealer -> independent verifier. It refuses to build
under a different Node version. The current runner executed this preflight and correctly stopped at
`actual=22.16.0` versus required `24.19.0`; therefore `runtime_bundle_built`, APK packaging and service
round-trip proof remain false.

A synthetic standalone fixture verified the pruning/sealing path, and a separate unknown `.node`
fixture was rejected with `omniroute_android_bundle_host_native_binary_forbidden`. Synthetic fixture
success is validation of the packaging code only; it is not evidence that OmniRoute itself has been
built or started on Android.

## Part 34.3.3 — generated APK asset + app-private installation foundation

The production runtime bundle is now assigned a generated Android asset lane. `build.gradle.kts`
reads `android/app/build/generated/omnirouteAssets` in addition to tracked static assets. The reviewed
64 MB source ZIP is never an APK input. `scripts/stage_omniroute_android_asset.py` accepts only a
bundle that first passes the independent Part 34.3.2 verifier and stages it at
`omniroute/bundle` through a same-filesystem temporary directory and replacement. Its evidence says
only `apk_asset_staged=true`; APK packaging, device extraction and service round-trip proof remain
false until those events actually happen.

`OmniRouteAssetInstaller` is the Android-side trust boundary. When the bundle manifest is present in
the signed APK, it validates the exact OmniRoute/version/profile/source-patch/Node/runtime contract,
rejects absolute/traversal/backslash/reserved paths, bounds manifest/file/total sizes, re-computes the
manifest tree hash, then copies each listed asset while checking its declared byte count and SHA-256.
No runtime file is accepted from shared/external storage.

The verified staging tree is written below the app-private runtime parent and promoted to
`files/vibecoder/runtime/omniroute`. A process-local file lock serializes installations. The previous verified runtime is retained during replacement; commit/post-commit failure restores it. Stale stage
directories are removed on the next pass, and an interrupted state with a valid `.omniroute-previous`
can be recovered before processing the APK's current manifest. Existing installations are fully
re-hashed before reuse rather than trusting a marker file alone.

APK verification now has an `omniroute_asset` mode that safely extracts only
`assets/omniroute/bundle/*` from the built APK and re-runs the independent runtime-bundle verifier.
Device acceptance has a matching `omniroute_asset` mode which requires the diagnostic app to report
`packaged=true` and `verified=true` after app-private installation while explicitly requiring
`service_round_trip_proven=false`. Service launch belongs to Part 34.3.4.

The real production bundle still cannot be staged in this runner because Part 34.3.2 correctly refuses
to build OmniRoute under Node 22.16.0 instead of the pinned Node 24.19.0. Therefore this step proves
the packaging/extraction implementation and regression fixtures, not a real APK/device extraction.

## Part 34.3.4 — supervised OmniRoute service lifecycle source foundation

OmniRoute is now assigned a dedicated long-running service path rather than being disguised as a
normal project command. The earlier runtime-service primitive inherited the command wall-clock limit,
which would have killed a healthy gateway after at most 30 minutes. The local process runtime now has
`start_persistent_runtime_service`: package-owned executable authority, no shell/PATH lookup, cleared
ambient environment, bounded stdout/stderr, process-group cancellation and Android
`PR_SET_PDEATHSIG`, but **no automatic wall-clock timeout**. Normal project commands retain their
existing timeout policy.

The trusted Node executable runs with working directory `files/vibecoder/runtime/omniroute`. Writable
OmniRoute state uses a separate service-private `DATA_DIR`. The only accepted runtime environment is
bounded and explicit: production mode, `HOSTNAME=127.0.0.1`, all OmniRoute/API/dashboard ports pinned
to 20128, vector-native disablement and the reviewed MITM stub profile. `PATH`, `NODE_OPTIONS`,
`NODE_PATH`, loader-preload variables and ambient environment inheritance are rejected.

Before every launch the Rust Android host independently re-verifies the mutable app-private runtime.
It binds the launch to the SHA-256 of the manifest that came from the signed APK asset installer,
validates the installer receipt, exact upstream/profile/Node/source/routing identities, every file
size/SHA-256, deterministic tree SHA-256, required entrypoint, exact file set and no symlink/special
file. Rewriting both the installed manifest and receipt therefore cannot substitute a new runtime
without also matching the signed APK manifest hash supplied at launch.

Readiness is deliberately stronger than `spawn()` success or a log substring. The service must remain
active while the anonymous hash-pinned VibeCoder runtime-profile endpoint at
`http://127.0.0.1:20128/v1` succeeds **twice consecutively** with exact upstream version, routing
profile/hash and exact-model/no-hidden-reroute flags. Startup failure cancels and reaps the child.
This proves the intended OmniRoute runtime profile only; authenticated `/v1/models` and a real model
request remain later gateway/model milestones.

A process-global Rust FFI session retains the service handle across JNI calls and exposes bounded JSON
start/status/stop controls. JNI uses a one-shot bounded buffer for mutating start/stop operations;
a two-call size-query ABI would execute those mutations twice. Android diagnostics start the service
only when explicitly launched with the `vibecoder_omniroute_service_test` flag. The new
`omniroute_service` device acceptance mode requires exact Node 24.19.0 package/ARM64/execution/version/
16-KiB evidence, verified OmniRoute asset installation, signed-manifest binding, loopback URL and the
runtime-profile round trip. Its diagnostic run then performs a live status read and an explicit JNI
stop, and reports `service_round_trip_proven=true` only if the process is confirmed inactive/reaped
after that stop. Activity destruction remains a best-effort cleanup guard, while Android process
death is additionally guarded by the child PDEATHSIG behavior.

Crash recovery does not self-loop. A stale exited handle is reaped on the next explicit start/stop,
and restart is an explicit stop -> full runtime re-verification -> fresh start operation. Automatic
service restart remains false so Part 34 does not accidentally grow an autonomous recovery loop.

This checkpoint does **not** claim a real service round trip yet. The current runner still lacks the
pinned Android Node binary and cannot produce the real Node-24-built OmniRoute standalone bundle.
Fresh Rust compilation of these new service changes is also not claimed because this runner has no
Cargo/Rust toolchain; the recorded historical compile baseline predates this slice. C/JNI syntax and
source/regression validators are exercised separately.
