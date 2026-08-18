#!/usr/bin/env python3
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def die(code): raise SystemExit('test_part34_10_16_sideload_alpha: ' + code)

play = (ROOT/'.github/workflows/android-play-bundle.yml').read_text()
diag = (ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
verify = (ROOT/'scripts/verify_android_diagnostic_apk.sh').read_text()
script = (ROOT/'scripts/part34_sideload_alpha_from_play_build.sh').read_text()
writer = (ROOT/'scripts/write_sideload_alpha_build_evidence.py').read_text()
delivery = (ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
ui = (ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeSetupUi.java').read_text()
play_script = (ROOT/'scripts/part34_play_bundle_build_and_verify.sh').read_text()

for token in (
    'Build verified sideload Alpha APK with packaged Node',
    'part34_sideload_alpha_from_play_build.sh',
    'vibecoder-part34-sideload-alpha-apk',
    'vibecoder-part34-sideload-alpha-build-evidence.json',
    'compression-level: 0',
):
    if token not in play: die('play_workflow_missing:' + token)

if 'vibecoder-part34-full-alpha-apk' in diag:
    die('ambiguous_old_base_alpha_artifact_name_survived')
if 'vibecoder-part34-base-alpha-play-node-required' not in diag:
    die('base_alpha_artifact_not_marked_play_node_required')

for token in ('sideload_alpha', 'jcode_native_entry_missing', 'node_native_entry_missing', 'omniroute_apk_asset_bundle_verification_failed'):
    if token not in verify: die('sideload_verifier_missing:' + token)

for token in (
    'verified_play_aab_missing',
    'verify_node_feature_bundle.py',
    'verify_node_cross_build_evidence.py',
    'install -m 0644 "$NODE" "$BASE_NODE"',
    'trap \'rm -f "$BASE_NODE"\' EXIT',
    'verify_android_diagnostic_apk.sh" "$APK" sideload_alpha',
    'write_sideload_alpha_build_evidence.py',
):
    if token not in script: die('sideload_script_missing:' + token)

for token in (
    "'delivery': 'packaged_in_sideload_base_apk'",
    "'bundled_in_base_apk': True",
    "'production_play_delivery_remains_on_demand': True",
    'packaged_node_payload_mismatch',
    'node_cross_build_evidence_payload_mismatch',
):
    if token not in writer: die('sideload_evidence_missing:' + token)

# Production AAB remains fail-closed against base-node leakage.
for token in ('rm -f "$BASE_NODE"', 'verify_node_feature_bundle.py" "$AAB" "$FEATURE_NODE"'):
    if token not in play_script: die('play_base_node_guard_regressed:' + token)

# Wrong base APK now diagnoses APP_NOT_OWNED rather than pretending the download can succeed.
for token in ('errorCode == -15', 'node_runtime_play_app_not_owned_use_sideload_alpha'):
    if token not in delivery: die('app_not_owned_mapping_missing:' + token)
for token in ('This base APK cannot download Node from Google Play.', 'Use the Sideload Alpha APK'):
    if token not in ui: die('app_not_owned_ui_missing:' + token)

print('Part 34.10.16 sideload Alpha regression PASSED')
