#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, sys, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
JCODE = 'lib/arm64-v8a/libvibecoder_jcode_exec.so'
HOST = 'lib/arm64-v8a/libvibecoder_android_host.so'
JNI = 'lib/arm64-v8a/libvibecoder_shell_jni.so'
OMNI_MANIFEST = 'assets/omniroute/bundle/.vibecoder-omniroute-bundle.json'

def fail(message: str) -> None:
    raise SystemExit(f'write_alpha_build_evidence: {message}')

def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()

def load_json(path: Path, label: str) -> dict:
    if not path.is_file() or path.stat().st_size <= 0:
        fail(f'{label}_missing_or_empty')
    try:
        value=json.loads(path.read_text(encoding='utf-8'))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(f'{label}_invalid:{type(exc).__name__}')
    if not isinstance(value, dict):
        fail(f'{label}_not_object')
    return value

def native_claim(evidence: dict, entry: str) -> dict:
    claims=evidence.get('native_entries')
    if not isinstance(claims, list):
        fail('jcode_build_evidence_native_entries_missing')
    for item in claims:
        if isinstance(item, dict) and item.get('entry') == entry:
            return item
    fail('jcode_build_evidence_entry_missing')

def main() -> int:
    if len(sys.argv) != 4:
        fail('usage: write_alpha_build_evidence.py APK JCODE_BUILD_EVIDENCE OUTPUT_JSON')
    apk = Path(sys.argv[1]).resolve()
    jcode_evidence_path = Path(sys.argv[2]).resolve()
    output = Path(sys.argv[3]).resolve()
    if not apk.is_file() or apk.stat().st_size <= 0:
        fail('apk_missing_or_empty')
    jcode_evidence = load_json(jcode_evidence_path, 'jcode_build_evidence')

    with zipfile.ZipFile(apk) as zf:
        names = set(zf.namelist())
        for required in (JNI, HOST, JCODE, OMNI_MANIFEST):
            if required not in names:
                fail(f'required_apk_entry_missing:{required}')
        native = {}
        for entry in (JNI, HOST, JCODE):
            data = zf.read(entry)
            native[entry] = {'size': len(data), 'sha256': sha256_bytes(data)}
        omni_manifest_bytes = zf.read(OMNI_MANIFEST)
        try:
            omni_manifest = json.loads(omni_manifest_bytes.decode('utf-8'))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            fail(f'omniroute_manifest_invalid:{type(exc).__name__}')

    if jcode_evidence.get('mode') != 'jcode' or jcode_evidence.get('application_id') != 'com.vibecoder.shell':
        fail('jcode_build_evidence_identity_mismatch')
    jcode_source=jcode_evidence.get('source') or {}
    if jcode_source.get('checksums_sha256') != sha256_file(ROOT / 'CHECKSUMS.sha256'):
        fail('jcode_build_evidence_source_checkpoint_mismatch')
    jcode_claim=native_claim(jcode_evidence, JCODE)
    if jcode_claim.get('sha256') != native[JCODE]['sha256'] or jcode_claim.get('size') != native[JCODE]['size']:
        fail('jcode_build_evidence_payload_mismatch')

    if omni_manifest.get('component_id') != 'omniroute' or omni_manifest.get('version') != '3.8.50':
        fail('omniroute_manifest_identity_mismatch')
    if omni_manifest.get('profile_id') != 'vibecoder-omniroute-android-backend-v1':
        fail('omniroute_manifest_profile_mismatch')

    evidence = {
        'schema': 1,
        'part': 34,
        'step': '34.10.3-precompile-full-alpha-package',
        'claim': 'base_alpha_apk_with_direct_node_setup_download_not_device_execution',
        'application_id': 'com.vibecoder.shell',
        'apk': {'name': apk.name, 'size': apk.stat().st_size, 'sha256': sha256_file(apk)},
        'native_entries': native,
        'jcode': {
            'version_requirement': '0.73.0',
            'proof_build_evidence_sha256': sha256_file(jcode_evidence_path),
            'payload_bound_to_proof_evidence': True,
            'device_execution_proven': False,
        },
        'node': {
            'version_requirement': '24.19.0',
            'delivery': 'github_release_packageinstaller_split',
            'split': 'node_runtime',
            'release_tag': 'vibecoder-node-runtime-24.19.0-v31',
            'bundled_in_base_apk': False,
            'device_execution_proven': False,
        },
        'omniroute': {
            'version': '3.8.50',
            'profile_id': omni_manifest['profile_id'],
            'manifest_sha256': sha256_bytes(omni_manifest_bytes),
            'tree_sha256': omni_manifest.get('tree_sha256'),
            'file_count': omni_manifest.get('file_count'),
            'total_bytes': omni_manifest.get('total_bytes'),
            'device_service_round_trip_proven': False,
        },
        'source': {
            'checksums_sha256': sha256_file(ROOT / 'CHECKSUMS.sha256'),
            'runtime_inventory_sha256': sha256_file(ROOT / 'config/android-runtime-inventory.json'),
            'payload_provisioning_sha256': sha256_file(ROOT / 'config/android-payload-provisioning.json'),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(output.suffix + '.tmp')
    temp.write_text(json.dumps(evidence, sort_keys=True, separators=(',', ':')) + '\n', encoding='utf-8')
    os.replace(temp, output)
    print(json.dumps({'alpha_build_evidence':'PASSED','apk_sha256':evidence['apk']['sha256']}, separators=(',', ':')))
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
