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

print('Part 34.10 compile-log repair regression PASSED')
