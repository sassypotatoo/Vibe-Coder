#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, re, subprocess, sys, tempfile, zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WRITER=ROOT/'scripts/write_alpha_build_evidence.py'
APP_GRADLE=ROOT/'android/app/build.gradle.kts'
AAPT_TRANSPARENT_PATTERN='__vibecoder_aapt_ignore_none__'

gradle_text=APP_GRADLE.read_text(encoding='utf-8')
assignments=re.findall(r'androidResources\.ignoreAssetsPattern\s*=\s*"([^"]*)"',gradle_text)
if assignments != [AAPT_TRANSPARENT_PATTERN]:
    raise SystemExit(f'alpha_aapt_transparent_omniroute_asset_policy_missing:{assignments}')
alpha_lane=(ROOT/'scripts/part34_alpha_build_and_verify.sh').read_text(encoding='utf-8')
for token in (
    'run_stage omniroute-aapt-policy 60 python3 "$ROOT/scripts/verify_omniroute_aapt_asset_policy.py"',
    'libvibecoder_node_exec.so',
    'verify_node_cross_build_evidence.py',
    'verify_android_diagnostic_apk.sh" "$APK" sideload_alpha',
):
    if token not in alpha_lane:
        raise SystemExit('development_alpha_contract_missing:' + token)

def sha(data: bytes) -> str: return hashlib.sha256(data).hexdigest()
def run(args): return subprocess.run(args,cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)

with tempfile.TemporaryDirectory(prefix='vibecoder-alpha-evidence-test-') as td:
    tmp=Path(td)
    payloads={
        'lib/arm64-v8a/libvibecoder_shell_jni.so':b'jni-fixture',
        'lib/arm64-v8a/libvibecoder_android_host.so':b'host-fixture',
        'lib/arm64-v8a/libvibecoder_jcode_exec.so':b'jcode-fixture',
        'lib/arm64-v8a/libvibecoder_node_exec.so':b'node-fixture',
    }
    omni={'component_id':'omniroute','version':'3.8.50','profile_id':'vibecoder-omniroute-android-backend-v1',
          'tree_sha256':'0'*64,'file_count':1,'total_bytes':1}
    apk=tmp/'fixture.apk'
    with zipfile.ZipFile(apk,'w') as zf:
        for name,data in payloads.items(): zf.writestr(name,data)
        zf.writestr('assets/omniroute/bundle/.vibecoder-omniroute-bundle.json',json.dumps(omni,separators=(',',':')))
    source_sha=hashlib.sha256((ROOT/'CHECKSUMS.sha256').read_bytes()).hexdigest()
    jcode={
        'mode':'jcode','application_id':'com.vibecoder.shell',
        'native_entries':[{'entry':'lib/arm64-v8a/libvibecoder_jcode_exec.so','size':len(payloads['lib/arm64-v8a/libvibecoder_jcode_exec.so']),'sha256':sha(payloads['lib/arm64-v8a/libvibecoder_jcode_exec.so'])}],
        'source':{'checksums_sha256':source_sha},
    }
    node_binary=tmp/'libvibecoder_node_exec.so'; node_binary.write_bytes(payloads['lib/arm64-v8a/libvibecoder_node_exec.so'])
    node_evidence={
        'node':{'version':'24.19.0','output_sha256':sha(node_binary.read_bytes()),'output_size':node_binary.stat().st_size},
        'target':{'os':'android','abi':'arm64-v8a','libc':'bionic'},
    }
    jp=tmp/'jcode.json'; np=tmp/'node.json'; out=tmp/'out.json'
    jp.write_text(json.dumps(jcode)); np.write_text(json.dumps(node_evidence))
    ok=run([sys.executable,str(WRITER),str(apk),str(jp),str(node_binary),str(np),str(out)])
    if ok.returncode != 0: raise SystemExit(f'alpha_evidence_success_fixture_failed:{ok.stdout}')
    evidence=json.loads(out.read_text())
    assert evidence['jcode']['payload_bound_to_proof_evidence'] is True
    assert evidence['node']['delivery'] == 'packaged_in_development_base_apk'
    assert evidence['node']['bundled_in_base_apk'] is True
    assert evidence['node']['google_play_required_for_development_apk'] is False
    assert evidence['node']['play_delivery_deferred_until_publishing'] is True
    assert evidence['jcode']['device_execution_proven'] is False
    assert evidence['node']['device_execution_proven'] is False
    assert evidence['omniroute']['device_service_round_trip_proven'] is False

    jcode['native_entries'][0]['sha256']='f'*64
    bad=tmp/'jcode-bad.json'; bad.write_text(json.dumps(jcode))
    rejected=run([sys.executable,str(WRITER),str(apk),str(bad),str(node_binary),str(np),str(tmp/'bad.json')])
    if rejected.returncode == 0 or 'jcode_build_evidence_payload_mismatch' not in rejected.stdout:
        raise SystemExit(f'alpha_evidence_tampered_jcode_not_rejected:{rejected.stdout}')

    node_evidence['node']['output_sha256']='e'*64
    bad_node=tmp/'node-bad.json'; bad_node.write_text(json.dumps(node_evidence))
    rejected=run([sys.executable,str(WRITER),str(apk),str(jp),str(node_binary),str(bad_node),str(tmp/'bad-node.json')])
    if rejected.returncode == 0 or 'node_cross_build_evidence_payload_mismatch' not in rejected.stdout:
        raise SystemExit(f'alpha_evidence_tampered_node_not_rejected:{rejected.stdout}')

print('Part 34.10.3 Alpha package evidence regression PASSED')
