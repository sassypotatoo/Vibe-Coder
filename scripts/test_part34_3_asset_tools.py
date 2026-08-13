#!/usr/bin/env python3
"""Regression tests for Part 34.3.3 OmniRoute asset staging and install contracts."""
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEAL = ROOT / "scripts" / "prepare_omniroute_android_bundle.py"
STAGE = ROOT / "scripts" / "stage_omniroute_android_asset.py"
VERIFY = ROOT / "scripts" / "verify_omniroute_android_bundle.py"
PROFILE = json.loads((ROOT / "config" / "omniroute-android-runtime-profile.json").read_text())


def run(*args: str, ok: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(args, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if ok and result.returncode != 0:
        raise AssertionError(result.stdout)
    if not ok and result.returncode == 0:
        raise AssertionError("command unexpectedly succeeded")
    return result


def make_standalone(root: Path) -> None:
    required_files = {
        "server.js": "export default {};\n",
        "server-ws.mjs": "console.log('fixture');\n",
        "package.json": json.dumps({"name": "omniroute", "version": "3.8.50"}) + "\n",
        "build/runtime-env.mjs": "export {};\n",
        "build/bootstrap-env.mjs": "export {};\n",
        "healthcheck.mjs": "export {};\n",
        "migrations/0001.sql": "select 1;\n",
        "node_modules/sql.js/package.json": json.dumps({"name": "sql.js", "version": "1.13.0"}) + "\n",
    }
    for relative, content in required_files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-omni-asset-test-") as tmp_raw:
        tmp = Path(tmp_raw)
        standalone = tmp / "standalone"
        sealed = tmp / "sealed"
        assets = tmp / "generated-assets"
        evidence = tmp / "evidence.json"
        standalone.mkdir()
        make_standalone(standalone)

        run(sys.executable, str(SEAL), str(standalone), str(sealed), "--allowed-symlink-root", str(standalone))
        run(sys.executable, str(STAGE), str(sealed), "--assets-root", str(assets), "--evidence", str(evidence))
        staged = assets / "omniroute" / "bundle"
        run(sys.executable, str(VERIFY), str(staged))
        info = json.loads(evidence.read_text())
        manifest = json.loads((staged / ".vibecoder-omniroute-bundle.json").read_text())
        if info.get("apk_asset_staged") is not True:
            raise AssertionError("asset staging evidence missing true staging marker")
        if info.get("apk_asset_packaging_proven") is not False or info.get("device_extraction_proven") is not False:
            raise AssertionError("asset staging evidence overclaimed APK/device proof")
        if info.get("tree_sha256") != manifest.get("tree_sha256"):
            raise AssertionError("asset staging evidence tree hash mismatch")

        # Atomic replacement must remove stale/unmanifested target contents.
        stale = staged / "stale.txt"
        stale.write_text("must disappear", encoding="utf-8")
        run(sys.executable, str(STAGE), str(sealed), "--assets-root", str(assets), "--evidence", str(evidence))
        if stale.exists():
            raise AssertionError("stale staged asset survived atomic replacement")

        # Producer must refuse to write generated content into tracked Android source assets.
        bad = run(
            sys.executable,
            str(STAGE),
            str(sealed),
            "--assets-root",
            str(ROOT / "android" / "app" / "src" / "main" / "assets"),
            "--evidence",
            str(evidence),
            ok=False,
        )
        if "omniroute_asset_stage_tracked_source_output_forbidden" not in bad.stdout:
            raise AssertionError("tracked-source output guard did not fire")

        # Tampering the sealed input must be caught before staging.
        (sealed / "server.js").write_text("tampered\n", encoding="utf-8")
        tampered = run(
            sys.executable,
            str(STAGE),
            str(sealed),
            "--assets-root",
            str(tmp / "tampered-assets"),
            "--evidence",
            str(tmp / "tampered-evidence.json"),
            ok=False,
        )
        if "omniroute_asset_stage_bundle_verification_failed" not in tampered.stdout:
            raise AssertionError("tampered input was not rejected before staging")

    java = (ROOT / "android" / "app" / "src" / "main" / "java" / "com" / "vibecoder" / "shell" / "OmniRouteAssetInstaller.java").read_text()
    for token in (
        "vibecoder/runtime/omniroute",
        "FileChannel",
        "lockChannel.lock()",
        "StandardCopyOption.ATOMIC_MOVE",
        "omniroute_asset_sha_mismatch",
        "omniroute_post_commit_verification_failed",
        ".omniroute-previous",
        ".omniroute-stage-",
        "service_round_trip_proven",
    ):
        if token not in java:
            raise AssertionError(f"installer contract token missing: {token}")

    print("Part 34.3.3 asset-tool regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
