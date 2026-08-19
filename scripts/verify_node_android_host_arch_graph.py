#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

EXPECTED_HOST_ARCH = "x64"
EXPECTED_HOST_OBJECT = "deps/v8/src/heap/base/asm/x64/push_registers_asm.o"
ARCH_OBJECT_RE = re.compile(
    r"deps/v8/src/heap/base/asm/(?P<arch>[^/]+)/push_registers_(?:asm|masm)\.o"
)


def fail(message: str) -> None:
    raise SystemExit(f"verify_node_android_host_arch_graph: {message}")


def main() -> int:
    if len(sys.argv) != 2:
        fail("usage: verify_node_android_host_arch_graph.py OUT_DIR")
    out_dir = Path(os.path.abspath(sys.argv[1]))
    if not out_dir.is_dir():
        fail("out_dir_missing")

    host_files = sorted(out_dir.rglob("*.host.mk"))
    if not host_files:
        fail("host_makefiles_missing")

    v8_base = [p for p in host_files if p.name == "v8_base_without_compiler.host.mk"]
    if len(v8_base) != 1:
        fail(f"v8_base_without_compiler_host_makefile_count:{len(v8_base)}")

    path = v8_base[0]
    if path.is_symlink() or not path.is_file():
        fail("v8_base_without_compiler_host_makefile_not_regular")
    text = path.read_text(encoding="utf-8", errors="strict")
    matches = [(m.group("arch"), m.group(0)) for m in ARCH_OBJECT_RE.finditer(text)]
    if not matches:
        fail("host_push_register_object_missing")

    arches = sorted({arch for arch, _ in matches})
    foreign = sorted({arch for arch in arches if arch != EXPECTED_HOST_ARCH})
    if foreign:
        fail("host_push_register_arch_mismatch:" + ",".join(foreign))
    if EXPECTED_HOST_OBJECT not in text:
        fail("expected_x64_host_push_register_object_missing")

    # The exact failure that motivated this guard was x86_64 /usr/bin/g++ being handed the
    # ARM64 push-register source. Reject both object and source spellings so a future GYP format
    # change cannot reintroduce the same two-hour failure invisibly.
    for forbidden in (
        "deps/v8/src/heap/base/asm/arm64/push_registers_asm.o",
        "deps/v8/src/heap/base/asm/arm64/push_registers_asm.cc",
        "deps/v8/src/heap/base/asm/arm64/push_registers_masm.o",
        "deps/v8/src/heap/base/asm/arm64/push_registers_masm.S",
    ):
        if forbidden in text:
            fail("arm64_push_register_leaked_into_host_graph:" + forbidden)

    print(json.dumps({
        "node_android_host_arch_graph": "VERIFIED",
        "host_arch": EXPECTED_HOST_ARCH,
        "host_makefiles_scanned": len(host_files),
        "v8_base_host_makefile": str(path.relative_to(out_dir)),
        "push_register_arches": arches,
        "expected_object": EXPECTED_HOST_OBJECT,
    }, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
