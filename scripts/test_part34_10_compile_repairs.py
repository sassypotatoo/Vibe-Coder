#!/usr/bin/env python3
from __future__ import annotations
import json, re, subprocess, sys, tempfile, tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def die(msg): raise SystemExit(f'test_part34_10_compile_repairs: {msg}')
core=tomllib.loads((ROOT/'crates/vibecoder-core/Cargo.toml').read_text())
if core.get('dependencies',{}).get('tokio') != {'workspace': True}: die('vibecoder_core_tokio_direct_dependency_missing')
lock=(ROOT/'Cargo.lock').read_text()
start=lock.index('name = "vibecoder-core"'); end=lock.index('\n[[package]]', start)
if ' "tokio",' not in lock[start:end]: die('vibecoder_core_lock_dependency_missing_tokio')
lib=(ROOT/'crates/vibecoder-core/src/lib.rs').read_text()
if 'let (project, session_id, mut conversation, checkpoint) = {' in lib: die('known_unused_mut_regression_present')

omni=(ROOT/'crates/vibecoder-android-host/src/omniroute_service.rs').read_text()
if 'ProcessRuntime, ProcessTermination' not in omni: die('omniroute_process_runtime_trait_import_missing')
if 'last_profile' in omni: die('omniroute_unused_last_profile_regression_present')
if 'self.process_runtime.cancel(process_id)' not in omni: die('omniroute_cancel_contract_missing')
provision=(ROOT/'scripts/provision_node_android.sh').read_text()
generate='make -j1 V=1 PYTHON=python3 out/Makefile'
preflight='verify_node_android_toolchain_split.py" preflight'
if generate not in provision: die('node_generated_makefile_materialization_missing')
if preflight not in provision: die('node_toolchain_split_preflight_missing')
if provision.index(generate) > provision.index(preflight): die('node_toolchain_preflight_runs_before_generated_makefile_exists')
if 'make -j"$JOBS" V=1' not in provision: die('node_verbose_build_required_for_toolchain_evidence_missing')
configure_bind='CC_host="$HOST_CC" CXX_host="$HOST_CXX" AR_host="$HOST_AR"'
if configure_bind not in provision: die('node_configure_time_host_toolchain_binding_missing')
if provision.index(configure_bind) > provision.index('./android-configure "$NDK_ROOT" "$API" arm64'):
    die('node_configure_time_host_toolchain_binding_order_invalid')
if 'node_android_generated_makefile_failed:' not in provision: die('node_generated_makefile_failure_not_fail_closed')
wrapper=(ROOT/'scripts/part34_node_execute_cross_build.sh').read_text()
if 'build_graph_generation_failed' not in wrapper or 'node_android_generated_makefile_(failed|missing)' not in wrapper:
    die('node_generated_makefile_failure_classification_missing')
verify=ROOT/'scripts/verify_node_android_toolchain_split.py'
with tempfile.TemporaryDirectory() as td:
    d=Path(td); ndk=d/'ndk'; (ndk/'bin').mkdir(parents=True); host=d/'host'; host.mkdir()
    for f in [host/'gcc',host/'g++',ndk/'bin'/'aarch64-linux-android29-clang',ndk/'bin'/'aarch64-linux-android29-clang++']:
        f.write_text('#!/bin/sh\nexit 0\n'); f.chmod(0o755)
    make=d/'Makefile'; make.write_text('CC.host = bad\nCXX.host = bad\nCC.target = bad\nCXX.target = bad\n')
    args=[sys.executable,str(verify),'preflight',str(make),str(ndk),str(host/'gcc'),str(host/'g++'),str(ndk/'bin'/'aarch64-linux-android29-clang'),str(ndk/'bin'/'aarch64-linux-android29-clang++')]
    if subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode: die('toolchain_preflight_fixture_failed')
    bad=d/'bad.log'; bad.write_text(f"{ndk/'bin'/'aarch64-linux-android29-clang++'} -o /x/obj.host/v8/a.o a.cc -c\n")
    args=[sys.executable,str(verify),'log',str(bad),str(host/'gcc'),str(host/'g++'),str(ndk/'bin'/'aarch64-linux-android29-clang'),str(ndk/'bin'/'aarch64-linux-android29-clang++')]
    if subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode == 0: die('old_android_compiler_for_obj_host_not_rejected')
    good=d/'good.log'; good.write_text(f"{host/'g++'} -o /x/obj.host/v8/a.o a.cc -c\n{ndk/'bin'/'aarch64-linux-android29-clang++'} -o /x/obj.target/v8/b.o b.cc -c\n")
    args=[sys.executable,str(verify),'log',str(good),str(host/'gcc'),str(host/'g++'),str(ndk/'bin'/'aarch64-linux-android29-clang'),str(ndk/'bin'/'aarch64-linux-android29-clang++'),'--require-observed']
    if subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode: die('correct_host_target_split_fixture_rejected')

