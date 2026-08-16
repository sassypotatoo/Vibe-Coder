# Part 34.2.1 — Android Node runtime foundation audit

Status: **AUDIT COMPLETE; NO NODE RUNTIME ARTIFACT BUILT OR PACKAGED BY THIS STEP**

This checkpoint only inspects the existing Android runtime architecture before changing it. It does
not advance OmniRoute, npm/website builds, the Alpha UI, or physical-device readiness.

## Existing foundation that should be reused

- The Android runtime inventory already pins Node **24.19.0** as a native executable named
  `libvibecoder_node_exec.so` and requires package presence, ARM64 identity, execution, version and
  16 KiB page-size compatibility evidence.
- The payload manifest pins the Node 24.19.0 source tarball and SHA-256
  `f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f`.
- The reviewed upstream v24.19.0 source contains `android-configure`/`android_configure.py` and accepts
  `arm64`/`aarch64` Android targets. The current `./android-configure <NDK> <API> arm64` invocation
  shape is therefore consistent with that upstream entry point.
- `AndroidHostRuntime` already registers Node from the package-owned executable root rather than
  ambient PATH or writable app data.
- `collect_packaged_native_evidence()` already performs the Node `--version` probe and feeds the
  native ELF/ARM64/16-KiB evidence into fail-closed runtime readiness.
- `packaging.jniLibs.useLegacyPackaging = true` is already the package-layout contract used by the
  diagnostic shell so child-process native payloads are expected to exist as real extracted files.

## Definite defects / missing proof found

1. **Node staging still mutates the source JNI tree.**
   `scripts/provision_node_android.sh` writes the Node binary to
   `android/app/src/main/jniLibs/arm64-v8a/libvibecoder_node_exec.so`. Part 31 moved generated native
   payload authority to `android/app/build/generated/jniLibs/arm64-v8a`; Node was not migrated.

2. **The Node provisioning script is coupled to npm and also mutates source assets.**
   The same script copies `deps/npm` into `android/app/src/main/assets/node/npm`. Part 34.2 is Node
   runtime proof only; npm belongs to the later website-build/runtime-binding path. Node provisioning
   must not silently advance that capability or dirty checksummed source assets.

3. **The Node build output is not passed through the existing Android ELF verifier before staging.**
   The script checks only that `out/Release/node` exists. The Jcode/host lanes already use stronger
   ELF identity/loader/16-KiB checks; Node needs the same fail-closed pre-package gate.

4. **There is no Node-specific APK build/verification lane.**
   `part31_build_and_verify.sh`, `verify_android_diagnostic_apk.sh`, and the GitHub Actions workflow
   currently distinguish only `minimal` and `jcode`. A generated Node payload can therefore be
   produced without a first-class APK evidence mode.

5. **There is no Node physical-device acceptance mode.**
   `test_android_diagnostic_device.sh` validates Core and optionally Jcode. It does not require Node
   package presence, ARM64 identity, process execution, exact version, and 16-KiB compatibility to
   pass together.

6. **The Part-31 source validator has a Node provisioning blind spot.**
   It rejects source-tree JNI staging in the host/Jcode/build-lane scripts but does not inspect
   `scripts/provision_node_android.sh`, so the current invalid Node destination does not fail the
   checkpoint validator.

## Non-problems confirmed

- The pinned Node version is inside the previously reviewed OmniRoute engine range.
- The Node source SHA pin is internally consistent across the provisioning script and payload
  manifests.
- The runtime inventory and its APK asset copy are byte-equivalent at the JSON data level.
- The payload provisioning manifest and its APK asset copy are byte-equivalent at the JSON data level.
- The current Node provisioning script passes Bash syntax validation.
- No fake Node payload is committed in source control.

## Required next implementation step

Part 34.2.2 must repair the Node-only build/staging lane before attempting a physical build:

1. stage only the Node executable under `android/app/build/generated/jniLibs/arm64-v8a`;
2. remove npm staging from the Node-only provisioning path;
3. run the Android ELF verifier before accepting the generated Node payload;
4. teach checkpoint validation to reject Node source-tree staging;
5. add a dedicated Node APK verification/evidence mode without changing Jcode behavior.

