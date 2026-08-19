#!/usr/bin/env python3
"""Patch reviewed OmniRoute backend-only build to stub desktop-only API routes for Android."""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

TARGET = Path("scripts/build/backendOnlyPages.mjs")
EXPECTED_UPSTREAM_GIT_BLOB_SHA1 = "9719e8918ec1d88cb9a9829cb3a61c51fc2ea59d"
PATCH_MARKER = "omniroute:vibecoder-android-minimal-route-stub"

# Core VibeCoder gateway routes are intentionally NOT in this list:
# /v1/models, /v1/chat/completions, /v1/vibecoder/runtime-profile.
MINIMAL_DISABLED_ROUTES = (
    "api/openapi/spec/route.ts",
    "api/cli-tools/codewhale-settings/route.ts",
    "api/cli-tools/crush-settings/route.ts",
    "api/cli-tools/jcode-settings/route.ts",
    "api/cli-tools/pi-settings/route.ts",
    "api/cli-tools/smelt-settings/route.ts",
    "api/cli-tools/grok-build-settings/route.ts",
    "api/cli-tools/deepseek-tui-settings/route.ts",
    "api/cli-tools/forge-settings/route.ts",
    "api/cli-tools/guide-settings/[toolId]/route.ts",
    "api/tools/agent-bridge/agents/[id]/route.ts",
    "api/tools/agent-bridge/server/route.ts",
    "api/tools/agent-bridge/tproxy/route.ts",
    "api/tunnels/tailscale/start-daemon/route.ts",
    "api/tunnels/cloudflared/route.ts",
    "api/settings/mitm/route.ts",
    "api/db-backups/exportAll/route.ts",
    "api/db-backups/import/route.ts",
    "api/oauth/kiro/auto-import/route.ts",
    "api/oauth/raycast/auto-import/route.ts",
    "api/providers/agy-auth/apply-local/route.ts",
)


def git_blob_sha1(data: bytes) -> str:
    return hashlib.sha1(f"blob {len(data)}\0".encode("ascii") + data).hexdigest()


def patch_text(text: str) -> str:
    if PATCH_MARKER in text:
        return text

    route_line = r"const ROUTE_FILE_RE = /[\\/]route\.(ts|js|tsx|jsx)$/;"
    if route_line not in text:
        raise ValueError("omniroute_android_minimal_pruning_route_anchor_missing")

    routes = "\n".join(f'  "{route}",' for route in MINIMAL_DISABLED_ROUTES)
    definitions = (
        f"\n\n// {PATCH_MARKER}\n"
        "// VibeCoder Android keeps the OpenAI-compatible core gateway hot while desktop-only\n"
        "// management routes are replaced before Turbopack traces their filesystem probes.\n"
        "const VIBECODER_ANDROID_MINIMAL_DISABLED_ROUTES = new Set([\n"
        f"{routes}\n"
        "]);\n\n"
        "const VIBECODER_ANDROID_MINIMAL_ROUTE_STUB = `${HEADER}\n"
        "export const dynamic = \"force-dynamic\";\n"
        "export const runtime = \"nodejs\";\n"
        "const disabled = () => new Response(\n"
        "  JSON.stringify({ error: { type: \"feature_disabled\", message: \"Disabled in VibeCoder Android minimal backend\" } }),\n"
        "  { status: 501, headers: { \"content-type\": \"application/json; charset=utf-8\" } }\n"
        ");\n"
        "export async function GET() { return disabled(); }\n"
        "export async function HEAD() { return new Response(null, { status: 501 }); }\n"
        "export async function OPTIONS() { return new Response(null, { status: 501 }); }\n"
        "export async function POST() { return disabled(); }\n"
        "export async function PUT() { return disabled(); }\n"
        "export async function PATCH() { return disabled(); }\n"
        "export async function DELETE() { return disabled(); }\n"
        "`;\n"
    )
    text = text.replace(route_line, route_line + definitions, 1)

    loop_anchor = "  for (const file of walkFiles(appDir)) {\n    const stub = stubFor(file, appDir);"
    if loop_anchor not in text:
        raise ValueError("omniroute_android_minimal_pruning_loop_anchor_missing")

    injected = '''  for (const file of walkFiles(appDir)) {
    const relativeRoute = path.relative(appDir, file).split(path.sep).join("/");
    if (
      process.env.OMNIROUTE_BUILD_PROFILE === "minimal" &&
      VIBECODER_ANDROID_MINIMAL_DISABLED_ROUTES.has(relativeRoute)
    ) {
      let original;
      try {
        original = fs.readFileSync(file, "utf8");
      } catch {
        continue;
      }
      if (original.includes(BACKEND_ONLY_STUB_MARKER)) continue;
      try {
        fs.writeFileSync(file, VIBECODER_ANDROID_MINIMAL_ROUTE_STUB, "utf8");
        stubbed.push({ file, original });
      } catch (err) {
        log.warn?.(`[backend-only] Could not minimal-stub ${file}: ${err?.message || err}`);
      }
      continue;
    }

    const stub = stubFor(file, appDir);'''
    return text.replace(loop_anchor, injected, 1)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("source_root", type=Path)
    args = ap.parse_args()
    target = args.source_root / TARGET
    if not target.is_file():
        raise SystemExit(f"omniroute_android_minimal_pruning_target_missing:{TARGET.as_posix()}")
    raw = target.read_bytes()
    text = raw.decode("utf-8")
    if PATCH_MARKER not in text:
        actual = git_blob_sha1(raw)
        if actual != EXPECTED_UPSTREAM_GIT_BLOB_SHA1:
            raise SystemExit(f"omniroute_android_minimal_pruning_input_blob_mismatch:{actual}")
    try:
        patched = patch_text(text)
    except ValueError as exc:
        raise SystemExit(str(exc))
    if PATCH_MARKER not in patched:
        raise SystemExit("omniroute_android_minimal_pruning_marker_missing")
    for route in MINIMAL_DISABLED_ROUTES:
        if f'"{route}"' not in patched:
            raise SystemExit(f"omniroute_android_minimal_pruning_route_missing:{route}")
    target.write_text(patched, encoding="utf-8")
    print(
        "OmniRoute Android minimal route-pruning profile applied: "
        f"routes={len(MINIMAL_DISABLED_ROUTES)} sha256={hashlib.sha256(patched.encode()).hexdigest()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
