#!/usr/bin/env python3
import hashlib
import json
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def die(code): raise SystemExit('test_part34_10_15_node_delivery: '+code)

def run(*args):
    return subprocess.run([sys.executable, *map(str,args)], cwd=ROOT, text=True, capture_output=True)

workflow=(ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
node_workflow=(ROOT/'.github/workflows/node-runtime-proof.yml').read_text()
play_workflow=(ROOT/'.github/workflows/android-play-bundle.yml').read_text()
activity=(ROOT/'android/app/src/main/java/com/vibecoder/shell/MainActivity.java').read_text()
delivery=(ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
host=(ROOT/'crates/vibecoder-android-host/src/lib.rs').read_text()
play_script=(ROOT/'scripts/part34_play_bundle_build_and_verify.sh').read_text()

if 'node-android-proof-build:' in workflow: die('normal_ci_rebuilds_node')
for token in ('workflow_dispatch:', 'node-android-proof-build:', 'timeout-minutes: 360', 'retention-days: 90',
              'Fail-fast source and Node build-contract validation', 'python3 scripts/validate_checkpoint.py',
              'python3 scripts/test_part34_10_compile_repairs.py'):
    if token not in node_workflow: die('dedicated_node_workflow_missing:'+token)
for token in ('node_runtime_run_id:', 'actions/download-artifact@v8.0.1', 'run-id: ${{ inputs.node_runtime_run_id }}', 'dtolnay/rust-toolchain@1.91.0', 'part34_play_bundle_build_and_verify.sh'):
    if token not in play_workflow: die('play_bundle_workflow_missing:'+token)
for token in ('nodeRoot.getCanonicalPath()', 'SplitCompat.installActivity(this)', '@SuppressWarnings("deprecation")'):
    if token not in activity: die('android_node_delivery_runtime_missing:'+token)
for token in ('BaseDexClassLoader', 'findLibrary(NODE_LIBRARY_NAME)', 'getSessionStates()', 'emitTerminal(', '@SuppressWarnings("deprecation")'):
    if token not in delivery: die('node_delivery_manager_hardening_missing:'+token)
for token in ('RuntimePlacement::PlayFeatureNativeExecutable', 'RuntimePlacement::ApkNativeExecutable => self.paths.native_library_dir()', 'RuntimePlacement::PlayFeatureNativeExecutable => self.paths.packaged_executable_dir()'):
    if token not in host: die('separate_execution_root_contract_missing:'+token)
for token in ('FEATURE_NODE=', 'BASE_NODE=', 'rm -f "$BASE_NODE"', 'verify_node_feature_bundle.py" "$AAB" "$FEATURE_NODE"'):
    if token not in play_script: die('play_bundle_stale_base_node_guard_missing:'+token)

verify=ROOT/'scripts/verify_node_feature_bundle.py'
writer=ROOT/'scripts/write_play_bundle_evidence.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-feature-bundle-') as td:
    d=Path(td); node=d/'node.so'; node.write_bytes(b'node-24.19.0-fixture')
    aab=d/'app.aab'
    node_hash=hashlib.sha256(node.read_bytes()).hexdigest()
    runtime_manifest=json.dumps({
        'schema':1,
        'component_id':'node',
        'version':'24.19.0',
        'abi':'arm64-v8a',
        'libc':'bionic',
        'file_name':'libvibecoder_node_exec.so',
        'size':node.stat().st_size,
        'sha256':node_hash,
        'delivery':'play_feature_on_demand',
        'module':'node_runtime',
    },sort_keys=True,separators=(',',':')).encode()
    entries={
        'base/lib/arm64-v8a/libvibecoder_android_host.so': b'host',
        'base/lib/arm64-v8a/libvibecoder_jcode_exec.so': b'jcode',
        'base/assets/omniroute/bundle/.vibecoder-omniroute-bundle.json': b'{}',
        'node_runtime/manifest/AndroidManifest.xml': b'manifest',
        'node_runtime/assets/node-runtime/manifest.json': runtime_manifest,
        'node_runtime/lib/arm64-v8a/libvibecoder_node_exec.so': node.read_bytes(),
    }
    with zipfile.ZipFile(aab,'w') as z:
        for k,v in entries.items(): z.writestr(k,v)
    result=run(verify,aab,node)
    if result.returncode: die('valid_bundle_fixture_rejected:'+result.stderr.strip())
    cross=d/'cross.json'; cross.write_text(json.dumps({
        'node': {'version':'24.19.0','output_sha256':node_hash,'output_size':node.stat().st_size},
        'target': {'os':'android','abi':'arm64-v8a','libc':'bionic'}
    }))
    out=d/'evidence.json'; result=run(writer,aab,node,cross,out)
    if result.returncode: die('bundle_evidence_fixture_rejected:'+result.stderr.strip())
    evidence=json.loads(out.read_text())
    if evidence.get('node_delivery')!='play_feature_on_demand' or evidence.get('node_bundled_in_base') is not False:
        die('bundle_evidence_delivery_identity_invalid')
    bad_metadata=d/'bad-metadata.aab'
    with zipfile.ZipFile(bad_metadata,'w') as z:
        for k,v in entries.items():
            if k == 'node_runtime/assets/node-runtime/manifest.json':
                broken=json.loads(v.decode()); broken['sha256']='0'*64
                v=json.dumps(broken,sort_keys=True,separators=(',',':')).encode()
            z.writestr(k,v)
    result=run(verify,bad_metadata,node)
    if result.returncode == 0 or 'node_feature_metadata_mismatch:sha256' not in result.stderr:
        die('corrupt_node_feature_metadata_not_rejected')

    bad=d/'bad.aab'
    with zipfile.ZipFile(bad,'w') as z:
        for k,v in entries.items(): z.writestr(k,v)
        z.writestr('base/lib/arm64-v8a/libvibecoder_node_exec.so', node.read_bytes())
    result=run(verify,bad,node)
    if result.returncode == 0 or 'node_leaked_into_base_module' not in result.stderr:
        die('base_node_leak_not_rejected')

print('Part 34.10.15 Node delivery regression PASSED')