# Node configure must detect the CI machine as x64 from CC_host before GYP generates V8 host
# sources. The previous failure misdetected host_arch=arm64 and fed ARM64 inline asm to /usr/bin/g++.
configure_verify=ROOT/'scripts/verify_node_android_configure_output.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-config-host-arch-') as td:
    d=Path(td); makefile=d/'Makefile'; makefile.write_text('all:\n')
    good=d/'config-good.gypi'; good.write_text("# generated\n{'variables': {'host_arch': 'x64', 'target_arch': 'arm64', 'want_separate_host_toolset': 1}}\n")
    result=subprocess.run([sys.executable,str(configure_verify),str(good),str(makefile)], text=True, capture_output=True)
    if result.returncode: die('node_configure_host_arch_fixture_failed:'+result.stderr.strip())
    bad=d/'config-bad.gypi'; bad.write_text("{'variables': {'host_arch': 'arm64', 'target_arch': 'arm64', 'want_separate_host_toolset': 1}}\n")
    result=subprocess.run([sys.executable,str(configure_verify),str(bad),str(makefile)], text=True, capture_output=True)
    if result.returncode == 0 or 'host_arch_mismatch' not in result.stderr:
        die('node_configure_arm64_host_misdetection_not_rejected')

host_arch_graph=ROOT/'scripts/verify_node_android_host_arch_graph.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-host-arch-graph-') as td:
    out=Path(td)/'out'; hostdir=out/'tools/v8_gypfiles'; hostdir.mkdir(parents=True)
    hostmk=hostdir/'v8_base_without_compiler.host.mk'
    hostmk.write_text('OBJS := $(obj).host/v8_base_without_compiler/deps/v8/src/heap/base/asm/x64/push_registers_asm.o\n', encoding='utf-8')
    result=subprocess.run([sys.executable,str(host_arch_graph),str(out)], text=True, capture_output=True)
    if result.returncode: die('node_host_arch_graph_fixture_failed:'+result.stderr.strip())
    payload=json.loads(result.stdout.strip())
    if payload.get('host_arch') != 'x64' or payload.get('node_android_host_arch_graph') != 'VERIFIED':
        die('node_host_arch_graph_evidence_invalid')
    hostmk.write_text('OBJS := $(obj).host/v8_base_without_compiler/deps/v8/src/heap/base/asm/arm64/push_registers_asm.o\n', encoding='utf-8')
    result=subprocess.run([sys.executable,str(host_arch_graph),str(out)], text=True, capture_output=True)
    if result.returncode == 0 or 'host_push_register_arch_mismatch:arm64' not in result.stderr:
        die('node_arm64_push_register_host_graph_not_rejected')
if 'verify_node_android_host_arch_graph.py' not in provision: die('node_host_arch_graph_verifier_not_invoked')
host_graph_call='verify_node_android_host_arch_graph.py" "$WORK/out"'
if host_graph_call not in provision: die('node_host_arch_graph_verifier_wrong_root')
if provision.index(host_graph_call) < provision.index('make -j1 V=1 PYTHON=python3 out/Makefile'):
    die('node_host_arch_graph_verified_before_gyp_generation')
if provision.index(host_graph_call) > provision.index('make -j"$JOBS" V=1'):
    die('node_host_arch_graph_verified_after_expensive_compile')
