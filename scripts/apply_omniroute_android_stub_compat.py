#!/usr/bin/env python3
"""Apply hash-pinned OmniRoute 3.8.50 Android minimal-stub compatibility repairs."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
META = ROOT / "third_party" / "patches" / "omniroute-3.8.50-android-stub-compat.json"

PATCHED_CONTENT = {
    "src/lib/services/installers/ninerouter.stub.ts": '/**\n * Android/backend-only compatibility stub for `ninerouter.ts`.\n *\n * The minimal OmniRoute profile must preserve the original module\'s export\n * surface even though 9router installation/spawn is intentionally disabled.\n */\nimport { featureDisabledError } from "@/lib/build-profile/featureDisabled";\n\nconst FEATURE = "9router-installer";\n\nexport const NINEROUTER_PACKAGE = "9router";\nexport const NINEROUTER_INSTALL_DIR = "";\n\nexport interface InstallResult {\n  installedVersion: string;\n  installPath: string;\n  durationMs: number;\n}\n\nexport interface SpawnArgs {\n  command: string;\n  args: string[];\n  env: NodeJS.ProcessEnv;\n  cwd: string;\n}\n\nexport async function getInstalledVersion(): Promise<string | null> {\n  return null;\n}\n\nexport async function getLatestVersion(): Promise<string | null> {\n  return null;\n}\n\nexport async function install(_version = "latest"): Promise<never> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function update(): Promise<never> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function uninstall(): Promise<never> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function installNinerouter(): Promise<never> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport function resolveSpawnArgs(_apiKey: string, _port: number): never {\n  throw featureDisabledError(FEATURE);\n}\n',
    "src/mitm/cert/install.stub.ts": '/**\n * Android/backend-only compatibility stub for `src/mitm/cert/install.ts`.\n *\n * Privileged trust-store mutation stays physically disabled, while the stub\n * preserves the upstream module\'s public export surface so API route modules\n * can be statically linked by Turbopack.\n */\nimport { featureDisabledError } from "../../lib/build-profile/featureDisabled.ts";\n\nconst FEATURE = "mitm-cert-install";\nconst DISABLED_MESSAGE = "MITM certificate installation is disabled in this build";\n\nexport type CertInstallReason = "canceled" | "environment";\n\nexport interface CertManualGuide {\n  platform: NodeJS.Platform;\n  certPath: string;\n  downloadUrl: string;\n  steps: string[];\n}\n\nexport interface CertInstallResult {\n  installed: boolean;\n  skipped: boolean;\n  reason?: CertInstallReason;\n  message?: string;\n  manualGuide?: CertManualGuide;\n}\n\nexport async function checkCertInstalled(_certPath: string): Promise<boolean> {\n  return false;\n}\n\nexport function macCertOutputHasFingerprint(securityOutput: string, fingerprint: string): boolean {\n  const normalize = (value: string) => value.replace(/:/g, "").toUpperCase();\n  return normalize(securityOutput).includes(normalize(fingerprint));\n}\n\nexport function certutilThumbprint(_certPath: string): never {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function installCert(_sudoPassword: string, _certPath: string): Promise<void> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport function classifyCertInstallError(message: string): CertInstallReason {\n  return /cancel+ed/i.test(message) ? "canceled" : "environment";\n}\n\nexport function buildCertManualGuide(\n  certPath: string,\n  platform: NodeJS.Platform = process.platform\n): CertManualGuide {\n  return {\n    platform,\n    certPath,\n    downloadUrl: "",\n    steps: [DISABLED_MESSAGE],\n  };\n}\n\nexport async function installCertResult(\n  _sudoPassword: string,\n  certPath: string\n): Promise<CertInstallResult> {\n  return {\n    installed: false,\n    skipped: true,\n    reason: "environment",\n    message: DISABLED_MESSAGE,\n    manualGuide: buildCertManualGuide(certPath),\n  };\n}\n\nexport async function installCaCert(\n  sudoPassword: string,\n  caCertPath: string\n): Promise<CertInstallResult> {\n  return installCertResult(sudoPassword, caCertPath);\n}\n\nexport async function ensureSystemCertMode(_destFile: string, _sudoPassword: string): Promise<void> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function uninstallCert(_sudoPassword: string, _certPath: string): Promise<void> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport function buildWindowsDelstoreScript(_thumbprint: string): never {\n  throw featureDisabledError(FEATURE);\n}\n',
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("omniroute_root", type=Path)
    args = parser.parse_args()

    meta = json.loads(META.read_text(encoding="utf-8"))
    entries = {entry["target_path"]: entry for entry in meta["files"]}
    if set(entries) != set(PATCHED_CONTENT):
        raise SystemExit("omniroute_android_stub_compat_manifest_paths_mismatch")

    pending: dict[Path, bytes] = {}
    for relative, content in PATCHED_CONTENT.items():
        target = args.omniroute_root / relative
        if not target.is_file():
            raise SystemExit(f"omniroute_android_stub_compat_target_missing:{relative}")
        current_sha = digest(target.read_bytes())
        entry = entries[relative]
        expected_output = entry["expected_patched_sha256"]
        if current_sha == expected_output:
            continue
        if current_sha != entry["required_upstream_sha256"]:
            raise SystemExit(
                f"omniroute_android_stub_compat_input_hash_mismatch:{relative}:{current_sha}"
            )
        patched = content.encode("utf-8")
        patched_sha = digest(patched)
        if patched_sha != expected_output:
            raise SystemExit(
                f"omniroute_android_stub_compat_output_hash_mismatch:{relative}:{patched_sha}"
            )
        pending[target] = patched

    for target, patched in pending.items():
        target.write_bytes(patched)
        print(
            "Patched OmniRoute Android stub contract "
            f"{target.relative_to(args.omniroute_root)} -> {digest(patched)}"
        )

    for relative, entry in entries.items():
        target = args.omniroute_root / relative
        final_sha = digest(target.read_bytes())
        if final_sha != entry["expected_patched_sha256"]:
            raise SystemExit(
                f"omniroute_android_stub_compat_final_hash_mismatch:{relative}:{final_sha}"
            )

    print("OmniRoute Android minimal-stub compatibility profile applied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
