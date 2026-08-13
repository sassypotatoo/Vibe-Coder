#!/usr/bin/env python3
"""Patch reviewed Node 24.19.0 zlib GYP for Android NDK cpufeatures linkage.

Node's vendored zlib cpu_features.c includes <cpu-features.h> and calls
android_getCpuFeatures() when ARMV8_OS_ANDROID is enabled. Upstream's Android
configure exposes android_ndk_path to GYP, but zlib.gyp does not add the NDK
cpufeatures implementation to the zlib static library. The result is a late
undefined-symbol failure. This patch adds exactly that implementation and its
include directory, nothing else.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

PATCH_ID = "vibecoder-node-24.19.0-android-zlib-cpufeatures-v1"
EXPECTED_RELATIVE = Path("deps/zlib/zlib.gyp")
INSERT = '''            ['OS=="android"', {\n              # Node's zlib cpu_features.c calls android_getCpuFeatures().\n              # The NDK cpufeatures implementation is source-only/static and\n              # must be included explicitly for Android cross-builds.\n              'include_dirs': [\n                '<(android_ndk_path)/sources/android/cpufeatures',\n              ],\n              'sources': [\n                '<(android_ndk_path)/sources/android/cpufeatures/cpu-features.c',\n              ],\n            }],\n'''
MARKER = '''            # Incorporate optimizations where possible.\n'''


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("node_root", type=Path)
    args = ap.parse_args()
    root = args.node_root.resolve()
    target = root / EXPECTED_RELATIVE
    if not target.is_file() or target.is_symlink():
        raise SystemExit("node_android_cpufeatures_patch_target_missing")
    text = target.read_text(encoding="utf-8")
    if PATCH_ID in text or "android_ndk_path)/sources/android/cpufeatures/cpu-features.c" in text:
        raise SystemExit("node_android_cpufeatures_patch_already_applied")
    if text.count(MARKER) != 1:
        raise SystemExit("node_android_cpufeatures_patch_anchor_mismatch")
    if "'target_name': 'zlib'" not in text or "ARMV8_OS_ANDROID" not in text:
        raise SystemExit("node_android_cpufeatures_patch_upstream_contract_mismatch")
    before = sha256(target)
    patched = text.replace(MARKER, f"            # {PATCH_ID}\n" + INSERT + MARKER, 1)
    target.write_text(patched, encoding="utf-8")
    after = sha256(target)
    print(f"node_android_cpufeatures_patch=APPLIED id={PATCH_ID} before_sha256={before} after_sha256={after}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
