#!/usr/bin/env python3
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def die(code): raise SystemExit('test_part34_10_16_sideload_alpha: ' + code)

diag = (ROOT/'.github/workflows/android-diagnostic-apk.yml').read_text()
play = (ROOT/'.github/workflows/android-play-bundle.yml').read_text()
verify = (ROOT/'scripts/verify_android_diagnostic_apk.sh').read_text()
alpha = (ROOT/'scripts/part34_alpha_build_and_verify.sh').read_text()
writer = (ROOT/'scripts/write_alpha_build_evidence.py').read_text()
delivery = (ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
ui = (ROOT/'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeSetupUi.java').read_text()
play_script = (ROOT/'scripts/part34_play_bundle_build_and_verify.sh').read_text()

# The automatic development workflow itself must now produce the directly installable APK.
for token in (
    'node-android-proof-build:',
    'Cross-compile exact Node 24.19.0 for Android Bionic',
    'vibecoder-node-24.19.0-android-arm64-development',
    'needs: [jcode-android-proof-build, node-android-proof-build]',
    'Development Alpha APK (Jcode + OmniRoute + Node packaged)',
    'Stage exact proven Node payload for development APK',
    'vibecoder-part34-development-alpha-apk',
):
    if token not in diag: die('development_workflow_missing:' + token)
if 'vibecoder-part34-base-alpha-play-node-required' in diag:
    die('obsolete_play_required_development_artifact_survived')

for token in ('sideload_alpha', 'jcode_native_entry_missing', 'node_native_entry_missing', 'omniroute_apk_asset_bundle_verification_failed'):
    if token not in verify: die('development_verifier_missing:' + token)

for token in (
    'node_payload_not_staged_for_development_alpha',
    'verify_node_cross_build_evidence.py',
    'verify_android_diagnostic_apk.sh" "$APK" sideload_alpha',
    'write_alpha_build_evidence.py',
):
    if token not in alpha: die('development_alpha_script_missing:' + token)

for token in (
    "'delivery': 'packaged_in_development_base_apk'",
    "'bundled_in_base_apk': True",
    "'google_play_required_for_development_apk': False",
    "'play_delivery_deferred_until_publishing': True",
    'packaged_node_payload_mismatch',
    'node_cross_build_evidence_payload_mismatch',
):
    if token not in writer: die('development_alpha_evidence_missing:' + token)

# During development a missing packaged Node must fail locally, never trigger Play.
for token in (
    'DEVELOPMENT_PACKAGED_NODE_ONLY = true',
    'node_runtime_missing_from_development_apk',
):
    if token not in delivery: die('development_delivery_mode_missing:' + token)
for token in (
    'This development APK is missing the packaged Node.js runtime.',
    'No Google Play download is used during development.',
):
    if token not in ui: die('development_ui_missing:' + token)

# Publishing lane stays present but is intentionally deferred. Its base-node leak guard remains intact.
for token in ('rm -f "$BASE_NODE"', 'verify_node_feature_bundle.py" "$AAB" "$FEATURE_NODE"'):
    if token not in play_script: die('deferred_play_base_node_guard_regressed:' + token)
if 'vibecoder-part34-play-aab' not in play:
    die('deferred_play_workflow_missing')

print('Part 34.10.16 sideload Alpha regression PASSED')
