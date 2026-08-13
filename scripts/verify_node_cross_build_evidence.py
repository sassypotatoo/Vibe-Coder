#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

EXPECTED_NODE_VERSION = '24.19.0'
EXPECTED_NODE_SOURCE_SHA256 = 'f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f'
EXPECTED_NDK_REVISION = '28.2.13676358'


def fail(message: str) -> None:
    raise SystemExit(f'verify_node_cross_build_evidence: {message}')


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    if len(sys.argv) != 3:
        fail('usage: verify_node_cross_build_evidence.py NODE EVIDENCE_JSON')
    node = Path(sys.argv[1]).resolve()
    evidence_path = Path(sys.argv[2]).resolve()
    if not node.is_file() or node.stat().st_size <= 0:
        fail('node_missing_or_empty')
    if not evidence_path.is_file() or evidence_path.stat().st_size <= 0:
        fail('evidence_missing_or_empty')
    try:
        evidence = json.loads(evidence_path.read_text(encoding='utf-8'))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f'evidence_invalid_json:{type(exc).__name__}')
    if evidence.get('schema') != 1 or evidence.get('part') != 34 or evidence.get('step') != '34.2.3':
        fail('evidence_identity_mismatch')
    if evidence.get('claim') != 'cross_build_candidate_only_not_device_execution':
        fail('evidence_claim_mismatch')
    info = evidence.get('node') or {}
    if info.get('version') != EXPECTED_NODE_VERSION:
        fail('node_version_mismatch')
    if info.get('source_archive_sha256') != EXPECTED_NODE_SOURCE_SHA256:
        fail('node_source_sha256_mismatch')
    if info.get('output_sha256') != sha256_file(node):
        fail('node_output_sha256_mismatch')
    elf_check = subprocess.run(
        [sys.executable, str(ROOT / 'scripts/verify_android_elf.py'), str(node)],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=30, check=False,
    )
    if elf_check.returncode != 0:
        fail(f'node_bytes_elf_verification_failed:{elf_check.stdout.strip()}')
    if info.get('device_execution_proven') is not False:
        fail('device_execution_claim_must_remain_false')
    elf = info.get('elf') or {}
    for key in ('elf64', 'aarch64', 'et_dyn', 'page16', 'android_linker_compatible'):
        if elf.get(key) is not True:
            fail(f'elf_evidence_missing:{key}')
    target = evidence.get('target') or {}
    if target.get('os') != 'android' or target.get('abi') != 'arm64-v8a' or target.get('libc') != 'bionic':
        fail('target_identity_mismatch')
    if target.get('api') != 29:
        fail(f'android_api_mismatch:{target.get("api")!r}')
    toolchain = evidence.get('toolchain') or {}
    if toolchain.get('android_ndk_revision') != EXPECTED_NDK_REVISION:
        fail('android_ndk_revision_mismatch')
    print(json.dumps({'node_cross_build_evidence': 'VERIFIED', 'node_sha256': info['output_sha256']}, separators=(',', ':')))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
