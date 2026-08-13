#!/usr/bin/env python3
"""Apply VibeCoder's complete, hash-pinned OmniRoute 3.8.50 profile fail-closed.

This is packaging infrastructure, not a generic patcher. Every existing input file must match the
reviewed upstream SHA-256, every new file must be absent, every hunk must match exactly, and every
result must match the reviewed patched SHA-256 before any file is written.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
META_PATH = ROOT / "third_party" / "patches" / "omniroute-3.8.50-vibecoder-deterministic-routing.json"
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


@dataclass
class FilePatch:
    old_path: str | None
    new_path: str
    hunks: list[list[str]]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalized_path(header: str) -> str | None:
    value = header.split("\t", 1)[0].strip()
    if value == "/dev/null":
        return None
    if not value.startswith(("a/", "b/")):
        raise SystemExit(f"unsupported patch path: {value}")
    value = value[2:]
    if not value or value.startswith("/") or ".." in Path(value).parts:
        raise SystemExit(f"unsafe patch path: {value}")
    return value


def parse_patch(text: str) -> list[FilePatch]:
    lines = text.splitlines(keepends=True)
    output: list[FilePatch] = []
    index = 0
    while index < len(lines):
        if not lines[index].startswith("--- "):
            raise SystemExit(f"unexpected patch line {index + 1}")
        old_path = normalized_path(lines[index][4:])
        index += 1
        if index >= len(lines) or not lines[index].startswith("+++ "):
            raise SystemExit("patch file is missing a +++ header")
        new_path = normalized_path(lines[index][4:])
        if new_path is None:
            raise SystemExit("deletion patches are not supported")
        index += 1
        hunks: list[list[str]] = []
        while index < len(lines) and not lines[index].startswith("--- "):
            if not lines[index].startswith("@@ "):
                raise SystemExit(f"patch file has content outside a hunk at line {index + 1}")
            hunk = [lines[index]]
            index += 1
            while index < len(lines) and not lines[index].startswith(("@@ ", "--- ")):
                if lines[index].startswith("\\ No newline at end of file"):
                    index += 1
                    continue
                if not lines[index].startswith((" ", "+", "-")):
                    raise SystemExit(f"invalid hunk line {index + 1}")
                hunk.append(lines[index])
                index += 1
            hunks.append(hunk)
        output.append(FilePatch(old_path, new_path, hunks))
    return output


def apply_file_patch(original: bytes, file_patch: FilePatch) -> bytes:
    try:
        source = original.decode("utf-8").splitlines(keepends=True)
    except UnicodeDecodeError as error:
        raise SystemExit(f"patch input is not UTF-8: {file_patch.new_path}") from error
    result: list[str] = []
    cursor = 0
    for hunk in file_patch.hunks:
        match = HUNK.match(hunk[0].rstrip("\n"))
        if not match:
            raise SystemExit(f"invalid hunk header in {file_patch.new_path}")
        old_start = int(match.group(1))
        old_count = int(match.group(2) or "1")
        new_count = int(match.group(4) or "1")
        start_index = 0 if old_start == 0 else old_start - 1
        if start_index < cursor:
            raise SystemExit(f"overlapping hunks in {file_patch.new_path}")
        result.extend(source[cursor:start_index])
        cursor = start_index
        consumed = 0
        produced = 0
        for line in hunk[1:]:
            marker, content = line[0], line[1:]
            if marker in (" ", "-"):
                if cursor >= len(source) or source[cursor] != content:
                    raise SystemExit(f"exact hunk mismatch in {file_patch.new_path}")
                cursor += 1
                consumed += 1
            if marker in (" ", "+"):
                result.append(content)
                produced += 1
        if consumed != old_count or produced != new_count:
            raise SystemExit(f"hunk count mismatch in {file_patch.new_path}")
    result.extend(source[cursor:])
    return "".join(result).encode("utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("omniroute_root", type=Path)
    args = parser.parse_args()

    meta = json.loads(META_PATH.read_text(encoding="utf-8"))
    patch_path = ROOT / meta["patch_path"]
    patches = parse_patch(patch_path.read_text(encoding="utf-8"))
    patch_by_path = {patch.new_path: patch for patch in patches}
    manifest_by_path = {entry["target_path"]: entry for entry in meta["files"]}
    if set(patch_by_path) != set(manifest_by_path):
        raise SystemExit("patch paths and hash manifest paths differ")

    already_patched = True
    for relative, entry in manifest_by_path.items():
        target = args.omniroute_root / relative
        if not target.is_file() or digest(target.read_bytes()) != entry["expected_patched_sha256"]:
            already_patched = False
            break
    if already_patched:
        print("OmniRoute VibeCoder deterministic runtime profile already applied")
        return 0

    pending: dict[Path, bytes] = {}
    for relative, entry in manifest_by_path.items():
        target = args.omniroute_root / relative
        required = entry["required_upstream_sha256"]
        if required is None:
            if target.exists():
                raise SystemExit(f"new patch target already exists: {relative}")
            original = b""
        else:
            if not target.is_file():
                raise SystemExit(f"missing OmniRoute patch target: {relative}")
            original = target.read_bytes()
            current = digest(original)
            if current != required:
                raise SystemExit(
                    f"OmniRoute patch target hash mismatch for {relative}; got {current}"
                )
        patched = apply_file_patch(original, patch_by_path[relative])
        patched_digest = digest(patched)
        if patched_digest != entry["expected_patched_sha256"]:
            raise SystemExit(
                f"patched OmniRoute digest mismatch for {relative}; got {patched_digest}"
            )
        pending[target] = patched

    for target, patched in pending.items():
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(patched)
        print(f"Patched {target.relative_to(args.omniroute_root)} -> {digest(patched)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
