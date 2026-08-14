#!/usr/bin/env python3
"""Stage NDK cpufeatures into Node and patch zlib GYP with relative Android sources.

Node 24.19.0's vendored zlib calls android_getCpuFeatures() on Android. Feeding the
NDK's absolute cpu-features.c path directly to GYP makes the Make generator create
an impossible object dependency such as obj.target/zlib//usr/local/.../cpu-features.o.
VibeCoder therefore copies the exact NDK source/header into the temporary reviewed
Node build tree and references only that relative staged directory from zlib.gyp.
"""
from __future__ import annotations

import argparse
import hashlib
import shutil
from pathlib import Path

PATCH_ID = "vibecoder-node-24.19.0-android-zlib-cpufeatures-v2"
EXPECTED_RELATIVE = Path("deps/zlib/zlib.gyp")
STAGED_RELATIVE = Path("deps/zlib/vibecoder-android-cpufeatures")
SOURCE_NAME = "cpu-features.c"
HEADER_NAME = "cpu-features.h"
INSERT = '''            ['OS=="android"', {
              # Node's zlib cpu_features.c calls android_getCpuFeatures().
              # Stage the exact NDK cpufeatures implementation inside the temporary
              # Node tree so GYP emits a normal relative object path.
              'include_dirs': [
                '<(ZLIB_ROOT)/vibecoder-android-cpufeatures',
              ],
              'sources': [
                '<(ZLIB_ROOT)/vibecoder-android-cpufeatures/cpu-features.c',
              ],
            }],
'''
MARKER = '''            # Incorporate optimizations where possible.
'''


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require_regular(path: Path, code: str) -> None:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(code)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("node_root", type=Path)
    ap.add_argument("ndk_cpufeatures_dir", type=Path)
    args = ap.parse_args()

    root = args.node_root.resolve()
    ndk_dir = args.ndk_cpufeatures_dir.resolve()
    target = root / EXPECTED_RELATIVE
    source = ndk_dir / SOURCE_NAME
    header = ndk_dir / HEADER_NAME
    require_regular(target, "node_android_cpufeatures_patch_target_missing")
    require_regular(source, "node_android_cpufeatures_stage_source_missing")
    require_regular(header, "node_android_cpufeatures_stage_header_missing")

    text = target.read_text(encoding="utf-8")
    if PATCH_ID in text or "vibecoder-android-cpufeatures/cpu-features.c" in text:
        raise SystemExit("node_android_cpufeatures_patch_already_applied")
    if text.count(MARKER) != 1:
        raise SystemExit("node_android_cpufeatures_patch_anchor_mismatch")
    if "'target_name': 'zlib'" not in text or "ARMV8_OS_ANDROID" not in text:
        raise SystemExit("node_android_cpufeatures_patch_upstream_contract_mismatch")

    staged = root / STAGED_RELATIVE
    if staged.exists():
        raise SystemExit("node_android_cpufeatures_stage_destination_exists")
    staged.mkdir(parents=False, exist_ok=False)
    staged_source = staged / SOURCE_NAME
    staged_header = staged / HEADER_NAME
    shutil.copyfile(source, staged_source)
    shutil.copyfile(header, staged_header)
    require_regular(staged_source, "node_android_cpufeatures_staged_source_invalid")
    require_regular(staged_header, "node_android_cpufeatures_staged_header_invalid")
    if sha256(staged_source) != sha256(source) or sha256(staged_header) != sha256(header):
        raise SystemExit("node_android_cpufeatures_stage_hash_mismatch")

    before = sha256(target)
    patched = text.replace(MARKER, f"            # {PATCH_ID}\n" + INSERT + MARKER, 1)
    target.write_text(patched, encoding="utf-8")
    after = sha256(target)
    print(
        "node_android_cpufeatures_patch=APPLIED "
        f"id={PATCH_ID} before_sha256={before} after_sha256={after} "
        f"source_sha256={sha256(staged_source)} header_sha256={sha256(staged_header)} "
        f"staged_relative={STAGED_RELATIVE.as_posix()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
