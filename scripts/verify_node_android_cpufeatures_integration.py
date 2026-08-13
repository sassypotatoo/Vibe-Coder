#!/usr/bin/env python3
"""Verify Node's generated GYP graph contains the Android NDK cpufeatures source.

The source-level zlib.gyp patch is not enough evidence by itself. This verifier runs
after GYP materializes *.target.mk / *.host.mk and before compilation. It proves the
Android zlib target actually contains cpu-features.c and that the Android source did
not leak into any host recipe.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

SOURCE_TOKEN = "sources/android/cpufeatures/cpu-features.c"
INCLUDE_TOKEN = "sources/android/cpufeatures"


def fail(message: str) -> None:
    raise SystemExit(f"verify_node_android_cpufeatures_integration: {message}")


def regular_makefiles(root: Path, suffix: str) -> list[Path]:
    files: list[Path] = []
    for path in sorted(root.rglob(f"*{suffix}")):
        path.lstat()
        if path.is_symlink() or not path.is_file():
            fail(f"generated_makefile_not_regular:{path.relative_to(root).as_posix()}")
        files.append(path)
    return files


def read_utf8(path: Path, root: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        fail(f"generated_makefile_non_utf8:{path.relative_to(root).as_posix()}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out_dir")
    args = ap.parse_args()

    out = Path(args.out_dir)
    if not out.is_absolute():
        out = Path(os.path.abspath(out))
    if not out.is_dir():
        fail("out_dir_missing")

    targets = regular_makefiles(out, ".target.mk")
    hosts = regular_makefiles(out, ".host.mk")
    if not targets:
        fail("target_makefiles_missing")
    if not hosts:
        fail("host_makefiles_missing")

    matches: list[str] = []
    for path in targets:
        text = read_utf8(path, out)
        if SOURCE_TOKEN in text:
            rel = path.relative_to(out).as_posix()
            if "zlib" not in rel.lower() and "zlib" not in text.lower():
                fail(f"cpufeatures_source_in_non_zlib_target:{rel}")
            if INCLUDE_TOKEN not in text:
                fail(f"cpufeatures_include_contract_missing:{rel}")
            matches.append(rel)

    if not matches:
        fail("cpufeatures_source_missing_from_target_graph")
    if len(matches) != 1:
        fail("cpufeatures_source_target_graph_ambiguous:" + ",".join(matches))

    host_leaks: list[str] = []
    for path in hosts:
        text = read_utf8(path, out)
        if SOURCE_TOKEN in text:
            host_leaks.append(path.relative_to(out).as_posix())
    if host_leaks:
        fail("cpufeatures_source_leaked_into_host_graph:" + ",".join(host_leaks))

    print(json.dumps({
        "node_android_cpufeatures_generated_graph": "VERIFIED",
        "target_makefiles_scanned": len(targets),
        "host_makefiles_scanned": len(hosts),
        "zlib_target_makefile": matches[0],
        "source_token": SOURCE_TOKEN,
        "host_leak": False,
    }, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
