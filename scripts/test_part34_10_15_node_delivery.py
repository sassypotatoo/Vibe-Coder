#!/usr/bin/env python3
"""Regression for the current development Node delivery contract.

Part 34.10.15 originally introduced Play on-demand Node. During pre-Play development the contract is
intentionally different: Node 24.19.0 is a verified package-owned executable in the base APK so a
sideloaded Alpha never depends on Google Play. Jcode is the runtime downloaded during setup.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def die(code: str) -> None:
    raise SystemExit("test_part34_10_15_node_delivery: " + code)

workflow = (ROOT / ".github/workflows/android-diagnostic-apk.yml").read_text()
node_workflow = (ROOT / ".github/workflows/node-runtime-proof.yml").read_text()
activity = (ROOT / "android/app/src/main/java/com/vibecoder/shell/MainActivity.java").read_text()
alpha = (ROOT / "scripts/part34_alpha_build_and_verify.sh").read_text()
verify_apk = (ROOT / "scripts/verify_android_diagnostic_apk.sh").read_text()
evidence = (ROOT / "scripts/write_alpha_build_evidence.py").read_text()
host = (ROOT / "crates/vibecoder-android-host/src/lib.rs").read_text()
inventory = json.loads((ROOT / "config/android-runtime-inventory.json").read_text())

for token in (
    "node-android-proof-build:",
    "Exact Node 24.19.0 Android cross-compile",
    "id: reuse-node",
    "Cross-compile exact Node 24.19.0 for Android Bionic",
    "vibecoder-node-24.19.0-android-arm64-development",
    "needs: [jcode-runtime-release, node-android-proof-build]",
    "Development Alpha APK (Jcode downloads in setup; Node packaged)",
    "vibecoder-part34-development-alpha-apk",
):
    if token not in workflow:
        die("development_packaged_node_workflow_missing:" + token)

for token in (
    "workflow_dispatch:",
    "node-android-proof-build:",
    "timeout-minutes: 360",
    "retention-days: 90",
    "Fail-fast source and Node build-contract validation",
    "python3 scripts/validate_checkpoint.py",
    "python3 scripts/test_part34_10_compile_repairs.py",
):
    if token not in node_workflow:
        die("dedicated_node_workflow_missing:" + token)

for token in (
    "libvibecoder_node_exec.so",
    "node_cross_build_evidence_missing",
    "verify_node_cross_build_evidence.py",
    'verify_android_diagnostic_apk.sh" "$APK" development_alpha_download_jcode',
):
    if token not in alpha:
        die("development_alpha_node_guard_missing:" + token)

for token in (
    "File nodeExecutable = new File(nativeRoot, NODE_FILE_NAME).getCanonicalFile();",
    "packaged Node.js runtime missing",
    "nativeRoot.getCanonicalPath()",
):
    if token not in activity:
        die("android_base_node_runtime_missing:" + token)

for token in (
    '"development_alpha_download_jcode"',
    "lib/arm64-v8a/libvibecoder_node_exec.so",
    "node_native_entry_missing",
):
    if token not in verify_apk:
        die("development_node_apk_verifier_missing:" + token)

for token in (
    "'delivery':'development_base_apk'",
    "'bundled_in_base_apk':True",
    "NODE",
):
    if token not in evidence:
        die("development_node_evidence_missing:" + token)

components = {item.get("component_id"): item for item in inventory.get("components", []) if isinstance(item, dict)}
node = components.get("node", {})
if node.get("version_requirement") != "24.19.0":
    die("node_version_drift")
if node.get("placement") != "apk_native_executable" or node.get("bundled_in_base") is not True:
    die("node_development_placement_invalid")

for token in (
    "RuntimePlacement::ApkNativeExecutable => self.paths.native_library_dir()",
    "RuntimePlacement::PlayFeatureNativeExecutable | RuntimePlacement::PackageSplitNativeExecutable => {",
):
    if token not in host:
        die("execution_root_contract_missing:" + token)

print("Part 34.10.15 Node delivery regression PASSED (development base-packaged Node)")
