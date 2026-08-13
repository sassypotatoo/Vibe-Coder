#!/usr/bin/env python3
from __future__ import annotations
import subprocess, sys, tempfile, tomllib
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
verify=ROOT/'scripts/verify_node_android_toolchain_split.py'

process_local=(ROOT/'crates/vibecoder-process-local/src/lib.rs').read_text()
if 'is_forbidden_control' in process_local: die('undefined_is_forbidden_control_regression_present')
if 'value.chars().any(is_forbidden_display_char)' not in process_local: die('runtime_service_arg_shared_display_validator_missing')
chat=(ROOT/'crates/vibecoder-gateway-omniroute/src/chat.rs').read_text()
if '    GatewayChatMessage, GatewayChatRequest' in chat: die('gateway_chat_message_prod_unused_import_regression')
sanitize=ROOT/'scripts/sanitize_node_android_host_makefiles.py'
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
    bad_flag=d/'bad-flag.log'; bad_flag.write_text(f"{host/'g++'} -o /x/obj.host/v8/a.o a.cc -mbranch-protection=standard -c\n")
    args=[sys.executable,str(verify),'log',str(bad_flag),str(host/'gcc'),str(host/'g++'),str(ndk/'bin'/'aarch64-linux-android29-clang'),str(ndk/'bin'/'aarch64-linux-android29-clang++')]
    if subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode == 0: die('android_arm_flag_for_obj_host_not_rejected')
    out=d/'out'/'Release'; out.mkdir(parents=True)
    hostmk=out/'node_js2c.host.mk'; targetmk=out/'node.target.mk'
    hostmk.write_text("\t$(CXX.host) -Wall '-mbranch-protection=standard' -I"+str(ndk)+"/sources/android/cpufeatures -c a.cc\n")
    target_text="\t$(CXX.target) -mbranch-protection=standard -I"+str(ndk)+"/sources/android/cpufeatures -c b.cc\n"
    targetmk.write_text(target_text)
    args=[sys.executable,str(sanitize),str(out),str(ndk)]
    if subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode: die('host_makefile_sanitizer_fixture_failed')
    cleaned=hostmk.read_text()
    if '-mbranch-protection=' in cleaned: die('host_makefile_target_flag_not_removed')
    if 'sources/android/cpufeatures' not in cleaned: die('host_makefile_unproven_ndk_include_was_removed')
    if not cleaned.startswith('\t'): die('host_makefile_recipe_tab_was_damaged')
    if targetmk.read_text() != target_text: die('target_makefile_was_modified_by_host_sanitizer')
print('Part 34.10 compile-log repair regression PASSED')