Only after those source contracts pass should the actual Android Node cross-build/device probe be
attempted.

## Part 34.2.2 implementation — generated staging + validation lane repair

Status: **SOURCE LANE REPAIRED; NODE BINARY STILL NOT BUILT OR DEVICE-PROVEN**

This follow-up fixes only the Node packaging/evidence lane identified by the audit. It does not claim
that Node 24.19.0 has successfully cross-built for Android, that the APK has executed Node on a phone,
or that OmniRoute is available.

Implemented contracts:

- `provision_node_android.sh` now stages only
  `android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so`.
- Node provisioning no longer copies npm and no longer writes to `src/main/assets` or source JNI
  directories. npm stays a later website-build/runtime-binding capability.
- The raw `out/Release/node` candidate and the staged generated copy must both pass
  `verify_android_elf.py` before the payload can enter an APK build lane.
- `verify_android_diagnostic_apk.sh` has a dedicated `node` mode that requires the Node native entry
  while preserving the existing `minimal` and `jcode` requirements.
- `part34_node_build_and_verify.sh` consumes an already staged Node candidate, validates source
  authority, resets generated JNI state, restores exactly that candidate, rebuilds the Android host,
  assembles the diagnostic APK, verifies the APK in `node` mode, and emits Node-specific evidence.
- `write_node_build_evidence.py` records the APK/native hashes and explicitly marks device execution
  as unproven. Packaging evidence is not runtime readiness evidence.
- The static checkpoint validator now inspects `provision_node_android.sh`, rejects source-tree Node
  staging/npm coupling, and requires the dedicated Node staging/evidence contracts.

Deferred to the next sub-step:

- actual Node 24.19.0 Android cross-build;
- APK artifact production using the real Node candidate;
- physical-device `node --version` and process execution proof;
- any OmniRoute packaging or service work.

## Part 34.2.3 implementation — reproducible cross-build execution/evidence lane

Status: **EXECUTION LANE READY; CURRENT RUNNER BLOCKED BEFORE NODE CONFIGURE BY MISSING NDK**

This step does not convert an unavailable compiler run into a success claim. Node upstream still
classifies Android as unsupported/experimental, so every build candidate must earn its own evidence.
The VibeCoder Android project already pins NDK `28.2.13676358`; Part 34.2.3 intentionally reuses that
project-wide pin rather than introducing a Node-only NDK. Android's 16-KiB guidance states that NDK
r28 and higher emit 16-KiB-aligned native code by default, while VibeCoder still verifies the actual
ELF instead of trusting the default.

Implemented in this step:

- `provision_node_android.sh` now rejects any NDK revision other than `28.2.13676358`, requires the exact
  VibeCoder Android API 29 build identity, checks Node-supported Python versions, preserves configure and
  make logs, and emits cross-build evidence only after the real output passes the Android ELF gate.
- The provisioner preflights the exact NDK ARM64 clang/clang++ executables and verifies generated
  `config.gypi` + `Makefile` target identity after `android-configure`; wrapper exit status alone is
  not treated as configure success because upstream currently shells out to `./configure`.
- `write_node_cross_build_evidence.py` binds the candidate to Node 24.19.0's source SHA-256, Android
  API, Bionic/ARM64 target, exact NDK revision, configure/make log hashes, output SHA-256 and the
  verified 16-KiB ELF properties. Device execution remains explicitly false.
- `verify_node_cross_build_evidence.py` refuses a Node candidate whose bytes, source identity, target,
  API, NDK identity or ELF evidence do not match that record.
- `part34_node_build_and_verify.sh` now requires the cross-build evidence before APK packaging and
  re-verifies the candidate after the generated-JNI reset.
- `write_node_build_evidence.py` binds APK evidence to the cross-build record, preventing a random
  same-named prebuilt from being relabeled as the pinned source build.
- `.github/workflows/android-diagnostic-apk.yml` now contains an isolated Node 24.19.0 Android proof
  job using the existing pinned SDK/NDK/Gradle/JDK/Rust toolchain. Configure/make logs are uploaded on
  failure so the next loop can fix the first real build error rather than guessing.

