#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path

# This is deliberately narrow. It is the only host-incompatible Android target flag proven by the
# pinned Node 24.19.0 + NDK r28c CI evidence. Do not grow this list without a real compiler failure.
PROVEN_HOST_INCOMPATIBLE_FLAGS = ("-mbranch-protection=standard",)


def exact_flag_pattern(flag: str) -> re.Pattern[str]:
    return re.compile(r"(?<!\S)" + re.escape(flag) + r"(?!\S)")


def fail(message: str) -> None:
    raise SystemExit(f"sanitize_node_android_host_makefiles: {message}")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def regular_files(root: Path, pattern: str) -> list[Path]:
    files: list[Path] = []
    for path in sorted(root.glob(pattern)):
        path.lstat()  # fail immediately if the generated path disappears mid-scan
        if path.is_symlink() or not path.is_file():
            fail(f"generated_makefile_not_regular:{path.name}")
        files.append(path)
    return files


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("out_dir")
    args = parser.parse_args()

    out_dir = Path(args.out_dir)
    if not out_dir.is_absolute():
        out_dir = Path(os.path.abspath(out_dir))
    if not out_dir.is_dir():
        fail("out_dir_missing")

    host_files = regular_files(out_dir, "*.host.mk")
    target_files = regular_files(out_dir, "*.target.mk")
    if not host_files:
        fail("host_makefiles_missing")
    if not target_files:
        fail("target_makefiles_missing")

    # Snapshot every target recipe before touching host recipes. A sanitizer that mutates Android
    # target makefiles could quietly remove security/codegen flags from the binary we intend to ship.
    target_before = {path.name: sha256(path) for path in target_files}
    replacements = 0
    touched: list[str] = []

    for path in host_files:
        raw = path.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            fail(f"host_makefile_non_utf8:{path.name}")
        updated = text
        file_replacements = 0
        for flag in PROVEN_HOST_INCOMPATIBLE_FLAGS:
            updated, count = exact_flag_pattern(flag).subn("", updated)
            file_replacements += count
        if file_replacements:
            # GYP makefiles are generated text. Preserve all other bytes and permissions.
            with path.open("w", encoding="utf-8", newline="") as stream:
                stream.write(updated)
            replacements += file_replacements
            touched.append(path.name)

    if replacements <= 0:
        fail("proven_host_target_flag_not_found")

    # Re-scan rather than trusting the replacement loop.
    for path in host_files:
        text = path.read_text(encoding="utf-8")
        for flag in PROVEN_HOST_INCOMPATIBLE_FLAGS:
            if exact_flag_pattern(flag).search(text):
                fail(f"host_flag_still_present:{path.name}:{flag}")

    target_after = {path.name: sha256(path) for path in target_files}
    if target_before != target_after:
        fail("target_makefiles_modified")

    print(
        json.dumps(
            {
                "node_android_host_makefile_sanitize": "VERIFIED",
                "host_makefiles_scanned": len(host_files),
                "target_makefiles_guarded": len(target_files),
                "host_makefiles_touched": len(touched),
                "flag_replacements": replacements,
                "removed_flags": list(PROVEN_HOST_INCOMPATIBLE_FLAGS),
            },
            separators=(",", ":"),
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
