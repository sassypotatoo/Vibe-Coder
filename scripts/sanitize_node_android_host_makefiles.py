#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f"sanitize_node_android_host_makefiles: {message}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("out_dir")
    parser.add_argument("ndk_root")
    args = parser.parse_args()

    out_dir = Path(args.out_dir).resolve()
    ndk_root = Path(args.ndk_root).resolve()
    if not out_dir.is_dir():
        fail("out_dir_missing")
    if not ndk_root.is_dir():
        fail("ndk_root_missing")

    host_makefiles = sorted(out_dir.rglob("*.host.mk"))
    if not host_makefiles:
        fail("host_makefiles_missing")

    branch_re = re.compile(r"(?<!\S)(?:['\"])?-mbranch-protection=[^\s'\"\\]+(?:['\"])?(?=\s|\\|$)")
    branch_removed = 0
    changed_files = 0

    for path in host_makefiles:
        original = path.read_text(encoding="utf-8", errors="strict")
        text, count = branch_re.subn("", original)
        branch_removed += count
        if text != original:
            path.write_text(text, encoding="utf-8")
            changed_files += 1

    # Fail closed if a target-only flag remains in any host recipe after sanitization.
    residual: list[str] = []
    for path in host_makefiles:
        text = path.read_text(encoding="utf-8", errors="strict")
        if re.search(r"(?<!\S)-mbranch-protection=", text):
            residual.append(f"branch_protection:{path}")
    if residual:
        fail("host_target_flag_residual:" + ",".join(residual[:8]))

    print(json.dumps({
        "node_android_host_makefile_sanitize": "VERIFIED",
        "host_makefiles_scanned": len(host_makefiles),
        "host_makefiles_changed": changed_files,
        "branch_protection_tokens_removed": branch_removed,
    }, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