Current-runner execution result:

- no Android NDK is installed;
- direct external binary download is unavailable in this execution environment;
- the Node provisioner therefore fails closed at `android_ndk_root_missing` before source download or
  configure;
- no Node binary, APK evidence, or device-execution evidence is claimed by this checkpoint.

The next acceptance event is the first run of the dedicated Node proof job on a runner that can
install the pinned NDK. If Node itself fails, the configure/make logs become the source of truth for
the smallest next fix.


## Part 34.2.3 execution follow-up — real runner attempt + portable NDK bootstrap

Status: **REAL EXECUTION ATTEMPT RECORDED; BLOCKED AT TOOLCHAIN AVAILABILITY, BEFORE CONFIGURE**

The cross-build wrapper was executed in the current runner rather than leaving Part 34.2.3 as a
static CI-only promise. `part34_node_execute_cross_build.sh` invokes the real provisioner, preserves
its complete stdout/stderr, classifies the first failure without converting it into success, and
atomically emits `vibecoder-part34-node-execution-attempt.json` on both success and failure.

The current runner's real result is preserved under `docs/evidence/`:

- `part34_2_3_current_runner_execution.log`
- `part34_2_3_current_runner_execution.json`

It failed at `android_ndk_root_missing`, classified as `toolchain_unavailable`. Configure, make,
binary staging, APK packaging and device execution therefore remain unproven. This is an environment
availability failure, not evidence of either Node source compatibility or incompatibility.

Additional hardening in this follow-up:

- the GitHub Node proof job now invokes the same execution wrapper, so local and CI failure
  classification have one authority;
- CI artifacts include the execution log and attempt JSON even when configure/build fails;
- `bootstrap_pinned_android_ndk_r28c.sh` supports an offline, out-of-band copy of Google's exact
  Linux r28c archive and refuses it unless byte size, SHA-1, NDK revision, and API-29 ARM64 clang
  identity all match before installation;
- no speculative upstream Android patch is applied before a real configure/compiler/linker failure
  exists for the pinned Node 24.19.0 + NDK 28.2.13676358 + API 29 combination.

## Part 34.2.4 — supervised Node runtime lifecycle source

The package-installed Node executable now has a production-shaped process lifecycle before
OmniRoute is introduced. `LocalProcessRuntime` has a runtime-service scope that is separate from
project command approval: only a pre-registered immutable package executable can be selected, argv
is passed directly without a shell, ambient environment variables are cleared, and the working
directory is a private service directory under the runtime root.

The runtime-service path deliberately reuses the existing supervisor, so stdout/stderr limits,
timeout, cancellation, process-group SIGTERM/SIGKILL escalation and completion events have one
authority. Duplicate service ids are rejected while active. On Android, child setup also requests
`PR_SET_PDEATHSIG(SIGKILL)` and checks for a parent-death race before exec; this prevents a supervised
Node child from intentionally surviving app-process death and being mistaken for current runtime
state after restart.

`AndroidHostRuntime` exposes a narrow Node wrapper: start, active-state inspection, and cancel. The
wrapper re-verifies the Node package component path before launch. This checkpoint still does not
claim that Node bytes exist or execute on Android; binary/APK/device evidence remains the final
external acceptance gate.

## Part 34.2 Android cross-build follow-up — configure-time host architecture authority

A later real CI run progressed through the relative cpufeatures integration and into thousands of
host/target compiles, then exposed a separate cross-build configuration defect: `/usr/bin/g++`
received V8's ARM64 `push_registers_asm.cc` for an `obj.host` target. The failure was therefore not a
Node timeout and not a target-toolchain leak at make time; the generated host graph itself had selected
the wrong architecture.

The provisioner now binds the host C/C++/archive tools at Node configure time, before architecture
selection is emitted into `config.gypi` and GYP recipes. Validation requires x64 host + ARM64 target +
a separate host toolset, and an independent generated-graph guard requires V8's x64 push-register
object in `v8_base_without_compiler.host.mk` while rejecting ARM64 variants. The guard runs before the
expensive compile, converting this observed late failure into an early configuration failure if the
same architecture mix reappears.