if 'node_android_host_arch_graph_invalid' not in provision: die('node_host_arch_graph_fail_closed_missing')
if 'host_arch_graph_invalid' not in wrapper: die('node_host_arch_graph_failure_classification_missing')

# Node 24.19.0 Android GYP regression: remove only the actual Android AArch64 flag proven to leak
# into obj.host recipes. Target makefiles must remain byte-for-byte untouched.
sanitize=ROOT/'scripts/sanitize_node_android_host_makefiles.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-host-flags-') as td:
    out=Path(td)/'out'; out.mkdir()
    host=out/'node_js2c.host.mk'; target=out/'node.target.mk'
    host.write_text('CFLAGS_CC := -Wall -mbranch-protection=standard -mbranch-protection=standardized -O3\n', encoding='utf-8')
    target_original='CFLAGS_CC := -Wall -mbranch-protection=standard -O3\n'
    target.write_text(target_original, encoding='utf-8')
    result=subprocess.run([sys.executable,str(sanitize),str(out)], text=True, capture_output=True)
    if result.returncode: die('host_flag_sanitizer_fixture_failed:'+result.stderr.strip())
    host_after=host.read_text(encoding='utf-8')
    if ' -mbranch-protection=standard ' in host_after: die('host_target_flag_not_removed')
    if '-mbranch-protection=standardized' not in host_after: die('host_flag_sanitizer_overmatched_near_token')
    if target.read_text(encoding='utf-8') != target_original: die('target_makefile_modified_by_host_sanitizer')
    payload=json.loads(result.stdout.strip())
    if payload.get('node_android_host_makefile_sanitize') != 'VERIFIED' or payload.get('flag_replacements') != 1:
        die('host_flag_sanitizer_evidence_invalid')
with tempfile.TemporaryDirectory(prefix='vibecoder-node-host-flags-clean-') as td:
    out=Path(td)/'out'; out.mkdir()
    host=out/'node_js2c.host.mk'; target=out/'node.target.mk'
    host_original='CFLAGS_CC := -Wall -O3\n'
    target_original='CFLAGS_CC := -Wall -mbranch-protection=standard -O3\n'
    host.write_text(host_original, encoding='utf-8')
    target.write_text(target_original, encoding='utf-8')
    result=subprocess.run([sys.executable,str(sanitize),str(out)], text=True, capture_output=True)
    if result.returncode:
        die('host_flag_sanitizer_clean_graph_rejected:'+result.stderr.strip())
    if host.read_text(encoding='utf-8') != host_original:
        die('host_flag_sanitizer_clean_graph_mutated')
    if target.read_text(encoding='utf-8') != target_original:
        die('target_makefile_modified_by_clean_host_sanitizer')
    payload=json.loads(result.stdout.strip())
    if payload.get('node_android_host_makefile_sanitize') != 'VERIFIED' or payload.get('flag_replacements') != 0 or payload.get('sanitization_mode') != 'already_clean':
        die('host_flag_sanitizer_clean_graph_evidence_invalid')
if 'sanitize_node_android_host_makefiles.py' not in provision: die('node_host_flag_sanitizer_not_invoked')
if provision.index(generate) > provision.index('sanitize_node_android_host_makefiles.py'): die('node_host_flag_sanitizer_runs_before_gyp_makefiles_exist')
if provision.index('sanitize_node_android_host_makefiles.py') > provision.index(preflight): die('node_toolchain_preflight_runs_before_host_flag_sanitizer')
if 'node_android_host_makefile_sanitize_failed' not in provision: die('node_host_flag_sanitizer_not_fail_closed')
if 'host_target_flag_sanitize_failed' not in wrapper: die('node_host_flag_sanitize_failure_classification_missing')

