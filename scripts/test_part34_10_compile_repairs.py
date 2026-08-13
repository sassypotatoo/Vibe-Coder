#!/usr/bin/env python3
from __future__ import annotations
import json, subprocess, sys, tempfile, tomllib
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
with tempfile.TemporaryDirectory(prefix='vibecoder-node-host-flags-missing-') as td:
    out=Path(td)/'out'; out.mkdir()
    (out/'node_js2c.host.mk').write_text('CFLAGS_CC := -Wall -O3\n', encoding='utf-8')
    (out/'node.target.mk').write_text('CFLAGS_CC := -Wall -O3\n', encoding='utf-8')
    result=subprocess.run([sys.executable,str(sanitize),str(out)], text=True, capture_output=True)
    if result.returncode == 0 or 'proven_host_target_flag_not_found' not in result.stderr:
        die('host_flag_sanitizer_missing_flag_not_fail_closed')
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

# Latest Node Android linker regression: Node 24.19.0's vendored zlib calls android_getCpuFeatures
# under ARMV8_OS_ANDROID, so the NDK cpufeatures implementation must be added deterministically to
# the zlib static library before GYP generates makefiles.
cpupatch=ROOT/'scripts/patch_node_android_zlib_cpufeatures.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-cpufeatures-') as td:
    node=Path(td)/'node'; target=node/'deps/zlib/zlib.gyp'; target.parent.mkdir(parents=True)
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
    result=subprocess.run([sys.executable,str(cpupatch),str(node)], text=True, capture_output=True)
    if result.returncode: die('node_cpufeatures_patch_fixture_failed:'+result.stderr.strip())
    patched=target.read_text(encoding='utf-8')
    for token in ('vibecoder-node-24.19.0-android-zlib-cpufeatures-v1',
                  '<(android_ndk_path)/sources/android/cpufeatures/cpu-features.c',
                  '<(android_ndk_path)/sources/android/cpufeatures'):
        if token not in patched: die('node_cpufeatures_patch_output_missing:'+token)
    again=subprocess.run([sys.executable,str(cpupatch),str(node)], text=True, capture_output=True)
    if again.returncode == 0 or 'node_android_cpufeatures_patch_already_applied' not in again.stderr:
        die('node_cpufeatures_patch_double_apply_not_rejected')
if 'patch_node_android_zlib_cpufeatures.py' not in provision: die('node_cpufeatures_patch_not_invoked')
if provision.index('patch_node_android_zlib_cpufeatures.py') > provision.index('./android-configure "$NDK_ROOT" "$API" arm64'): die('node_cpufeatures_patch_runs_after_configure')
for token in ('android_ndk_cpufeatures_source_missing:', 'android_ndk_cpufeatures_header_missing:',
              'node_android_cpufeatures_patch_failed:'):
    if token not in provision: die('node_cpufeatures_provision_guard_missing:'+token)
if 'android_ndk_cpufeatures_missing' not in wrapper or 'node_android_zlib_cpufeatures_patch_failed' not in wrapper:
    die('node_cpufeatures_failure_classification_missing')

# Deep-audit hardening: prove the source-level zlib patch survived GYP generation before the expensive
# compile starts. The generated graph must contain cpu-features.c in exactly one zlib target and in
# zero host makefiles.
graph_verify=ROOT/'scripts/verify_node_android_cpufeatures_integration.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-cpufeatures-graph-') as td:
    out=Path(td)/'out'; out.mkdir()
    target=out/'deps_zlib_zlib.target.mk'
    host=out/'node_js2c.host.mk'
    target.write_text("""# generated zlib target
INCS := -I/ndk/sources/android/cpufeatures
SRCS := /ndk/sources/android/cpufeatures/cpu-features.c
""", encoding='utf-8')
    host.write_text('# generated host target\n', encoding='utf-8')
    result=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if result.returncode: die('node_cpufeatures_generated_graph_fixture_failed:'+result.stderr.strip())
    if 'node_android_cpufeatures_generated_graph' not in result.stdout: die('node_cpufeatures_generated_graph_evidence_missing')
    host.write_text('/ndk/sources/android/cpufeatures/cpu-features.c\n', encoding='utf-8')
    leaked=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if leaked.returncode == 0 or 'cpufeatures_source_leaked_into_host_graph' not in leaked.stderr:
        die('node_cpufeatures_host_graph_leak_not_rejected')
    host.write_text('# clean host\n', encoding='utf-8')
    target.write_text('# zlib target missing source\n', encoding='utf-8')
    missing=subprocess.run([sys.executable,str(graph_verify),str(out)], text=True, capture_output=True)
    if missing.returncode == 0 or 'cpufeatures_source_missing_from_target_graph' not in missing.stderr:
        die('node_cpufeatures_missing_generated_target_not_rejected')
if 'verify_node_android_cpufeatures_integration.py' not in provision: die('node_cpufeatures_generated_graph_verifier_not_invoked')
graph_call='verify_node_android_cpufeatures_integration.py" "$WORK/out"'
if graph_call not in provision: die('node_cpufeatures_generated_graph_verifier_wrong_root')
if provision.index(graph_call) < provision.index('make -j1 V=1 PYTHON=python3 out/Makefile'): die('node_cpufeatures_graph_verified_before_gyp_generation')
if provision.index(graph_call) > provision.index('sanitize_node_android_host_makefiles.py'): die('node_cpufeatures_graph_verified_after_host_sanitizer')
if 'node_android_cpufeatures_generated_graph_invalid' not in provision: die('node_cpufeatures_generated_graph_fail_closed_missing')
if 'build_graph_integration_invalid' not in wrapper: die('node_cpufeatures_generated_graph_failure_classification_missing')

print('Part 34.10 compile-log repair regression PASSED')
