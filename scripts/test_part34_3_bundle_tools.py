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
        "node_modules/sharp/node_modules/transitive-helper",
        "node_modules/better-sqlite3-90e2652d1716b047/prebuilds",
        "node_modules/better-sqlite3-helper",
        "node_modules/sqlite-vec-linux-x64", "node_modules/@img/sharp-linux-x64",
        "src/mitm/tproxy/native/build/Release", "assets",
        ".next/server/app/_not-found", ".well-known",
        "node_modules/roarr/node_modules/sprintf-js/dist",
        "node_modules/example-with-metadata/.git",
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
    (root / "node_modules/sharp/node_modules/transitive-helper/package.json").write_text(
        '{"name":"transitive-helper","version":"1.0.0"}\n'
    )
    traced = root / "node_modules/better-sqlite3-90e2652d1716b047"
    (traced / "package.json").write_text('{"name":"better-sqlite3","version":"12.4.1"}\n')
    (traced / "prebuilds/darwin-arm64.node").write_bytes(b"\xcf\xfa\xed\xfehost-macho")
    (root / "node_modules/better-sqlite3-helper/package.json").write_text(
        '{"name":"better-sqlite3-helper","version":"1.0.0"}\n'
    )
    (root / "node_modules/sqlite-vec-linux-x64/vec0.so").write_bytes(b"\x7fELFjunk")
    (root / "node_modules/@img/sharp-linux-x64/sharp.node").write_bytes(b"\x7fELFjunk")
    (root / "src/mitm/tproxy/native/build/Release/transparent.node").write_bytes(b"\x7fELFjunk")
    # Reproduce CI run #26: Gradle's generated-assets FileTree drops SCM metadata
    # such as nested .gitattributes before AAPT, causing a one-file manifest mismatch.
    (root / "node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes").write_text("* text=auto\n")
    (root / "node_modules/roarr/node_modules/sprintf-js/dist/.gitignore").write_text("dist-cache\n")
    (root / "node_modules/example-with-metadata/.git/config").write_text("[core]\n")
    (root / ".next/server/app/_not-found/page.js").write_text("runtime\n")
    (root / ".well-known/agent.json").write_text("{}\n")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-part343-test-") as td:
        base = Path(td)
        src = base / "src"; out = base / "out"; fixture(src)
        # The fixture intentionally gives forbidden sharp its own node_modules.
        # This reproduces the stale nested traversal that failed CI run 86951467373.
        run([sys.executable, str(SEAL), str(src), str(out)])
        run([sys.executable, str(VERIFY), str(out)])
        if (out / "node_modules/sharp").exists() or (out / "node_modules/sqlite-vec-linux-x64").exists():
            raise AssertionError("known forbidden native package survived pruning")
        if (out / "node_modules/better-sqlite3-90e2652d1716b047").exists():
            raise AssertionError("Next-traced forbidden package alias survived pruning")
        if not (out / "node_modules/better-sqlite3-helper/package.json").is_file():
            raise AssertionError("non-hash package suffix was over-pruned")
        if (out / "assets/inside-link.txt").is_symlink():
            raise AssertionError("internal symlink was not materialized")
        manifest = json.loads((out / ".vibecoder-omniroute-bundle.json").read_text())
        if manifest["apk_asset_packaged"] or manifest["service_round_trip_proven"]:
            raise AssertionError("synthetic bundle overclaimed runtime proof")
        removed_metadata = set(manifest.get("removed_gradle_default_excluded_metadata_paths", []))
        expected_metadata = {
            "node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes",
            "node_modules/roarr/node_modules/sprintf-js/dist/.gitignore",
            "node_modules/example-with-metadata/.git",
        }
        if not expected_metadata.issubset(removed_metadata):
            raise AssertionError(f"Gradle-default metadata prune evidence incomplete: {removed_metadata}")
        for rel in expected_metadata:
            if (out / rel).exists():
                raise AssertionError(f"Gradle-default metadata survived sealing: {rel}")
        for rel in (".next/server/app/_not-found/page.js", ".well-known/agent.json"):
            if not (out / rel).is_file():
                raise AssertionError(f"legitimate hidden/underscore runtime path was over-pruned: {rel}")

        # Hash tamper must fail and produce actionable mismatch diagnostics.
        (out / "server.js").write_text("tampered\n")
        mismatch_report = base / "mismatch.json"
        text = run([sys.executable, str(VERIFY), str(out), "--write-mismatch-report", str(mismatch_report)], expect=1)
        if "omniroute_android_bundle_file_manifest_mismatch" not in text:
            raise AssertionError("tamper did not fail on file manifest mismatch")
        if "omniroute_android_bundle_changed_in_packaged_tree:server.js" not in text:
            raise AssertionError("tamper diagnostics did not name changed path")
        report = json.loads(mismatch_report.read_text())
        if report.get("changed_count") != 1 or report.get("changed_first_50", [{}])[0].get("path") != "server.js":
            raise AssertionError(f"mismatch report did not bind changed file: {report}")

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

        # Independent verifier must reject Gradle-default-excluded metadata if any producer
        # reintroduces it after sealing, before a doomed APK build is attempted.
        metadata_src = base / "metadata-src"; fixture(metadata_src)
        metadata_sealed = base / "metadata-sealed"; run([sys.executable, str(SEAL), str(metadata_src), str(metadata_sealed)])
        reintroduced = metadata_sealed / "node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes"
        reintroduced.parent.mkdir(parents=True, exist_ok=True)
        reintroduced.write_text("* text=auto\n")
        text = run([sys.executable, str(VERIFY), str(metadata_sealed)], expect=1)
        if "omniroute_android_bundle_gradle_default_excluded_metadata_forbidden:node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes" not in text:
            raise AssertionError("verifier did not reject Gradle-default-excluded runtime metadata")

        # Independent verifier must enforce forbidden-package policy even if bytes are pure JS.
        clean = base / "clean"; fixture(clean)
        sealed = base / "sealed"; run([sys.executable, str(SEAL), str(clean), str(sealed)])
        forbidden = sealed / "node_modules/wreq-js"; forbidden.mkdir(parents=True)
        (forbidden / "index.js").write_text("module.exports = {}\n")
        # Rewriting the manifest is intentionally not enough to bypass policy; verifier checks package roots itself.
        text = run([sys.executable, str(VERIFY), str(sealed)], expect=1)
        if "omniroute_android_bundle_forbidden_package:wreq-js" not in text:
            raise AssertionError("verifier did not independently enforce forbidden packages")

        # Independent verification must also reject Next standalone's hashed
        # aliases of exact forbidden package roots, even if only JS bytes remain.
        traced_forbidden = sealed / "node_modules/better-sqlite3-90e2652d1716b047"
        traced_forbidden.mkdir(parents=True)
        (traced_forbidden / "index.js").write_text("module.exports = {}\n")
        text = run([sys.executable, str(VERIFY), str(sealed)], expect=1)
        if "omniroute_android_bundle_forbidden_package:better-sqlite3-90e2652d1716b047" not in text:
            raise AssertionError("verifier did not reject traced forbidden package alias")

    print("Part 34.3 bundle-tool regression PASSED")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