# Latest CI Rust compile regression: runtime service arguments reject control characters with the
# standard predicate; no orphan helper symbol may be referenced. GatewayChatMessage is test-only.
process_local=(ROOT/'crates/vibecoder-process-local/src/lib.rs').read_text()
if 'value.chars().any(char::is_control)' not in process_local: die('runtime_service_control_character_guard_missing')
if 'is_forbidden_control' in process_local: die('undefined_is_forbidden_control_regression_present')
gateway_chat=(ROOT/'crates/vibecoder-gateway-omniroute/src/chat.rs').read_text()
production_head=gateway_chat.split('#[cfg(test)]',1)[0]
if 'GatewayChatMessage' in production_head: die('gateway_chat_message_test_only_import_warning_regressed')
if 'use vibecoder_gateway_contract::GatewayChatMessage;' not in gateway_chat: die('gateway_chat_message_test_import_missing')

# Latest Node Android linker regression: Node 24.19.0's vendored zlib calls
# android_getCpuFeatures under ARMV8_OS_ANDROID. Absolute NDK source paths cannot be fed directly
# to GYP because its Make backend objectifies them into impossible obj.target/...//usr/local paths.
# Stage the exact NDK cpufeatures source/header inside the temporary Node tree and use only relative
# GYP paths.
cpupatch=ROOT/'scripts/patch_node_android_zlib_cpufeatures.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-cpufeatures-') as td:
    base=Path(td); node=base/'node'; target=node/'deps/zlib/zlib.gyp'; target.parent.mkdir(parents=True)
    ndk=base/'ndk-cpufeatures'; ndk.mkdir()
    (ndk/'cpu-features.c').write_text('int android_getCpuFeatures(void) { return 0; }\n', encoding='utf-8')
    (ndk/'cpu-features.h').write_text('int android_getCpuFeatures(void);\n', encoding='utf-8')
    target.write_text("""{
  'targets': [{
    'target_name': 'zlib',
    'conditions': [
      ['OS==\"android\"', { 'defines': [ 'ARMV8_OS_ANDROID' ] }],
            # Incorporate optimizations where possible.
    ],
  }],
}
""", encoding='utf-8')
    result=subprocess.run([sys.executable,str(cpupatch),str(node),str(ndk)], text=True, capture_output=True)
    if result.returncode: die('node_cpufeatures_patch_fixture_failed:'+result.stderr.strip())
    patched=target.read_text(encoding='utf-8')
    for token in ('vibecoder-node-24.19.0-android-zlib-cpufeatures-v2',
                  '<(ZLIB_ROOT)/vibecoder-android-cpufeatures/cpu-features.c',
                  '<(ZLIB_ROOT)/vibecoder-android-cpufeatures'):
        if token not in patched: die('node_cpufeatures_patch_output_missing:'+token)
    staged=node/'deps/zlib/vibecoder-android-cpufeatures'
    if (staged/'cpu-features.c').read_bytes() != (ndk/'cpu-features.c').read_bytes(): die('node_cpufeatures_staged_source_mismatch')
    if (staged/'cpu-features.h').read_bytes() != (ndk/'cpu-features.h').read_bytes(): die('node_cpufeatures_staged_header_mismatch')
    if '/sources/android/cpufeatures/cpu-features.c' in patched: die('node_cpufeatures_absolute_ndk_source_regressed')
    again=subprocess.run([sys.executable,str(cpupatch),str(node),str(ndk)], text=True, capture_output=True)
    if again.returncode == 0 or 'node_android_cpufeatures_patch_already_applied' not in again.stderr:
        die('node_cpufeatures_patch_double_apply_not_rejected')
if 'patch_node_android_zlib_cpufeatures.py' not in provision: die('node_cpufeatures_patch_not_invoked')
patch_call='patch_node_android_zlib_cpufeatures.py" "$WORK" "$NDK_CPUFEATURES_DIR"'
if patch_call not in provision: die('node_cpufeatures_patch_ndk_stage_argument_missing')
if provision.index('patch_node_android_zlib_cpufeatures.py') > provision.index('./android-configure "$NDK_ROOT" "$API" arm64'): die('node_cpufeatures_patch_runs_after_configure')
for token in ('android_ndk_cpufeatures_source_missing:', 'android_ndk_cpufeatures_header_missing:',
              'node_android_cpufeatures_patch_failed:'):
    if token not in provision: die('node_cpufeatures_provision_guard_missing:'+token)
