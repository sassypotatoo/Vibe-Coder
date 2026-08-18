#!/usr/bin/env python3
"""Regression for Android-only OmniRoute backend route pruning."""
from __future__ import annotations
import importlib.util
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATCHER = ROOT / "scripts" / "apply_omniroute_android_minimal_build_pruning.py"


def load_module():
    spec = importlib.util.spec_from_file_location("minimal_pruning", PATCHER)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def fixture() -> str:
    return r'''import fs from "node:fs";
import path from "node:path";
export const BACKEND_ONLY_STUB_MARKER = "/* backend */";
const HEADER = `${BACKEND_ONLY_STUB_MARKER}\n`;
const ROUTE_FILE_RE = /[\\/]route\.(ts|js|tsx|jsx)$/;
function walkFiles() { return []; }
function stubFor() { return null; }
export function stubDashboardPages(rootDir = process.cwd(), log = console) {
  const appDir = path.join(rootDir, "src", "app");
  const stubbed = [];
  for (const file of walkFiles(appDir)) {
    const stub = stubFor(file, appDir);
    if (stub) stubbed.push({ file, original: "" });
    if (ROUTE_FILE_RE.test(file)) {}
  }
  return stubbed;
}
'''


def main() -> int:
    m = load_module()
    patched = m.patch_text(fixture())
    if m.PATCH_MARKER not in patched:
        raise AssertionError("minimal route patch marker missing")
    for protected in ("v1/models", "v1/chat/completions", "v1/vibecoder/runtime-profile"):
        if protected in "\n".join(m.MINIMAL_DISABLED_ROUTES):
            raise AssertionError(f"protected gateway route was disabled: {protected}")
    for route in (
        "api/cli-tools/codewhale-settings/route.ts",
        "api/tools/agent-bridge/agents/[id]/route.ts",
        "api/tunnels/cloudflared/route.ts",
        "api/db-backups/exportAll/route.ts",
    ):
        if f'"{route}"' not in patched:
            raise AssertionError(f"high-cost desktop route missing from pruning set: {route}")
    if 'process.env.OMNIROUTE_BUILD_PROFILE === "minimal"' not in patched:
        raise AssertionError("route pruning is not gated to Android minimal build profile")
    if "new Response" not in patched or "status: 501" not in patched:
        raise AssertionError("disabled route stub does not preserve HTTP linkage")
    if m.patch_text(patched) != patched:
        raise AssertionError("minimal route pruning patch is not idempotent")

    with tempfile.TemporaryDirectory(prefix="vibecoder-minimal-prune-") as td:
        target = Path(td) / "patched.mjs"
        target.write_text(patched, encoding="utf-8")
        result = subprocess.run(["node", "--check", str(target)], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        if result.returncode != 0:
            raise AssertionError("patched backendOnlyPages syntax invalid:\n" + result.stdout)

    print("Part 34.3 Android minimal-build route-pruning regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
