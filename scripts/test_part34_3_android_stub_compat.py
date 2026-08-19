#!/usr/bin/env python3
"""Regression tests for the hash-pinned OmniRoute Android minimal-stub compatibility repair."""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATCHER = ROOT / "scripts" / "apply_omniroute_android_stub_compat.py"

NINEROUTER_UPSTREAM = '/**\n * Stub for `src/lib/services/installers/ninerouter.ts` activated by\n * `OMNIROUTE_BUILD_PROFILE=minimal`. The 9router install / spawn helpers are\n * removed from the built bundle. See SECURITY.md and\n * docs/security/SOCKET_DEV_FINDINGS.md.\n */\nimport { featureDisabledError } from "@/lib/build-profile/featureDisabled";\n\nconst FEATURE = "9router-installer";\n\nexport async function installNinerouter(): Promise<never> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport function resolveSpawnArgs(_apiKey: string, _port: number): never {\n  throw featureDisabledError(FEATURE);\n}\n'
CERT_UPSTREAM = '/**\n * Stub for `src/mitm/cert/install.ts` activated by\n * `OMNIROUTE_BUILD_PROFILE=minimal`. Every function throws\n * `FeatureDisabledError("mitm-cert-install")` at runtime so the privileged\n * code paths (root-CA install, NSS DB manipulation, sudo helpers) are\n * physically absent from the built bundle. See SECURITY.md and\n * docs/security/SOCKET_DEV_FINDINGS.md.\n */\nimport { featureDisabledError } from "../../lib/build-profile/featureDisabled.ts";\n\nconst FEATURE = "mitm-cert-install";\n\nexport async function checkCertInstalled(_certPath: string): Promise<boolean> {\n  return false;\n}\n\nexport async function installCert(_sudoPassword: string, _certPath: string): Promise<void> {\n  throw featureDisabledError(FEATURE);\n}\n\nexport async function uninstallCert(_sudoPassword: string, _certPath: string): Promise<void> {\n  throw featureDisabledError(FEATURE);\n}\n'


def run(cmd: list[str], expected: int = 0) -> str:
    proc = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if proc.returncode != expected:
        raise AssertionError(
            f"unexpected rc={proc.returncode} expected={expected}: {' '.join(cmd)}\n{proc.stdout}"
        )
    return proc.stdout


def make_fixture(base: Path) -> Path:
    source = base / "OmniRoute-reviewed"
    ninerouter = source / "src/lib/services/installers/ninerouter.stub.ts"
    cert = source / "src/mitm/cert/install.stub.ts"
    ninerouter.parent.mkdir(parents=True, exist_ok=True)
    cert.parent.mkdir(parents=True, exist_ok=True)
    ninerouter.write_text(NINEROUTER_UPSTREAM, encoding="utf-8")
    cert.write_text(CERT_UPSTREAM, encoding="utf-8")
    return source


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-stub-compat-") as td:
        base = Path(td)
        source = make_fixture(base)
        output = run([sys.executable, str(PATCHER), str(source)])
        if "minimal-stub compatibility profile applied" not in output:
            raise AssertionError("stub compatibility patch did not report success")

        ninerouter = (source / "src/lib/services/installers/ninerouter.stub.ts").read_text(encoding="utf-8")
        for token in (
            "export async function getInstalledVersion",
            "export async function getLatestVersion",
            "export async function install(",
            "export async function update(",
            "export async function uninstall(",
            "export function resolveSpawnArgs",
            "featureDisabledError(FEATURE)",
        ):
            if token not in ninerouter:
                raise AssertionError(f"ninerouter stub contract missing: {token}")

        cert = (source / "src/mitm/cert/install.stub.ts").read_text(encoding="utf-8")
        for token in (
            "export async function installCertResult",
            "export async function installCaCert",
            "export async function checkCertInstalled",
            "export async function installCert",
            "export async function uninstallCert",
            "export async function ensureSystemCertMode",
            "export function buildWindowsDelstoreScript",
            "featureDisabledError(FEATURE)",
            'skipped: true',
        ):
            if token not in cert:
                raise AssertionError(f"certificate stub contract missing: {token}")

        # Idempotence: an already patched reviewed tree must still pass.
        run([sys.executable, str(PATCHER), str(source)])

        # Drift/tamper must fail closed before mutation.
        tampered = make_fixture(base / "tampered")
        target = tampered / "src/lib/services/installers/ninerouter.stub.ts"
        target.write_text(target.read_text(encoding="utf-8") + "\n// drift\n", encoding="utf-8")
        output = run([sys.executable, str(PATCHER), str(tampered)], expected=1)
        if "omniroute_android_stub_compat_input_hash_mismatch" not in output:
            raise AssertionError("tampered stub was not rejected by input hash")

    print("Part 34.3 Android minimal-stub compatibility regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