if 'android_ndk_cpufeatures_missing' not in wrapper or 'node_android_zlib_cpufeatures_patch_failed' not in wrapper:
    die('node_cpufeatures_failure_classification_missing')

# Generated graph must now contain the relative staged object exactly once in TARGET := zlib, never
# the old absolute NDK-derived object path, and never any host recipe.
graph_verify=ROOT/'scripts/verify_node_android_cpufeatures_integration.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-cpufeatures-graph-') as td:
    out=Path(td)/'out'; out.mkdir()
    target=out/'deps_zlib_zlib.target.mk'
    host=out/'node_js2c.host.mk'
    target.write_text("""# generated zlib target
TOOLSET := target
TARGET := zlib
INCS_Release := -Ideps/zlib/vibecoder-android-cpufeatures
OBJS := $(obj).target/zlib/deps/zlib/vibecoder-android-cpufeatures/cpu-features.o
""", encoding='utf-8')
    host.write_text('TOOLSET := host\nTARGET := node_js2c\n', encoding='utf-8')
    result=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if result.returncode: die('node_cpufeatures_generated_graph_fixture_failed:'+result.stderr.strip())
    payload=json.loads(result.stdout.strip())
    if payload.get('node_android_cpufeatures_generated_graph') != 'VERIFIED': die('node_cpufeatures_generated_graph_evidence_missing')
    if payload.get('absolute_ndk_object_regression') is not False: die('node_cpufeatures_absolute_graph_evidence_invalid')
    host.write_text('TOOLSET := host\nTARGET := node_js2c\nOBJS := deps/zlib/vibecoder-android-cpufeatures/cpu-features.o\n', encoding='utf-8')
    leaked=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if leaked.returncode == 0 or 'cpufeatures_object_leaked_into_host_graph' not in leaked.stderr:
        die('node_cpufeatures_host_graph_leak_not_rejected')
    host.write_text('TOOLSET := host\nTARGET := node_js2c\n', encoding='utf-8')
    target.write_text('TOOLSET := target\nTARGET := zlib\n# zlib target missing cpufeatures object\n', encoding='utf-8')
    missing=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if missing.returncode == 0 or 'cpufeatures_object_missing_from_target_graph' not in missing.stderr:
        die('node_cpufeatures_missing_generated_target_not_rejected')
    target.write_text("""TOOLSET := target
TARGET := zlib
INCS_Release := -I/ndk/sources/android/cpufeatures
OBJS := $(obj).target/zlib//usr/local/lib/android/sdk/ndk/28.2.13676358/sources/android/cpufeatures/cpu-features.o
""", encoding='utf-8')
    absolute=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if absolute.returncode == 0 or 'cpufeatures_absolute_ndk_object_regression' not in absolute.stderr:
        die('node_cpufeatures_absolute_ndk_object_not_rejected')
    target.write_text("""TOOLSET := target
TARGET := not_zlib
INCS_Release := -Ideps/zlib/vibecoder-android-cpufeatures
OBJS := deps/zlib/vibecoder-android-cpufeatures/cpu-features.o
""", encoding='utf-8')
    wrong_target=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if wrong_target.returncode == 0 or 'cpufeatures_object_in_non_zlib_target' not in wrong_target.stderr:
        die('node_cpufeatures_non_zlib_target_not_rejected')
if 'verify_node_android_cpufeatures_integration.py' not in provision: die('node_cpufeatures_generated_graph_verifier_not_invoked')
graph_call='verify_node_android_cpufeatures_integration.py" "$WORK/out"'
if graph_call not in provision: die('node_cpufeatures_generated_graph_verifier_wrong_root')
if provision.index(graph_call) < provision.index('make -j1 V=1 PYTHON=python3 out/Makefile'): die('node_cpufeatures_graph_verified_before_gyp_generation')
if provision.index(graph_call) > provision.index('sanitize_node_android_host_makefiles.py'): die('node_cpufeatures_graph_verified_after_host_sanitizer')
if 'node_android_cpufeatures_generated_graph_invalid' not in provision: die('node_cpufeatures_generated_graph_fail_closed_missing')
if 'build_graph_integration_invalid' not in wrapper: die('node_cpufeatures_generated_graph_failure_classification_missing')
if 'build_graph_source_path_invalid' not in wrapper or 'node_android_cpufeatures_absolute_object_path_invalid' not in wrapper:
    die('node_cpufeatures_absolute_object_failure_classification_missing')

