#!/usr/bin/env python3
from pathlib import Path
import json
ROOT=Path(__file__).resolve().parents[1]

def die(code): raise SystemExit('test_part34_10_18_jcode_runtime_download: '+code)
def need(text,token,label):
    if token not in text: die(label+':'+token)

inventory=json.loads((ROOT/'config/android-runtime-inventory.json').read_text())
components={c['component_id']:c for c in inventory['components']}
jcode=components['jcode']; node=components['node']
if jcode.get('placement')!='package_split_native_executable' or jcode.get('delivery_module')!='jcode_runtime' or jcode.get('bundled_in_base') is not False:
    die('jcode_inventory_not_package_split')
if node.get('placement')!='apk_native_executable' or node.get('bundled_in_base') is not True:
    die('node_inventory_not_development_base')

app_gradle=(ROOT/'android/app/build.gradle.kts').read_text()
settings=(ROOT/'android/settings.gradle.kts').read_text()
manifest=(ROOT/'android/app/src/main/AndroidManifest.xml').read_text()
activity=(ROOT/'android/app/src/main/java/com/vibecoder/shell/MainActivity.java').read_text()
manager=(ROOT/'android/app/src/main/java/com/vibecoder/shell/JcodeRuntimeDeliveryManager.java').read_text()
receiver=(ROOT/'android/app/src/main/java/com/vibecoder/shell/JcodeRuntimeInstallReceiver.java').read_text()
ui=(ROOT/'android/app/src/main/java/com/vibecoder/shell/JcodeRuntimeSetupUi.java').read_text()
workflow=(ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
alpha=(ROOT/'scripts/part34_alpha_build_and_verify.sh').read_text()
verify=(ROOT/'scripts/verify_android_diagnostic_apk.sh').read_text()
build_split=(ROOT/'scripts/build_jcode_runtime_release.sh').read_text()
extract_split=(ROOT/'scripts/extract_jcode_runtime_split.py').read_text()
verify_split=(ROOT/'scripts/verify_jcode_runtime_split.py').read_text()
descriptor=json.loads((ROOT/'android/app/src/main/assets/runtime/jcode-runtime-download.json').read_text())

for token in ('dynamicFeatures += setOf(":jcode_runtime")','versionCode = 33','versionName = "0.33.0-dev"','enableSplit = false'):
    need(app_gradle,token,'app_gradle')
need(settings,'include(":jcode_runtime")','settings')
for token in ('REQUEST_INSTALL_PACKAGES','JcodeRuntimeInstallReceiver'):
    need(manifest,token,'manifest')
if 'SplitCompatApplication' in manifest: die('play_splitcompat_application_survived')
for token in ('JcodeRuntimeDeliveryManager','JcodeRuntimeSetupUi','resolveInstalledJcodeDirectory','packaged Node.js runtime missing','jcodeRoot.getCanonicalPath()'):
    need(activity,token,'activity')
if 'initializeNodeRuntimeDelivery()' in activity: die('node_play_setup_still_primary')
for token in ('PackageInstaller.SessionParams.MODE_INHERIT_EXISTING','canRequestPackageInstalls()','ACTION_MANAGE_UNKNOWN_APP_SOURCES','jcode-runtime-download.apk','findLibrary(JCODE_LIBRARY_NAME)','https://github.com/sassypotatoo/Vibe-Coder/releases/download/'):
    need(manager,token,'manager')
if 'SplitInstallManager' in manager: die('play_split_installer_used_for_jcode')
for token in ('STATUS_PENDING_USER_ACTION','STATUS_SUCCESS','Intent.EXTRA_INTENT'):
    need(receiver,token,'receiver')
for token in ('Downloading Jcode runtime','✓ Node.js Android Runtime 24.19.0','⬇ Jcode 0.73.0'):
    need(ui,token,'ui')
for token in ('jcode-runtime-release:','Reuse fixed signed Jcode runtime release when available','Build exact Jcode once when runtime release is missing','gh release upload','node-android-proof-build:','needs: [jcode-runtime-release, node-android-proof-build]','Development Alpha APK (Jcode downloads in setup; Node packaged)'):
    need(workflow,token,'workflow')
if 'Stage exact proven Jcode payload' in workflow: die('alpha_still_stages_jcode')
for token in ('jcode_must_not_be_bundled_in_development_base_apk','node_payload_not_staged','development_alpha_download_jcode'):
    need(alpha,token,'alpha')
for token in ('development_alpha_download_jcode','jcode_must_not_be_bundled_in_development_base_apk','jcode_runtime_download_descriptor_missing'):
    need(verify,token,'apk_verifier')
for token in ('bundletool-all-1.18.3.jar','a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29','stage_jcode_runtime_split.py','extract_jcode_runtime_split.py','verify_jcode_runtime_split.py','apksigner','split_signing_certificate_mismatch','zipalign'):
    need(build_split,token,'split_builder')
need(extract_split,"'jcode_runtime'",'split_extractor')
need(verify_split,"lib/arm64-v8a/libvibecoder_jcode_exec.so",'split_verifier')
if descriptor.get('base_version_code')!=33 or descriptor.get('split_name')!='jcode_runtime' or descriptor.get('version')!='0.73.0': die('descriptor_identity')
if descriptor.get('download_url')!='https://github.com/sassypotatoo/Vibe-Coder/releases/download/vibecoder-jcode-runtime-0.73.0-dev-v33/vibecoder-jcode-runtime-arm64-v8a.apk': die('descriptor_url')

for token in ('"installed".equals(persisted)', 'new State("restart_required"', 'activity.finishAffinity()'):
    if token not in manager: die('jcode_restart_activation_guard_missing:'+token)
for token in ('CLOSE APP, THEN REOPEN', 'Jcode installed ✓ Close VibeCoder once'):
    if token not in ui: die('jcode_restart_activation_ui_missing:'+token)

print('Part 34.10.18 downloadable Jcode runtime regression PASSED')
