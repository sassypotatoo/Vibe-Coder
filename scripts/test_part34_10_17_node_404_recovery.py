#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def die(code: str) -> None:
    raise SystemExit('test_part34_10_17_node_404_recovery: ' + code)


delivery = (ROOT / 'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeDeliveryManager.java').read_text()
setup_ui = (ROOT / 'android/app/src/main/java/com/vibecoder/shell/NodeRuntimeSetupUi.java').read_text()
node_workflow = (ROOT / '.github/workflows/node-runtime-proof.yml').read_text()
alpha_workflow = (ROOT / '.github/workflows/android-diagnostic-apk.yml').read_text()

# The app must keep using the exact, versioned runtime URL. Recovery is retry/sequencing,
# not a hidden switch to a different Node build.
for token in (
    'RUNTIME_RELEASE_TAG = "vibecoder-node-runtime-24.19.0-v31"',
    'RUNTIME_APK_NAME = "vibecoder-node-runtime-arm64-v31.apk"',
    'https://github.com/sassypotatoo/Vibe-Coder/releases/download/',
    'DOWNLOAD_MAX_ATTEMPTS = 6',
    'DOWNLOAD_RETRY_DELAYS_MS',
    'isRetryableDownloadStatus(status)',
    'HttpURLConnection.HTTP_NOT_FOUND',
    'status == 429',
    'status >= 500',
    'sleepWithCancellation',
    'waiting_for_release',
):
    if token not in delivery:
        die('download_retry_contract_missing:' + token)

for token in (
    'waiting_for_release',
    'Node.js release is still becoming available. Retrying',
    'Retrying',
):
    if token not in setup_ui:
        die('retry_ui_missing:' + token)

# The Node release job is now the authority that makes the URL true. Only after
# public download + integrity verification passes may it kick a fresh Alpha build.
for token in (
    'contents: write',
    'actions: write',
    'Verify published Node runtime download URL',
    'Published Node runtime public URL verification PASSED',
    'Dispatch fresh Alpha after runtime is public',
    'gh workflow run android-diagnostic-apk.yml',
    '--ref "$GITHUB_REF_NAME"',
):
    if token not in node_workflow:
        die('node_publish_sequence_missing:' + token)

# The source-push Alpha/publication race is tested separately by Part 34.10.18.
# Part 34.10.17 owns app-side bounded retry plus Node publication sequencing.

# Guard against regressions to previously rejected delivery paths.
for forbidden in (
    'SplitInstallManager',
    'com.google.android.play:feature-delivery',
    'node-v24.19.0-linux',
):
    if forbidden in delivery:
        die('forbidden_delivery_regression:' + forbidden)

print('Part 34.10.17 Node HTTP 404 recovery regression PASSED')
