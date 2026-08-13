#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_NODE_VERSION = '24.19.0'
EXPECTED_NODE_SOURCE_SHA256 = 'f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f'
EXPECTED_NDK_REVISION = '28.2.13676358'


def fail(message: str) -> None:
    raise SystemExit(f'write_node_cross_build_evidence: {message}')


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def read_ndk_revision(ndk_root: Path) -> str:
    props = ndk_root / 'source.properties'
    if not props.is_file():
        fail('android_ndk_source_properties_missing')
    match = re.search(r'^\s*Pkg\.Revision\s*=\s*([^\s]+)\s*$', props.read_text(encoding='utf-8'), re.MULTILINE)
    if not match:
        fail('android_ndk_revision_unreadable')
    return match.group(1)


def verify_elf(node: Path) -> dict[str, object]:
    result = subprocess.run(
        [sys.executable, str(ROOT / 'scripts/verify_android_elf.py'), str(node)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
        check=False,
    )
    if result.returncode != 0:
        fail(f'node_android_elf_verification_failed:{result.stdout.strip()}')
    parsed: dict[str, object] = {}
    for line in result.stdout.splitlines():
        if '=' not in line:
            continue
        key, value = line.split('=', 1)
        parsed[key] = {'True': True, 'False': False, 'None': None}.get(value, value)
    for required in ('elf64', 'aarch64', 'et_dyn', 'page16', 'android_linker_compatible'):
        if parsed.get(required) is not True:
            fail(f'node_android_elf_evidence_missing:{required}')
    return parsed


def main() -> int:
    if len(sys.argv) != 8:
        fail('usage: write_node_cross_build_evidence.py NODE ARCHIVE NDK_ROOT API CONFIGURE_LOG BUILD_LOG OUTPUT_JSON')
    node = Path(sys.argv[1]).resolve()
    archive = Path(sys.argv[2]).resolve()
    ndk_root = Path(sys.argv[3]).resolve()
    api_text = sys.argv[4]
    configure_log = Path(sys.argv[5]).resolve()
    build_log = Path(sys.argv[6]).resolve()
    output = Path(sys.argv[7]).resolve()
    try:
        api = int(api_text)
    except ValueError:
        fail(f'android_api_not_integer:{api_text}')
    if api < 29:
        fail(f'android_api_below_vibecoder_min_sdk:{api}')
    for path, label in ((node, 'node'), (archive, 'node_source_archive'), (configure_log, 'configure_log'), (build_log, 'build_log')):
        if not path.is_file() or path.stat().st_size <= 0:
            fail(f'{label}_missing_or_empty:{path}')
    if sha256_file(archive) != EXPECTED_NODE_SOURCE_SHA256:
        fail('node_source_sha256_mismatch')
    ndk_revision = read_ndk_revision(ndk_root)
    if ndk_revision != EXPECTED_NDK_REVISION:
        fail(f'android_ndk_revision_mismatch:expected={EXPECTED_NDK_REVISION}:actual={ndk_revision}')
    elf = verify_elf(node)

    evidence = {
        'schema': 1,
        'part': 34,
        'step': '34.2.3',
        'mode': 'node_android_cross_build',
        'claim': 'cross_build_candidate_only_not_device_execution',
        'node': {
            'version': EXPECTED_NODE_VERSION,
            'source_url': f'https://nodejs.org/download/release/v{EXPECTED_NODE_VERSION}/node-v{EXPECTED_NODE_VERSION}.tar.xz',
            'source_archive_sha256': EXPECTED_NODE_SOURCE_SHA256,
            'output_size': node.stat().st_size,
            'output_sha256': sha256_file(node),
            'elf': elf,
            'device_execution_proven': False,
        },
        'target': {
            'os': 'android',
            'abi': 'arm64-v8a',
            'configure_arch': 'arm64',
            'api': api,
            'libc': 'bionic',
        },
        'toolchain': {
            'android_ndk_revision': ndk_revision,
            'android_ndk_revision_required': EXPECTED_NDK_REVISION,
            'ndk_r28_or_newer_16k_default_expected_but_verified': True,
        },
        'logs': {
            'configure_sha256': sha256_file(configure_log),
            'make_sha256': sha256_file(build_log),
        },
        'source_contract': {
            'node_provisioner_sha256': sha256_file(ROOT / 'scripts/provision_node_android.sh'),
            'android_elf_verifier_sha256': sha256_file(ROOT / 'scripts/verify_android_elf.py'),
            'host_target_toolchain_split_verifier_sha256': sha256_file(ROOT / 'scripts/verify_node_android_toolchain_split.py'),
            'host_makefile_sanitizer_sha256': sha256_file(ROOT / 'scripts/sanitize_node_android_host_makefiles.py'),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(output.suffix + '.tmp')
    temp.write_text(json.dumps(evidence, sort_keys=True, separators=(',', ':')) + '\n', encoding='utf-8')
    os.replace(temp, output)
    print(json.dumps({
        'node_cross_build_evidence': 'PASSED',
        'node_sha256': evidence['node']['output_sha256'],
        'ndk': ndk_revision,
        'api': api,
    }, separators=(',', ':')))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
