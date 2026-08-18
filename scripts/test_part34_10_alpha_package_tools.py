#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, subprocess, sys, tempfile, zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WRITER=ROOT/'scripts/write_alpha_build_evidence.py'
APP_GRADLE=ROOT/'android/app/build.gradle.kts'
AAPT_HIDDEN_SAFE_PATTERN='!.svn:!.git:!.ds_store:!*.scc:<dir>_*:!CVS:!thumbs.db:!picasa.ini:!*~'

# AAPT defaults include `.*`, which drops the signed OmniRoute manifest and hidden Next
# runtime directories from generated assets. The app must override only that broad rule.
gradle_text=APP_GRADLE.read_text(encoding='utf-8')
if 'androidResources.ignoreAssetsPattern =' not in gradle_text or f'"{AAPT_HIDDEN_SAFE_PATTERN}"' not in gradle_text:
    raise SystemExit('alpha_aapt_hidden_omniroute_asset_policy_missing')
if '!.scc:.*:' in gradle_text or ':.*:' in gradle_text:
    raise SystemExit('alpha_aapt_hidden_omniroute_asset_policy_regressed')

# Mirror AAPT's documented simplified ignore matcher for the names that matter here. `!` only
# silences the ignore warning; it does not negate the match.
def aapt_name_ignored(name: str, is_dir: bool, pattern: str) -> bool:
    lower=name.lower()
    for raw in pattern.split(':'):
        token=raw
        if not token:
            continue
        if token.startswith('!'):
            token=token[1:]
        if token.startswith('<dir>'):
            if not is_dir:
                continue
            token=token[5:]
        elif token.startswith('<file>'):
            if is_dir:
                continue
            token=token[6:]
        token=token.lower()
        if token.startswith('*') and len(token)>1:
            match=lower.endswith(token[1:])
        elif token.endswith('*') and len(token)>1:
            match=lower.startswith(token[:-1])
        else:
            match=lower==token
        if match:
            return True
    return name in ('.','..')

AAPT_DEFAULT='!.svn:!.git:!.ds_store:!*.scc:.*:<dir>_*:!CVS:!thumbs.db:!picasa.ini:!*~'
assert aapt_name_ignored('.vibecoder-omniroute-bundle.json', False, AAPT_DEFAULT)
assert aapt_name_ignored('.next', True, AAPT_DEFAULT)
assert not aapt_name_ignored('.vibecoder-omniroute-bundle.json', False, AAPT_HIDDEN_SAFE_PATTERN)
assert not aapt_name_ignored('.next', True, AAPT_HIDDEN_SAFE_PATTERN)
assert aapt_name_ignored('.git', True, AAPT_HIDDEN_SAFE_PATTERN)
assert aapt_name_ignored('scratch~', False, AAPT_HIDDEN_SAFE_PATTERN)

def sha(data: bytes) -> str: return hashlib.sha256(data).hexdigest()
def run(args): return subprocess.run(args,cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)

with tempfile.TemporaryDirectory(prefix='vibecoder-alpha-evidence-test-') as td:
    tmp=Path(td)
    payloads={
        'lib/arm64-v8a/libvibecoder_shell_jni.so':b'jni-fixture',
        'lib/arm64-v8a/libvibecoder_android_host.so':b'host-fixture',
        'lib/arm64-v8a/libvibecoder_jcode_exec.so':b'jcode-fixture',
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
    jp=tmp/'jcode.json'; out=tmp/'out.json'
    jp.write_text(json.dumps(jcode))
    ok=run([sys.executable,str(WRITER),str(apk),str(jp),str(out)])
    if ok.returncode != 0: raise SystemExit(f'alpha_evidence_success_fixture_failed:{ok.stdout}')
    evidence=json.loads(out.read_text())
    assert evidence['jcode']['payload_bound_to_proof_evidence'] is True
    assert evidence['node']['delivery'] == 'play_feature_on_demand'
    assert evidence['node']['bundled_in_base_apk'] is False
    assert evidence['jcode']['device_execution_proven'] is False
    assert evidence['node']['device_execution_proven'] is False
    assert evidence['omniroute']['device_service_round_trip_proven'] is False

    jcode['native_entries'][0]['sha256']='f'*64
    bad=tmp/'jcode-bad.json'; bad.write_text(json.dumps(jcode))
    rejected=run([sys.executable,str(WRITER),str(apk),str(bad),str(tmp/'bad.json')])
    if rejected.returncode == 0 or 'jcode_build_evidence_payload_mismatch' not in rejected.stdout:
        raise SystemExit(f'alpha_evidence_tampered_jcode_not_rejected:{rejected.stdout}')

print('Part 34.10.3 Alpha package evidence regression PASSED')
