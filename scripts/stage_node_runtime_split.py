#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, shutil, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'android/node_runtime/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so'
ASSET = ROOT / 'android/node_runtime/build/generated/assets/node-runtime/manifest.json'
RELEASE_TAG = 'vibecoder-node-runtime-24.19.0-v31'
RUNTIME_APK_NAME = 'vibecoder-node-runtime-arm64-v31.apk'


def fail(msg: str) -> None:
    raise SystemExit('stage_node_runtime_split: ' + msg)


def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


if len(sys.argv) != 3:
    fail('usage: stage_node_runtime_split.py NODE_BINARY NODE_EVIDENCE')
node = Path(sys.argv[1]).resolve()
evidence_path = Path(sys.argv[2]).resolve()
if not node.is_file() or node.stat().st_size <= 0:
    fail('node_binary_missing')
if not evidence_path.is_file():
    fail('node_evidence_missing')
try:
    evidence = json.loads(evidence_path.read_text())
except (OSError, json.JSONDecodeError) as exc:
    fail('node_evidence_invalid:' + type(exc).__name__)
claim = evidence.get('node') or {}
target = evidence.get('target') or {}
actual = sha(node)
if claim.get('version') != '24.19.0':
    fail('node_version_mismatch')
if claim.get('output_sha256') != actual or claim.get('output_size') != node.stat().st_size:
    fail('node_evidence_payload_mismatch')
if target.get('os') != 'android' or target.get('abi') != 'arm64-v8a' or target.get('libc') != 'bionic':
    fail('node_target_mismatch')
subprocess.run([sys.executable, str(ROOT / 'scripts/verify_android_elf.py'), str(node)],
               cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
DEST.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(node, DEST)
os.chmod(DEST, 0o644)
ASSET.parent.mkdir(parents=True, exist_ok=True)
manifest = {
    'schema': 1,
    'component_id': 'node',
    'version': '24.19.0',
    'abi': 'arm64-v8a',
    'libc': 'bionic',
    'file_name': DEST.name,
    'size': DEST.stat().st_size,
    'sha256': sha(DEST),
    'delivery': 'github_release_packageinstaller_split',
    'split': 'node_runtime',
    'release_tag': RELEASE_TAG,
    'runtime_apk': RUNTIME_APK_NAME,
}
ASSET.write_text(json.dumps(manifest, sort_keys=True, separators=(',', ':')) + '\n')
print(json.dumps({'node_runtime_split_staged': 'PASSED', 'sha256': manifest['sha256'], 'size': manifest['size']}, separators=(',', ':')))
