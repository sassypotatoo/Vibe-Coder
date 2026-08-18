#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, os, sys, zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
JNI='lib/arm64-v8a/libvibecoder_shell_jni.so'
HOST='lib/arm64-v8a/libvibecoder_android_host.so'
NODE='lib/arm64-v8a/libvibecoder_node_exec.so'
JCODE='lib/arm64-v8a/libvibecoder_jcode_exec.so'
OMNI='assets/omniroute/bundle/.vibecoder-omniroute-bundle.json'
JCODE_DESCRIPTOR='assets/runtime/jcode-runtime-download.json'

def fail(m): raise SystemExit('write_alpha_build_evidence: '+m)
def sha_bytes(b): return hashlib.sha256(b).hexdigest()
def sha_file(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for c in iter(lambda:f.read(1024*1024),b''): h.update(c)
    return h.hexdigest()
def load_json(p,label):
    if not p.is_file() or p.stat().st_size<=0: fail(label+'_missing_or_empty')
    try: v=json.loads(p.read_text())
    except Exception as e: fail(label+'_invalid:'+type(e).__name__)
    if not isinstance(v,dict): fail(label+'_not_object')
    return v
if len(sys.argv)!=4: fail('usage: APK NODE_CROSS_EVIDENCE OUTPUT_JSON')
apk=Path(sys.argv[1]); node_ev_path=Path(sys.argv[2]); out=Path(sys.argv[3])
node_ev=load_json(node_ev_path,'node_cross_build_evidence')
with zipfile.ZipFile(apk) as z:
    names=set(z.namelist())
    for req in (JNI,HOST,NODE,OMNI,JCODE_DESCRIPTOR):
        if req not in names: fail('required_apk_entry_missing:'+req)
    if JCODE in names: fail('jcode_must_not_be_bundled_in_development_base_apk')
    native={e:{'size':len(z.read(e)),'sha256':sha_bytes(z.read(e))} for e in (JNI,HOST,NODE)}
    omni_bytes=z.read(OMNI); desc_bytes=z.read(JCODE_DESCRIPTOR)
    omni=json.loads(omni_bytes.decode()); desc=json.loads(desc_bytes.decode())
node_claim=node_ev.get('node') or {}; target=node_ev.get('target') or {}
if node_ev.get('step')!='34.2.3' or node_claim.get('version')!='24.19.0': fail('node_cross_build_evidence_identity_mismatch')
if node_claim.get('output_sha256')!=native[NODE]['sha256']: fail('node_cross_build_payload_mismatch')
if target.get('os')!='android' or target.get('abi')!='arm64-v8a' or target.get('libc')!='bionic': fail('node_target_mismatch')
if omni.get('component_id')!='omniroute' or omni.get('version')!='3.8.50': fail('omniroute_manifest_identity_mismatch')
if desc.get('component_id')!='jcode' or desc.get('version')!='0.73.0' or desc.get('split_name')!='jcode_runtime': fail('jcode_descriptor_identity_mismatch')
if desc.get('base_version_code')!=33: fail('jcode_descriptor_version_code_mismatch')
if not str(desc.get('download_url','')).startswith('https://github.com/sassypotatoo/Vibe-Coder/releases/download/'): fail('jcode_descriptor_url_untrusted')
evidence={
 'schema':1,'part':34,'step':'34.10.18-development-jcode-download','claim':'development_base_apk_with_node_and_downloadable_signed_jcode_split_not_device_execution','application_id':'com.vibecoder.shell',
 'apk':{'name':apk.name,'size':apk.stat().st_size,'sha256':sha_file(apk)},'native_entries':native,
 'node':{'version_requirement':'24.19.0','delivery':'development_base_apk','bundled_in_base_apk':True,'cross_build_evidence_sha256':sha_file(node_ev_path),'device_execution_proven':False},
 'jcode':{'version_requirement':'0.73.0','delivery':'signed_package_split_download_during_setup','module':'jcode_runtime','bundled_in_base_apk':False,'descriptor_sha256':sha_bytes(desc_bytes),'release_tag':desc.get('release_tag'),'device_execution_proven':False},
 'omniroute':{'version':'3.8.50','manifest_sha256':sha_bytes(omni_bytes),'tree_sha256':omni.get('tree_sha256'),'file_count':omni.get('file_count'),'total_bytes':omni.get('total_bytes'),'device_service_round_trip_proven':False},
 'source':{'checksums_sha256':sha_file(ROOT/'CHECKSUMS.sha256'),'runtime_inventory_sha256':sha_file(ROOT/'config/android-runtime-inventory.json'),'payload_provisioning_sha256':sha_file(ROOT/'config/android-payload-provisioning.json')}
}
out.parent.mkdir(parents=True,exist_ok=True); tmp=out.with_suffix(out.suffix+'.tmp'); tmp.write_text(json.dumps(evidence,sort_keys=True,separators=(',',':'))+'\n'); os.replace(tmp,out)
print(json.dumps({'alpha_build_evidence':'PASSED','apk_sha256':evidence['apk']['sha256'],'jcode_bundled':False,'node_bundled':True},separators=(',',':')))
