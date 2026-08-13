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
print('Part 34.10 compile-log repair regression PASSED')
