#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, subprocess, sys, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
NODE_ENTRY = 'lib/arm64-v8a/libvibecoder_node_exec.so'


def fail(message: str) -> None:
    raise SystemExit(f"write_node_build_evidence: {message}")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def command_line(args: list[str]) -> str:
    try:
        out = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, timeout=20, check=True).stdout
    except Exception as exc:
        return f"unavailable:{type(exc).__name__}"
    return " ".join(out.split())[:1024]


def main() -> int:
    if len(sys.argv) != 4:
        fail("usage: write_node_build_evidence.py APK CROSS_BUILD_EVIDENCE OUTPUT_JSON")
    apk = Path(sys.argv[1]).resolve()
    cross_build_evidence_path = Path(sys.argv[2]).resolve()
    output = Path(sys.argv[3]).resolve()
    if not apk.is_file() or apk.stat().st_size <= 0:
        fail(f"apk_missing_or_empty:{apk}")

    native = []
    node = None
    with zipfile.ZipFile(apk) as zf:
        for info in sorted(zf.infolist(), key=lambda item: item.filename):
            if not info.filename.startswith('lib/arm64-v8a/') or not info.filename.endswith('.so') or info.is_dir():
                continue
            data = zf.read(info)
            item = {'entry': info.filename, 'size': len(data), 'sha256': sha256_bytes(data)}
            native.append(item)
            if info.filename == NODE_ENTRY:
                node = item

    required = {
        'lib/arm64-v8a/libvibecoder_shell_jni.so',
        'lib/arm64-v8a/libvibecoder_android_host.so',
        NODE_ENTRY,
    }
    present = {item['entry'] for item in native}
    missing = sorted(required - present)
    if missing:
        fail(f"required_native_entries_missing:{missing}")
    if node is None:
        fail("node_native_entry_missing")

    if not cross_build_evidence_path.is_file() or cross_build_evidence_path.stat().st_size <= 0:
        fail('cross_build_evidence_missing_or_empty')
    try:
        cross_build_evidence = json.loads(cross_build_evidence_path.read_text(encoding='utf-8'))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f'cross_build_evidence_invalid_json:{type(exc).__name__}')
    if cross_build_evidence.get('step') != '34.2.3' or cross_build_evidence.get('claim') != 'cross_build_candidate_only_not_device_execution':
        fail('cross_build_evidence_identity_mismatch')
    cross_node = cross_build_evidence.get('node') or {}
    if cross_node.get('output_sha256') != node['sha256']:
        fail('cross_build_node_sha256_mismatch')
    if cross_node.get('device_execution_proven') is not False:
        fail('cross_build_device_execution_claim_must_remain_false')
    if cross_node.get('version') != '24.19.0':
        fail('cross_build_node_version_mismatch')
    if cross_node.get('source_archive_sha256') != 'f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f':
        fail('cross_build_source_sha256_mismatch')
    cross_target = cross_build_evidence.get('target') or {}
    if cross_target.get('os') != 'android' or cross_target.get('abi') != 'arm64-v8a' or cross_target.get('libc') != 'bionic':
        fail('cross_build_target_identity_mismatch')
    if cross_target.get('api') != 29:
        fail('cross_build_android_api_mismatch')
    cross_toolchain = cross_build_evidence.get('toolchain') or {}
    if cross_toolchain.get('android_ndk_revision') != '28.2.13676358':
        fail('cross_build_android_ndk_revision_mismatch')
    cross_elf = cross_node.get('elf') or {}
    for key in ('elf64', 'aarch64', 'et_dyn', 'page16', 'android_linker_compatible'):
        if cross_elf.get(key) is not True:
            fail(f'cross_build_elf_evidence_missing:{key}')

    signing_config = json.loads((ROOT / 'config/android-diagnostic-signing.json').read_text(encoding='utf-8'))
    signing_keystore = ROOT / signing_config['keystore']
    if sha256_file(signing_keystore) != signing_config['keystore_sha256']:
        fail('diagnostic_keystore_sha256_mismatch')

    evidence = {
        'schema': 1,
        'part': 34,
        'step': '34.2.3',
        'mode': 'node',
        'application_id': 'com.vibecoder.shell',
        'claim': 'apk_package_evidence_only_not_device_execution',
        'node': {
            'version_requirement': '24.19.0',
            'entry': node['entry'],
            'size': node['size'],
            'sha256': node['sha256'],
            'device_execution_proven': False,
            'cross_build_evidence_sha256': sha256_file(cross_build_evidence_path),
            'cross_build_ndk_revision': (cross_build_evidence.get('toolchain') or {}).get('android_ndk_revision'),
            'cross_build_api': (cross_build_evidence.get('target') or {}).get('api'),
        },
        'signing': {
            'purpose': signing_config['purpose'],
            'certificate_sha256': signing_config['certificate_sha256'],
            'keystore_sha256': signing_config['keystore_sha256'],
        },
        'apk': {'name': apk.name, 'size': apk.stat().st_size, 'sha256': sha256_file(apk)},
        'native_entries': native,
        'source': {
            'checksums_sha256': sha256_file(ROOT / 'CHECKSUMS.sha256'),
            'runtime_inventory_sha256': sha256_file(ROOT / 'config/android-runtime-inventory.json'),
            'payload_provisioning_sha256': sha256_file(ROOT / 'config/android-payload-provisioning.json'),
            'node_provisioner_sha256': sha256_file(ROOT / 'scripts/provision_node_android.sh'),
            'android_elf_verifier_sha256': sha256_file(ROOT / 'scripts/verify_android_elf.py'),
        },
        'tool_evidence': {
            'java': command_line(['java', '-version']),
            'gradle': command_line(['gradle', '--version']),
            'cargo': command_line(['cargo', '--version']),
            'rustc': command_line(['rustc', '--version']),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(output.suffix + '.tmp')
    temp.write_text(json.dumps(evidence, sort_keys=True, separators=(',', ':')) + '\n', encoding='utf-8')
    os.replace(temp, output)
    print(json.dumps({'node_build_evidence': 'PASSED', 'mode': 'node',
                      'apk_sha256': evidence['apk']['sha256'],
                      'node_sha256': node['sha256']}, separators=(',', ':')))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
