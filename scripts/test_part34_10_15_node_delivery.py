#!/usr/bin/env python3
import hashlib
import json
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def die(code):
    raise SystemExit('test_part34_10_15_node_delivery: ' + code)

def run(*args):
    return subprocess.run([sys.executable, *map(str, args)], cwd=ROOT, text=True, capture_output=True)

workflow = (ROOT / '.github/workflows/android-diagnostic-apk.yml').read_text()
node_workflow = (ROOT / '.github/workflows/node-runtime-proof.yml').read_text()
activity = (ROOT / 'android/app/src/main/java/com/vibecoder/shell/MainActivity.java').read_text()
delivery = (ROOT / 'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
setup_ui = (ROOT / 'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeSetupUi.java').read_text()
base_gradle = (ROOT / 'android/app/build.gradle.kts').read_text()
base_manifest = (ROOT / 'android/app/src/main/AndroidManifest.xml').read_text()
feature_manifest = (ROOT / 'android/node_runtime/src/main/AndroidManifest.xml').read_text()
packager = (ROOT / 'scripts/package_node_runtime_release.sh').read_text()
stager = (ROOT / 'scripts/stage_node_runtime_split.py').read_text()

if (ROOT / '.github/workflows/android-play-bundle.yml').exists():
    die('play_bundle_workflow_must_be_removed')
for path in (
    'scripts/part34_play_bundle_build_and_verify.sh',
    'scripts/stage_node_play_feature.py',
    'scripts/verify_node_feature_bundle.py',
    'scripts/write_play_bundle_evidence.py',
):
    if (ROOT / path).exists():
        die('play_specific_source_must_be_removed:' + path)
for token in ('com.google.android.play:feature-delivery', 'SplitCompat', 'SplitInstallManager', 'SplitInstallRequest'):
    if token in base_gradle + base_manifest + activity + delivery:
        die('play_runtime_reference_survived:' + token)

for token in (
    'PackageInstaller.SessionParams.MODE_INHERIT_EXISTING',
    'params.setAppPackageName(activity.getPackageName())',
    'Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES',
    'canRequestPackageInstalls()',
    'RUNTIME_URL',
    'vibecoder-node-runtime-24.19.0-v31',
    'vibecoder-node-runtime-arm64-v31.apk',
    'BaseDexClassLoader',
    'findLibrary(NODE_LIBRARY_NAME)',
    'getPackageArchiveInfo',
    'GET_SIGNING_CERTIFICATES',
    'PackageInstaller.STATUS_PENDING_USER_ACTION',
):
    if token not in delivery:
        die('direct_download_manager_missing:' + token)
if 'android.permission.REQUEST_INSTALL_PACKAGES' not in base_manifest:
    die('request_install_packages_permission_missing')
if 'Download & Set Up Node.js' not in setup_ui or 'GitHub release' not in setup_ui:
    die('node_setup_ui_missing')
if '<dist:on-demand' in feature_manifest:
    die('play_on_demand_manifest_survived')
if '<dist:install-time>' not in feature_manifest or '<dist:removable dist:value="true"' not in feature_manifest:
    die('downloadable_runtime_split_packaging_manifest_missing')

# Normal APK CI remains the previously proven Jcode + OmniRoute base lane. Node compilation is isolated.
if 'bash scripts/part34_node_execute_cross_build.sh' in workflow:
    die('normal_app_workflow_rebuilds_node')
for token in (
    'Node Android downloadable runtime',
    'Check fixed downloadable runtime release',
    "gh release view \"$NODE_RUNTIME_TAG\"",
    'steps.runtime_release.outputs.exists != \'true\'',
    'part34_node_execute_cross_build.sh',
    'package_node_runtime_release.sh',
    'gh release create',
    'gh release upload',
    'Verify published Node runtime download URL',
    'https://github.com/${GITHUB_REPOSITORY}/releases/download/${NODE_RUNTIME_TAG}/${NODE_RUNTIME_ASSET}',
    'curl --fail --location --retry 8',
    'Published Node runtime public URL verification PASSED',
    '--json isDraft,isPrerelease,assets',
    'gh release edit "$NODE_RUNTIME_TAG"',
    '--draft=false --prerelease=false',
    'Node runtime public release already usable: $exists',
    '--head',
):
    if token not in node_workflow:
        die('downloadable_node_workflow_missing:' + token)
for token in (
    ':node_runtime:assembleDebug',
    'verify_node_runtime_split_apk.py',
    'apksigner',
    'aapt2',
    "split='node_runtime'|featureSplit='node_runtime'",
):
    if token not in packager:
        die('runtime_packager_missing:' + token)
for token in ('github_release_packageinstaller_split', 'release_tag', 'runtime_apk'):
    if token not in stager:
        die('runtime_stager_identity_missing:' + token)

verify = ROOT / 'scripts/verify_node_runtime_split_apk.py'
with tempfile.TemporaryDirectory(prefix='vibecoder-node-runtime-split-') as td:
    d = Path(td)
    node = d / 'node.so'
    node.write_bytes(b'node-24.19.0-fixture')
    node_hash = hashlib.sha256(node.read_bytes()).hexdigest()
    runtime_manifest = json.dumps({
        'schema': 1,
        'component_id': 'node',
        'version': '24.19.0',
        'abi': 'arm64-v8a',
        'libc': 'bionic',
        'file_name': 'libvibecoder_node_exec.so',
        'size': node.stat().st_size,
        'sha256': node_hash,
        'delivery': 'github_release_packageinstaller_split',
        'split': 'node_runtime',
        'release_tag': 'vibecoder-node-runtime-24.19.0-v31',
        'runtime_apk': 'vibecoder-node-runtime-arm64-v31.apk',
    }, sort_keys=True, separators=(',', ':')).encode()
    apk = d / 'runtime.apk'
    with zipfile.ZipFile(apk, 'w') as z:
        z.writestr('lib/arm64-v8a/libvibecoder_node_exec.so', node.read_bytes())
        z.writestr('assets/node-runtime/manifest.json', runtime_manifest)
    result = run(verify, apk, node)
    if result.returncode:
        die('valid_runtime_split_fixture_rejected:' + result.stderr.strip())
    bad = d / 'bad.apk'
    with zipfile.ZipFile(bad, 'w') as z:
        z.writestr('lib/arm64-v8a/libvibecoder_node_exec.so', b'wrong-node')
        z.writestr('assets/node-runtime/manifest.json', runtime_manifest)
    result = run(verify, bad, node)
    if result.returncode == 0 or 'node_payload_hash_mismatch' not in result.stderr:
        die('corrupt_runtime_split_not_rejected')

print('Part 34.10.15 direct Node setup regression PASSED')
