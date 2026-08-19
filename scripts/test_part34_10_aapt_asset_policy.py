#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "scripts" / "verify_omniroute_aapt_asset_policy.py"
DEFAULT = "!.svn:!.git:!.ds_store:!*.scc:.*:<dir>_*:!CVS:!thumbs.db:!picasa.ini:!*~"
SENTINEL = "__vibecoder_aapt_ignore_none__"

spec = importlib.util.spec_from_file_location("aapt_policy", POLICY)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

# Reproduce both real CI packaging losses: dot-prefixed Next state and `_not-found`.
assert module.aapt_name_ignored(".vibecoder-omniroute-bundle.json", False, DEFAULT)
assert module.aapt_name_ignored(".next", True, DEFAULT)
assert module.aapt_name_ignored("_not-found", True, DEFAULT)
assert not module.aapt_name_ignored(".vibecoder-omniroute-bundle.json", False, SENTINEL)
assert not module.aapt_name_ignored(".next", True, SENTINEL)
assert not module.aapt_name_ignored("_not-found", True, SENTINEL)

# Lock the whole Gradle/Ant default-excluded SCM/editor metadata class, not just the latest
# .gitattributes failure. Legitimate Next/HTTP runtime names must remain outside this class.
meta_spec = importlib.util.spec_from_file_location(
    "packaging_metadata_policy", ROOT / "scripts" / "omniroute_android_packaging_metadata_policy.py"
)
meta = importlib.util.module_from_spec(meta_spec)
assert meta_spec.loader is not None
meta_spec.loader.exec_module(meta)
for name in (
    ".gitattributes", ".gitignore", ".gitmodules", ".git", ".svn", ".hg", ".bzr",
    "CVS", "SCCS", ".DS_Store", "vssver.scc", "#autosave#", "%temp%", "scratch~",
    ".#lock", "._resource",
):
    assert meta.is_gradle_default_excluded_metadata_name(name), name
for name in (".next", "_not-found", ".well-known", ".vibecoder-omniroute-bundle.json", "package.json"):
    assert not meta.is_gradle_default_excluded_metadata_name(name), name

with tempfile.TemporaryDirectory(prefix="vibecoder-aapt-policy-") as td:
    root = Path(td) / "bundle"
    for rel, data in {
        ".vibecoder-omniroute-bundle.json": b"{}",
        ".next/server/app/_not-found/page.js": b"runtime",
        ".well-known/agent.json": b"{}",
        "node_modules/sql.js/package.json": b"{}",
        # exact file Gradle dropped in CI run #26
        "node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes": b"* text=auto\n",
    }.items():
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    gradle_rejected = subprocess.run([sys.executable, str(POLICY), str(root)], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if gradle_rejected.returncode == 0 or "omniroute_gradle_asset_metadata_conflict:node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes" not in gradle_rejected.stdout:
        raise SystemExit(f"gradle_default_metadata_not_rejected:{gradle_rejected.stdout}")
    (root / "node_modules/roarr/node_modules/sprintf-js/dist/.gitattributes").unlink()

    result = subprocess.run([sys.executable, str(POLICY), str(root)], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode != 0 or "OmniRoute AAPT asset transparency gate PASSED" not in result.stdout:
        raise SystemExit(f"aapt_transparency_fixture_failed:{result.stdout}")

    collision = root / SENTINEL
    collision.write_bytes(b"collision")
    rejected = subprocess.run([sys.executable, str(POLICY), str(root)], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if rejected.returncode == 0 or "omniroute_aapt_policy_would_drop_runtime_entries" not in rejected.stdout:
        raise SystemExit(f"aapt_sentinel_collision_not_rejected:{rejected.stdout}")

print("Part 34.10 AAPT asset-transparency regression PASSED")
