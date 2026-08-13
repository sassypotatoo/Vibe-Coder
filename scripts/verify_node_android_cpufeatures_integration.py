#!/usr/bin/env python3
"""Verify Node's generated GYP graph contains the Android NDK cpufeatures object.

The source-level zlib.gyp patch is not enough evidence by itself. Node's Make GYP
backend converts compilable source paths to object paths in generated *.target.mk
files, so this verifier checks for cpu-features.o (not cpu-features.c) after GYP
materializes the graph and before compilation. It also proves the object belongs
to the Android zlib target and did not leak into any host recipe.
"""
from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

SOURCE_TOKEN = "sources/android/cpufeatures/cpu-features.c"
OBJECT_TOKEN = "sources/android/cpufeatures/cpu-features.o"
INCLUDE_TOKEN = "sources/android/cpufeatures"
TARGET_RE = re.compile(r"^TARGET\s*:=\s*([^\s#]+)\s*$", re.MULTILINE)


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


def generated_target_name(text: str, rel: str) -> str:
    match = TARGET_RE.search(text)
    if match is None:
        fail(f"generated_target_name_missing:{rel}")
    return match.group(1)


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
        if OBJECT_TOKEN in text:
            rel = path.relative_to(out).as_posix()
            if generated_target_name(text, rel) != "zlib":
                fail(f"cpufeatures_object_in_non_zlib_target:{rel}")
            if INCLUDE_TOKEN not in text:
                fail(f"cpufeatures_include_contract_missing:{rel}")
            matches.append(rel)

    if not matches:
        fail("cpufeatures_object_missing_from_target_graph")
    if len(matches) != 1:
        fail("cpufeatures_object_target_graph_ambiguous:" + ",".join(matches))

    host_leaks: list[str] = []
    for path in hosts:
        text = read_utf8(path, out)
        if OBJECT_TOKEN in text or SOURCE_TOKEN in text:
            host_leaks.append(path.relative_to(out).as_posix())
    if host_leaks:
        fail("cpufeatures_object_leaked_into_host_graph:" + ",".join(host_leaks))

    print(json.dumps({
        "node_android_cpufeatures_generated_graph": "VERIFIED",
        "target_makefiles_scanned": len(targets),
        "host_makefiles_scanned": len(hosts),
        "zlib_target_makefile": matches[0],
        "object_token": OBJECT_TOKEN,
        "source_token": SOURCE_TOKEN,
        "host_leak": False,
    }, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
