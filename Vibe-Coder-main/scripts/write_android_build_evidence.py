#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, subprocess, sys, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def fail(message: str) -> None:
    raise SystemExit(f"write_android_build_evidence: {message}")

def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def sha256_file(path: Path) -> str:
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024), b''):
            h.update(chunk)
    return h.hexdigest()

def command_line(args: list[str]) -> str:
    try:
        out=subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=20, check=True).stdout
    except Exception as exc:
        return f"unavailable:{type(exc).__name__}"
    return " ".join(out.split())[:1024]

def main() -> int:
    if len(sys.argv) != 4:
        fail("usage: write_android_build_evidence.py APK MODE OUTPUT_JSON")
    apk=Path(sys.argv[1]).resolve(); mode=sys.argv[2]; output=Path(sys.argv[3]).resolve()
    if mode not in {'minimal','jcode'}: fail('mode_must_be_minimal_or_jcode')
    if not apk.is_file() or apk.stat().st_size <= 0: fail(f'apk_missing_or_empty:{apk}')
    native=[]
    with zipfile.ZipFile(apk) as zf:
        for info in sorted(zf.infolist(), key=lambda i:i.filename):
            if not info.filename.startswith('lib/arm64-v8a/') or not info.filename.endswith('.so') or info.is_dir():
                continue
            data=zf.read(info)
            native.append({'entry':info.filename,'size':len(data),'sha256':sha256_bytes(data)})
    required={'lib/arm64-v8a/libvibecoder_shell_jni.so','lib/arm64-v8a/libvibecoder_android_host.so'}
    if mode == 'jcode': required.add('lib/arm64-v8a/libvibecoder_jcode_exec.so')
    present={item['entry'] for item in native}
    missing=sorted(required-present)
    if missing: fail(f'required_native_entries_missing:{missing}')
    checksum_manifest=ROOT/'CHECKSUMS.sha256'
    signing_config=json.loads((ROOT/'config/android-diagnostic-signing.json').read_text(encoding='utf-8'))
    signing_keystore=ROOT/signing_config['keystore']
    if sha256_file(signing_keystore) != signing_config['keystore_sha256']: fail('diagnostic_keystore_sha256_mismatch')
    evidence={
        'schema':1,
        'part':31,
        'mode':mode,
        'application_id':'com.vibecoder.shell',
        'signing':{'purpose':signing_config['purpose'],'certificate_sha256':signing_config['certificate_sha256'],'keystore_sha256':signing_config['keystore_sha256']},
        'apk':{'name':apk.name,'size':apk.stat().st_size,'sha256':sha256_file(apk)},
        'native_entries':native,
        'source':{
            'checksums_sha256':sha256_file(checksum_manifest),
            'runtime_inventory_sha256':sha256_file(ROOT/'config/android-runtime-inventory.json'),
            'payload_provisioning_sha256':sha256_file(ROOT/'config/android-payload-provisioning.json'),
            'diagnostic_signing_config_sha256':sha256_file(ROOT/'config/android-diagnostic-signing.json'),
        },
        'tool_evidence':{
            'java':command_line(['java','-version']),
            'gradle':command_line(['gradle','--version']),
            'cargo':command_line(['cargo','--version']),
            'rustc':command_line(['rustc','--version']),
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp=output.with_suffix(output.suffix+'.tmp')
    temp.write_text(json.dumps(evidence, sort_keys=True, separators=(',',':'))+'\n', encoding='utf-8')
    os.replace(temp, output)
    print(json.dumps({'build_evidence':'PASSED','mode':mode,'apk_sha256':evidence['apk']['sha256']}, separators=(',',':')))
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
