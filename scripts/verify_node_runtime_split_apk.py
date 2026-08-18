#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, sys, zipfile
from pathlib import Path

NODE_ENTRY = 'lib/arm64-v8a/libvibecoder_node_exec.so'
MANIFEST_ENTRY = 'assets/node-runtime/manifest.json'


def fail(msg: str) -> None:
    raise SystemExit('verify_node_runtime_split_apk: ' + msg)


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


if len(sys.argv) != 3:
    fail('usage: verify_node_runtime_split_apk.py RUNTIME_APK EXPECTED_NODE')
apk = Path(sys.argv[1]).resolve()
expected = Path(sys.argv[2]).resolve()
if not apk.is_file() or apk.stat().st_size <= 0:
    fail('runtime_apk_missing')
if not expected.is_file() or expected.stat().st_size <= 0:
    fail('expected_node_missing')
try:
    with zipfile.ZipFile(apk) as archive:
        names = set(archive.namelist())
        if NODE_ENTRY not in names:
            fail('node_entry_missing')
        if MANIFEST_ENTRY not in names:
            fail('runtime_manifest_missing')
        node = archive.read(NODE_ENTRY)
        try:
            manifest = json.loads(archive.read(MANIFEST_ENTRY).decode('utf-8'))
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail('runtime_manifest_invalid')
except zipfile.BadZipFile:
    fail('runtime_apk_not_zip')
node_hash = sha(node)
expected_hash = hashlib.sha256(expected.read_bytes()).hexdigest()
if node_hash != expected_hash:
    fail('node_payload_hash_mismatch')
checks = {
    'component_id': 'node',
    'version': '24.19.0',
    'abi': 'arm64-v8a',
    'libc': 'bionic',
    'file_name': 'libvibecoder_node_exec.so',
    'size': len(node),
    'sha256': node_hash,
    'delivery': 'github_release_packageinstaller_split',
    'split': 'node_runtime',
    'release_tag': 'vibecoder-node-runtime-24.19.0-v31',
    'runtime_apk': 'vibecoder-node-runtime-arm64-v31.apk',
}
for key, value in checks.items():
    if manifest.get(key) != value:
        fail('runtime_manifest_mismatch:' + key)
print(json.dumps({'node_runtime_split_apk': 'PASSED', 'node_sha256': node_hash, 'apk_size': apk.stat().st_size}, sort_keys=True))
