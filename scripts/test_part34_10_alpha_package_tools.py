#!/usr/bin/env python3
from __future__ import annotations
import hashlib,json,re,subprocess,sys,tempfile,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WRITER=ROOT/'scripts/write_alpha_build_evidence.py'
APP_GRADLE=ROOT/'android/app/build.gradle.kts'
AAPT='__vibecoder_aapt_ignore_none__'
assignments=re.findall(r'androidResources\.ignoreAssetsPattern\s*=\s*"([^"]*)"',APP_GRADLE.read_text())
if assignments!=[AAPT]: raise SystemExit('alpha_aapt_policy_missing')
alpha=(ROOT/'scripts/part34_alpha_build_and_verify.sh').read_text()
for token in ('omniroute-aapt-policy','libvibecoder_node_exec.so','verify_node_cross_build_evidence.py','development_alpha_download_jcode','jcode_must_not_be_bundled_in_development_base_apk'):
    if token not in alpha: raise SystemExit('development_alpha_contract_missing:'+token)

def sha(b): return hashlib.sha256(b).hexdigest()
def run(args): return subprocess.run(args,cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
with tempfile.TemporaryDirectory(prefix='vibecoder-alpha-download-jcode-') as td:
    t=Path(td)
    payloads={
      'lib/arm64-v8a/libvibecoder_shell_jni.so':b'jni',
      'lib/arm64-v8a/libvibecoder_android_host.so':b'host',
      'lib/arm64-v8a/libvibecoder_node_exec.so':b'node',
    }
    omni={'component_id':'omniroute','version':'3.8.50','tree_sha256':'0'*64,'file_count':1,'total_bytes':1}
    desc={'schema':1,'component_id':'jcode','version':'0.73.0','application_id':'com.vibecoder.shell','base_version_code':33,'split_name':'jcode_runtime','abi':'arm64-v8a','release_tag':'vibecoder-jcode-runtime-0.73.0-dev-v33','download_url':'https://github.com/sassypotatoo/Vibe-Coder/releases/download/vibecoder-jcode-runtime-0.73.0-dev-v33/vibecoder-jcode-runtime-arm64-v8a.apk'}
    apk=t/'fixture.apk'
    with zipfile.ZipFile(apk,'w') as z:
        for n,b in payloads.items(): z.writestr(n,b)
        z.writestr('assets/omniroute/bundle/.vibecoder-omniroute-bundle.json',json.dumps(omni))
        z.writestr('assets/runtime/jcode-runtime-download.json',json.dumps(desc))
    node_ev={'step':'34.2.3','node':{'version':'24.19.0','output_sha256':sha(payloads['lib/arm64-v8a/libvibecoder_node_exec.so'])},'target':{'os':'android','abi':'arm64-v8a','libc':'bionic'}}
    ep=t/'node.json'; ep.write_text(json.dumps(node_ev)); out=t/'out.json'
    ok=run([sys.executable,str(WRITER),str(apk),str(ep),str(out)])
    if ok.returncode: raise SystemExit('alpha_evidence_fixture_failed:'+ok.stdout)
    ev=json.loads(out.read_text())
    assert ev['node']['bundled_in_base_apk'] is True
    assert ev['node']['delivery']=='development_base_apk'
    assert ev['jcode']['bundled_in_base_apk'] is False
    assert ev['jcode']['delivery']=='signed_package_split_download_during_setup'
    assert ev['jcode']['module']=='jcode_runtime'
    # Reintroducing Jcode into base must be rejected.
    bad=t/'bad.apk'
    with zipfile.ZipFile(apk) as src, zipfile.ZipFile(bad,'w') as dst:
        for info in src.infolist(): dst.writestr(info,src.read(info.filename))
        dst.writestr('lib/arm64-v8a/libvibecoder_jcode_exec.so',b'jcode')
    rej=run([sys.executable,str(WRITER),str(bad),str(ep),str(t/'bad.json')])
    if rej.returncode==0 or 'jcode_must_not_be_bundled_in_development_base_apk' not in rej.stdout:
        raise SystemExit('bundled_jcode_not_rejected:'+rej.stdout)
    node_ev['node']['output_sha256']='f'*64; ep.write_text(json.dumps(node_ev))
    rej=run([sys.executable,str(WRITER),str(apk),str(ep),str(t/'badnode.json')])
    if rej.returncode==0 or 'node_cross_build_payload_mismatch' not in rej.stdout:
        raise SystemExit('tampered_node_not_rejected:'+rej.stdout)
print('Part 34.10.3 Alpha package evidence regression PASSED')