# Part 34.10.10 automatic chat routing: normal questions remain direct model chat; clear coding
# mutations enter the already-built single-shot agent action controller. The Android runtime must
# attach the checkpoint store required by rollback-safe agent actions, and Stop must cancel both
# direct model and Jcode action turns.
host_cargo=tomllib.loads((ROOT/'crates/vibecoder-android-host/Cargo.toml').read_text())
host_deps=host_cargo.get('dependencies',{})
for dep in ('vibecoder-checkpoint-local','vibecoder-routing'):
    if dep not in host_deps: die('android_agent_routing_dependency_missing:'+dep)
ffi=(ROOT/'crates/vibecoder-android-host/src/app_controller_ffi.rs').read_text()
for token in (
    '.with_checkpoint_store(checkpoint_store)',
    'fn classify_chat_route(prompt: &str) -> ChatRoute',
    'ChatRoute::ModelChat',
    'ChatRoute::AgentAction',
    'run_persisted_model_conversation_turn_cancellable(',
    'run_persisted_agent_action_turn(',
    'turn_kind: "model_chat"',
    'turn_kind: "agent_action"',
    'successful_mutation_tool_calls: Some(outcome.successful_mutation_tool_calls())',
    'cancel_persisted_conversation_turn(project_id, conversation_id)',
):
    if token not in ffi: die('android_agent_routing_contract_missing:'+token)
if 'run_explicit_agent_loop' in ffi or 'run_persisted_explicit' in ffi:
    die('automatic_chat_routing_must_not_enable_outer_loop')
# Lock the concrete examples that motivated this wiring inside Rust unit tests, so the next real
# cargo test/build cannot silently change intended routing semantics.
for prompt in (
    'Add a Start button to the home screen',
    'Home screen pe Start button bana do',
    'How do I add a button in Android?',
    'What is Rust ownership?',
):
    if prompt not in ffi: die('android_agent_routing_regression_prompt_missing:'+prompt)

print('Part 34.10 compile-log repair regression PASSED')

# Part 34.10.11 Android libc signature + Node CI runtime budget repair.
# Android libc::renameat2 expects an unsigned flags argument; keep the atomic exchange
# semantics while using the libc ABI type rather than a host-dependent integer constant type.
checkpoint_local=(ROOT/'crates/vibecoder-checkpoint-local/src/lib.rs').read_text()
if 'libc::RENAME_EXCHANGE as libc::c_uint' not in checkpoint_local:
    die('android_renameat2_flags_type_cast_missing')
if re.search(r'libc::renameat2\([\s\S]{0,400}?\n\s*libc::RENAME_EXCHANGE,', checkpoint_local):
    die('android_renameat2_untyped_flags_regression')

workflow=(ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
node_runtime_workflow=(ROOT/'.github/workflows/node-runtime-proof.yml').read_text()
if 'node-android-proof-build:' in workflow:
    die('node_android_ci_job_must_be_detached_from_normal_app_ci')
node_job=re.search(
    r'(?ms)^  node-android-proof-build:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)',
    node_runtime_workflow,
)
if not node_job:
    die('node_android_ci_job_missing')
node_body=node_job.group('body')
match=re.search(r'^    timeout-minutes:\s*(\d+)\s*$', node_body, re.M)
if not match or int(match.group(1)) < 360:
    die('node_android_ci_timeout_budget_too_small')
if 'export VIBECODER_BUILD_JOBS="4"' not in node_body:
    die('node_android_parallelism_contract_changed')

print('Part 34.10.11 Android libc + Node timeout regression PASSED')

print('Part 34.10.13 Node clean-host sanitizer regression PASSED')
print('Part 34.10.14 Node CI throughput/timeout regression PASSED')
print('Part 34.10.15 Node on-demand CI isolation regression PASSED')
