#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shlex
import sys
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f'verify_node_android_toolchain_split: {message}')


def real(path: str) -> Path:
    # Keep the spelling passed to make (for example /usr/bin/g++) so log verification is stable
    # even when the executable is a symlink. Resolve only for trust-boundary containment checks.
    value = Path(os.path.abspath(os.path.expanduser(path)))
    if not value.is_file() or not os.access(value, os.X_OK):
        fail(f'compiler_missing_or_not_executable:{value}')
    return value


def same_or_within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def preflight(args: argparse.Namespace) -> None:
    makefile = Path(args.out_makefile).resolve()
    if not makefile.is_file() or makefile.stat().st_size <= 0:
        fail('out_makefile_missing_or_empty')
    ndk = Path(args.ndk_root).resolve()
    if not ndk.is_dir():
        fail('ndk_root_missing')
    host_cc, host_cxx = real(args.host_cc), real(args.host_cxx)
    target_cc, target_cxx = real(args.target_cc), real(args.target_cxx)
    if same_or_within(host_cc, ndk) or same_or_within(host_cxx, ndk):
        fail('host_compiler_must_not_be_from_android_ndk')
    if not same_or_within(target_cc, ndk) or not same_or_within(target_cxx, ndk):
        fail('target_compiler_must_be_from_android_ndk')
    if host_cc == target_cc or host_cxx == target_cxx:
        fail('host_and_target_compilers_must_differ')
    text = makefile.read_text(encoding='utf-8', errors='strict')
    # GYP's generated out/Makefile is the authority that distinguishes host and target recipes.
    # Command-line make assignments can safely override normal assignments, but not an explicit
    # `override` directive. Fail closed if the generator changes that contract.
    for token in ('CC.host', 'CXX.host', 'CC.target', 'CXX.target'):
        if token not in text:
            fail(f'generated_makefile_toolchain_variable_missing:{token}')
    for token in ('override CC.host', 'override CXX.host', 'override CC.target', 'override CXX.target'):
        if token in text:
            fail(f'generated_makefile_forbids_command_line_toolchain_override:{token}')
    print(json.dumps({
        'node_android_toolchain_split_preflight':'VERIFIED',
        'host_cc':str(host_cc),'host_cxx':str(host_cxx),
        'target_cc':str(target_cc),'target_cxx':str(target_cxx),
    }, separators=(',', ':')))


def command_has(tokens: list[str], compiler: Path) -> bool:
    c = str(compiler)
    name = compiler.name
    return any(tok == c or tok == name for tok in tokens)


def logcheck(args: argparse.Namespace) -> None:
    log = Path(args.build_log).resolve()
    if not log.is_file() or log.stat().st_size <= 0:
        fail('build_log_missing_or_empty')
    host_cc, host_cxx = real(args.host_cc), real(args.host_cxx)
    target_cc, target_cxx = real(args.target_cc), real(args.target_cxx)
    host_seen = target_seen = 0
    for raw in log.read_text(encoding='utf-8', errors='replace').splitlines():
        if ' -c' not in raw or '.o' not in raw:
            continue
        # Strip a possible CI timestamp prefix before shell tokenization.
        line = raw.split('Z ', 1)[-1] if 'Z ' in raw else raw
        try:
            tokens = shlex.split(line)
        except ValueError:
            tokens = line.split()
        if '/obj.host/' in raw:
            host_seen += 1
            if command_has(tokens, target_cc) or command_has(tokens, target_cxx):
                fail('android_target_compiler_used_for_obj_host')
            if not (command_has(tokens, host_cc) or command_has(tokens, host_cxx)):
                fail('expected_host_compiler_not_observed_for_obj_host')
        elif '/obj.target/' in raw:
            target_seen += 1
            if not (command_has(tokens, target_cc) or command_has(tokens, target_cxx)):
                fail('expected_android_compiler_not_observed_for_obj_target')
    if args.require_observed:
        if host_seen == 0:
            fail('no_obj_host_compile_observed')
        if target_seen == 0:
            fail('no_obj_target_compile_observed')
    print(json.dumps({
        'node_android_toolchain_split_log':'VERIFIED',
        'obj_host_compile_count':host_seen,
        'obj_target_compile_count':target_seen,
    }, separators=(',', ':')))


def main() -> int:
    parser=argparse.ArgumentParser()
    sub=parser.add_subparsers(dest='mode', required=True)
    p=sub.add_parser('preflight')
    p.add_argument('out_makefile'); p.add_argument('ndk_root')
    p.add_argument('host_cc'); p.add_argument('host_cxx'); p.add_argument('target_cc'); p.add_argument('target_cxx')
    p.set_defaults(func=preflight)
    p=sub.add_parser('log')
    p.add_argument('build_log'); p.add_argument('host_cc'); p.add_argument('host_cxx'); p.add_argument('target_cc'); p.add_argument('target_cxx')
    p.add_argument('--require-observed', action='store_true')
    p.set_defaults(func=logcheck)
    args=parser.parse_args(); args.func(args); return 0

if __name__ == '__main__':
    raise SystemExit(main())
