#!/usr/bin/env python3
"""Prove AAPT will not silently drop any staged OmniRoute runtime path."""
from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APP_GRADLE = ROOT / "android" / "app" / "build.gradle.kts"
DEFAULT_STAGED_BUNDLE = ROOT / "android" / "app" / "build" / "generated" / "omnirouteAssets" / "omniroute" / "bundle"
EXPECTED_PATTERN = "__vibecoder_aapt_ignore_none__"
ASSIGNMENT_RE = re.compile(r'androidResources\.ignoreAssetsPattern\s*=\s*"([^"]*)"')


def aapt_name_ignored(name: str, is_dir: bool, pattern: str) -> bool:
    """Mirror AAPT's colon-delimited, case-insensitive simplified glob matcher."""
    if name in (".", ".."):
        return True
    lower = name.lower()
    for raw in pattern.split(":"):
        token = raw
        if not token:
            continue
        if token.startswith("!"):
            token = token[1:]
        if token.lower().startswith("<dir>"):
            if not is_dir:
                continue
            token = token[5:]
        elif token.lower().startswith("<file>"):
            if is_dir:
                continue
            token = token[6:]
        token_lower = token.lower()
        if token_lower.startswith("*") and len(token_lower) > 1:
            matched = lower.endswith(token_lower[1:])
        elif token_lower.endswith("*") and len(token_lower) > 1:
            matched = lower.startswith(token_lower[:-1])
        else:
            matched = lower == token_lower
        if matched:
            return True
    return False


def configured_pattern() -> str:
    text = APP_GRADLE.read_text(encoding="utf-8")
    matches = ASSIGNMENT_RE.findall(text)
    if len(matches) != 1:
        raise SystemExit(f"omniroute_aapt_ignore_policy_assignment_count:{len(matches)}")
    pattern = matches[0]
    if pattern != EXPECTED_PATTERN:
        raise SystemExit(f"omniroute_aapt_ignore_policy_not_transparent:{pattern}")
    return pattern


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("bundle_root", nargs="?", type=Path, default=DEFAULT_STAGED_BUNDLE)
    args = ap.parse_args()
    root = args.bundle_root.resolve()
    if not root.is_dir() or root.is_symlink():
        raise SystemExit("omniroute_aapt_policy_bundle_invalid")
    pattern = configured_pattern()
    conflicts: list[dict[str, object]] = []
    entries = 0
    for path in sorted(root.rglob("*"), key=lambda item: item.as_posix()):
        entries += 1
        if aapt_name_ignored(path.name, path.is_dir(), pattern):
            conflicts.append({
                "path": path.relative_to(root).as_posix(),
                "name": path.name,
                "is_dir": path.is_dir(),
            })
            if len(conflicts) >= 20:
                break
    if conflicts:
        for item in conflicts:
            print(
                "omniroute_aapt_policy_conflict:"
                f"{item['path']}:type={'dir' if item['is_dir'] else 'file'}"
            )
        raise SystemExit(f"omniroute_aapt_policy_would_drop_runtime_entries:{len(conflicts)}+")
    print("OmniRoute AAPT asset transparency gate PASSED")
    print(f"pattern={pattern} scanned_entries={entries}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
