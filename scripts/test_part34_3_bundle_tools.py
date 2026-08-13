#!/usr/bin/env python3
"""Deterministic regression tests for Part 34.3 Android bundle sealing tools."""
from __future__ import annotations
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEAL = ROOT / "scripts" / "prepare_omniroute_android_bundle.py"
VERIFY = ROOT / "scripts" / "verify_omniroute_android_bundle.py"


def run(cmd, expect=0):
    p = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if p.returncode != expect:
        raise AssertionError(f"unexpected rc={p.returncode} expected={expect}: {' '.join(map(str, cmd))}\n{p.stdout}")
    return p.stdout


def fixture(root: Path) -> None:
    for rel in [
        "build", "migrations", "node_modules/sql.js/dist", "node_modules/sharp/build",
        "node_modules/sqlite-vec-linux-x64", "node_modules/@img/sharp-linux-x64",
        "src/mitm/tproxy/native/build/Release", "assets",
    ]:
        (root / rel).mkdir(parents=True, exist_ok=True)
    (root / "package.json").write_text('{"name":"omniroute","version":"3.8.50"}\n')
    (root / "server.js").write_text('console.log("server")\n')
    (root / "server-ws.mjs").write_text('await import("./server.js")\n')
    (root / "build/runtime-env.mjs").write_text('export {}\n')
    (root / "build/bootstrap-env.mjs").write_text('export {}\n')
    (root / "healthcheck.mjs").write_text('console.log("ok")\n')
    (root / "migrations/001.sql").write_text('migration\n')
    (root / "node_modules/sql.js/package.json").write_text('{"name":"sql.js","version":"1.14.1"}\n')
    (root / "node_modules/sql.js/dist/sql-wasm.wasm").write_bytes(b"\0asmFAKE")
    (root / "assets/inside.txt").write_text("inside\n")
    (root / "assets/inside-link.txt").symlink_to("inside.txt")
    (root / "node_modules/sharp/build/sharp.node").write_bytes(b"\x7fELFjunk")
    (root / "node_modules/sqlite-vec-linux-x64/vec0.so").write_bytes(b"\x7fELFjunk")
    (root / "node_modules/@img/sharp-linux-x64/sharp.node").write_bytes(b"\x7fELFjunk")
    (root / "src/mitm/tproxy/native/build/Release/transparent.node").write_bytes(b"\x7fELFjunk")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-part343-test-") as td:
        base = Path(td)
        src = base / "src"; out = base / "out"; fixture(src)
        run([sys.executable, str(SEAL), str(src), str(out)])
        run([sys.executable, str(VERIFY), str(out)])
        if (out / "node_modules/sharp").exists() or (out / "node_modules/sqlite-vec-linux-x64").exists():
            raise AssertionError("known forbidden native package survived pruning")
        if (out / "assets/inside-link.txt").is_symlink():
            raise AssertionError("internal symlink was not materialized")
        manifest = json.loads((out / ".vibecoder-omniroute-bundle.json").read_text())
        if manifest["apk_asset_packaged"] or manifest["service_round_trip_proven"]:
            raise AssertionError("synthetic bundle overclaimed runtime proof")

        # Hash tamper must fail.
        (out / "server.js").write_text("tampered\n")
        text = run([sys.executable, str(VERIFY), str(out)], expect=1)
        if "omniroute_android_bundle_file_manifest_mismatch" not in text:
            raise AssertionError("tamper did not fail on file manifest mismatch")

        # Unknown native addon must fail the sealer.
        bad = base / "bad-native"; shutil.copytree(src, bad, symlinks=True)
        (bad / "node_modules/unknown-native").mkdir(parents=True)
        (bad / "node_modules/unknown-native/addon.node").write_bytes(b"\x7fELFunknown")
        text = run([sys.executable, str(SEAL), str(bad), str(base / "bad-native-out")], expect=1)
        if "omniroute_android_bundle_host_native_binary_forbidden" not in text:
            raise AssertionError("unknown native addon was not rejected")

        # External symlink must fail before copying arbitrary host files.
        ext = base / "external"; shutil.copytree(src, ext, symlinks=True)
        (ext / "assets/external-link").symlink_to("/etc/passwd")
        text = run([sys.executable, str(SEAL), str(ext), str(base / "external-out")], expect=1)
        if "omniroute_bundle_external_symlink_forbidden" not in text:
            raise AssertionError("external symlink was not rejected")

        # Independent verifier must enforce forbidden-package policy even if bytes are pure JS.
        clean = base / "clean"; fixture(clean)
        sealed = base / "sealed"; run([sys.executable, str(SEAL), str(clean), str(sealed)])
        forbidden = sealed / "node_modules/wreq-js"; forbidden.mkdir(parents=True)
        (forbidden / "index.js").write_text("module.exports = {}\n")
        # Rewriting the manifest is intentionally not enough to bypass policy; verifier checks package roots itself.
        text = run([sys.executable, str(VERIFY), str(sealed)], expect=1)
        if "omniroute_android_bundle_forbidden_package:wreq-js" not in text:
            raise AssertionError("verifier did not independently enforce forbidden packages")

    print("Part 34.3 bundle-tool regression PASSED")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
