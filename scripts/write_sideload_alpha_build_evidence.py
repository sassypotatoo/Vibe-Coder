#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, sys, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
JNI = 'lib/arm64-v8a/libvibecoder_shell_jni.so'
HOST = 'lib/arm64-v8a/libvibecoder_android_host.so'
JCODE = 'lib/arm64-v8a/libvibecoder_jcode_exec.so'
NODE = 'lib/arm64-v8a/libvibecoder_node_exec.so'
OMNI_MANIFEST = 'assets/omniroute/bundle/.vibecoder-omniroute-bundle.json'

def fail(message: str) -> None:
    raise SystemExit('write_sideload_alpha_build_evidence: ' + message)

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
        fail(label + '_missing_or_empty')
    try:
        value = json.loads(path.read_text(encoding='utf-8'))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(label + '_invalid:' + type(exc).__name__)
    if not isinstance(value, dict):
        fail(label + '_not_object')
    return value

def native_claim(evidence: dict, entry: str) -> dict:
    claims = evidence.get('native_entries')
    if not isinstance(claims, list):
        fail('jcode_build_evidence_native_entries_missing')
    for item in claims:
        if isinstance(item, dict) and item.get('entry') == entry:
            return item
    fail('jcode_build_evidence_entry_missing:' + entry)

def main() -> int:
    if len(sys.argv) != 6:
        fail('usage: APK JCODE_BUILD_EVIDENCE NODE_BINARY NODE_CROSS_EVIDENCE OUTPUT_JSON')
    apk = Path(sys.argv[1]).resolve()
    jcode_evidence_path = Path(sys.argv[2]).resolve()
    node_binary = Path(sys.argv[3]).resolve()
    node_evidence_path = Path(sys.argv[4]).resolve()
    output = Path(sys.argv[5]).resolve()
    if not apk.is_file() or apk.stat().st_size <= 0:
        fail('apk_missing_or_empty')
    if not node_binary.is_file() or node_binary.stat().st_size <= 0:
        fail('node_binary_missing_or_empty')
    jcode_evidence = load_json(jcode_evidence_path, 'jcode_build_evidence')
    node_evidence = load_json(node_evidence_path, 'node_cross_build_evidence')

    with zipfile.ZipFile(apk) as zf:
        names = set(zf.namelist())
        for required in (JNI, HOST, JCODE, NODE, OMNI_MANIFEST):
            if required not in names:
                fail('required_apk_entry_missing:' + required)
        native = {}
        for entry in (JNI, HOST, JCODE, NODE):
            data = zf.read(entry)
            native[entry] = {'size': len(data), 'sha256': sha256_bytes(data)}
        omni_manifest_bytes = zf.read(OMNI_MANIFEST)
        try:
            omni_manifest = json.loads(omni_manifest_bytes.decode('utf-8'))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            fail('omniroute_manifest_invalid:' + type(exc).__name__)

    if jcode_evidence.get('mode') != 'jcode' or jcode_evidence.get('application_id') != 'com.vibecoder.shell':
        fail('jcode_build_evidence_identity_mismatch')
    jcode_claim = native_claim(jcode_evidence, JCODE)
    if jcode_claim.get('sha256') != native[JCODE]['sha256'] or jcode_claim.get('size') != native[JCODE]['size']:
        fail('jcode_build_evidence_payload_mismatch')

    node_claim = node_evidence.get('node') or {}
    target = node_evidence.get('target') or {}
    expected_node_hash = sha256_file(node_binary)
    if node_claim.get('version') != '24.19.0':
        fail('node_cross_build_version_mismatch')
    if node_claim.get('output_sha256') != expected_node_hash or node_claim.get('output_size') != node_binary.stat().st_size:
        fail('node_cross_build_evidence_payload_mismatch')
    if target.get('os') != 'android' or target.get('abi') != 'arm64-v8a' or target.get('libc') != 'bionic':
        fail('node_cross_build_target_mismatch')
    if native[NODE]['sha256'] != expected_node_hash or native[NODE]['size'] != node_binary.stat().st_size:
        fail('packaged_node_payload_mismatch')

    if omni_manifest.get('component_id') != 'omniroute' or omni_manifest.get('version') != '3.8.50':
        fail('omniroute_manifest_identity_mismatch')
    if omni_manifest.get('profile_id') != 'vibecoder-omniroute-android-backend-v1':
        fail('omniroute_manifest_profile_mismatch')

    evidence = {
        'schema': 1,
        'part': 34,
        'step': '34.10.16-sideload-alpha-with-packaged-node',
        'claim': 'sideload_alpha_apk_with_packaged_node_no_play_ownership_required_not_device_execution',
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
            'delivery': 'packaged_in_sideload_base_apk',
            'bundled_in_base_apk': True,
            'cross_build_evidence_sha256': sha256_file(node_evidence_path),
            'payload_bound_to_cross_build_evidence': True,
            'production_play_delivery_remains_on_demand': True,
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
    print(json.dumps({'sideload_alpha_build_evidence':'PASSED','apk_sha256':evidence['apk']['sha256']}, separators=(',', ':')))
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
