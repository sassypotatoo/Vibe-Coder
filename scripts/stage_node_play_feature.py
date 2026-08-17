#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, shutil, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
DEST=ROOT/'android/node_runtime/build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so'
ASSET=ROOT/'android/node_runtime/build/generated/assets/node-runtime/manifest.json'

def fail(msg): raise SystemExit('stage_node_play_feature: '+msg)
def sha(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''): h.update(chunk)
    return h.hexdigest()
if len(sys.argv)!=3: fail('usage: stage_node_play_feature.py NODE_BINARY NODE_EVIDENCE')
node=Path(sys.argv[1]).resolve(); evidence_path=Path(sys.argv[2]).resolve()
if not node.is_file() or node.stat().st_size<=0: fail('node_binary_missing')
if not evidence_path.is_file(): fail('node_evidence_missing')
evidence=json.loads(evidence_path.read_text())
claim=evidence.get('node') or {}; target=evidence.get('target') or {}
actual=sha(node)
if claim.get('version')!='24.19.0': fail('node_version_mismatch')
if claim.get('output_sha256')!=actual or claim.get('output_size')!=node.stat().st_size: fail('node_evidence_payload_mismatch')
if target.get('os')!='android' or target.get('abi')!='arm64-v8a' or target.get('libc')!='bionic': fail('node_target_mismatch')
subprocess_check=[sys.executable,str(ROOT/'scripts/verify_android_elf.py'),str(node)]
import subprocess
subprocess.run(subprocess_check,cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
DEST.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(node,DEST); os.chmod(DEST,0o644)
ASSET.parent.mkdir(parents=True,exist_ok=True)
manifest={'schema':1,'component_id':'node','version':'24.19.0','abi':'arm64-v8a','libc':'bionic','file_name':DEST.name,'size':DEST.stat().st_size,'sha256':sha(DEST),'delivery':'play_feature_on_demand','module':'node_runtime'}
ASSET.write_text(json.dumps(manifest,sort_keys=True,separators=(',',':'))+'\n')
print(json.dumps({'node_play_feature_staged':'PASSED','sha256':manifest['sha256'],'size':manifest['size']},separators=(',',':')))
