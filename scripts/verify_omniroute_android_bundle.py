#!/usr/bin/env python3
"""Independently verify a sealed VibeCoder OmniRoute Android bundle."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
PROFILE = json.loads((ROOT / "config" / "omniroute-android-runtime-profile.json").read_text())
MANIFEST = ".vibecoder-omniroute-bundle.json"


def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def native(path: Path) -> bool:
    if path.suffix.lower() in {x.lower() for x in PROFILE["forbidden_native_extensions"]}:
        return True
    with path.open("rb") as f:
        head = f.read(8)
    return head.startswith((b"\x7fELF", b"MZ")) or head[:4] in {
        b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
    }


def iter_node_modules(root: Path):
    for current, dirs, _files in os.walk(root):
        dirs[:] = [d for d in dirs if d != ".git"]
        if Path(current).name == "node_modules":
            yield Path(current)


def package_relatives(node_modules: Path):
    for child in sorted(node_modules.iterdir(), key=lambda p: p.name):
        if child.name.startswith("@") and child.is_dir():
            for scoped in sorted(child.iterdir(), key=lambda p: p.name):
                yield f"{child.name}/{scoped.name}"
        else:
            yield child.name


def is_forbidden_package_identity(rel: str, exact: set[str], prefixes: tuple[str, ...]) -> bool:
    if rel in exact or rel.startswith(prefixes):
        return True
    for package_root in exact:
        marker = package_root + "-"
        if not rel.startswith(marker):
            continue
        suffix = rel[len(marker):]
        if 8 <= len(suffix) <= 64 and all(ch in "0123456789abcdef" for ch in suffix):
            return True
    return False


def tree_digest(files: list[dict]) -> str:
    h = hashlib.sha256()
    for item in files:
        h.update(item["path"].encode()); h.update(b"\0")
        h.update(str(item["size"]).encode()); h.update(b"\0")
        h.update(item["sha256"].encode()); h.update(b"\n")
    return h.hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("bundle_root", type=Path)
    args = ap.parse_args()
    root = args.bundle_root.resolve()
    if not root.is_dir() or root.is_symlink():
        raise SystemExit("omniroute_android_bundle_invalid")
    mp = root / MANIFEST
    if not mp.is_file():
        raise SystemExit("omniroute_android_bundle_manifest_missing")
    manifest = json.loads(mp.read_text())
    if manifest.get("profile_id") != PROFILE["profile_id"]:
        raise SystemExit("omniroute_android_bundle_profile_mismatch")
    if manifest.get("source_archive_sha256") != PROFILE["source"]["reviewed_archive_sha256"]:
        raise SystemExit("omniroute_android_bundle_source_hash_mismatch")
    if manifest.get("required_node_version") != "24.19.0":
        raise SystemExit("omniroute_android_bundle_node_version_mismatch")
    if manifest.get("runtime") != PROFILE["runtime"]:
        raise SystemExit("omniroute_android_bundle_runtime_profile_mismatch")
    if manifest.get("feature_degradations") != PROFILE["feature_degradations"]:
        raise SystemExit("omniroute_android_bundle_feature_degradation_mismatch")
    if manifest.get("apk_asset_packaged") is not False or manifest.get("service_round_trip_proven") is not False:
        raise SystemExit("omniroute_android_bundle_manifest_overclaims_runtime_proof")

    package = json.loads((root / "package.json").read_text(encoding="utf-8"))
    if package.get("name") != "omniroute" or package.get("version") != "3.8.50":
        raise SystemExit("omniroute_android_bundle_package_identity_mismatch")
    exact = set(PROFILE["forbidden_package_roots"])
    prefixes = tuple(PROFILE["forbidden_package_prefixes"])
    for nm in iter_node_modules(root):
        for rel in package_relatives(nm):
            if is_forbidden_package_identity(rel, exact, prefixes):
                raise SystemExit(f"omniroute_android_bundle_forbidden_package:{rel}")

    actual: list[dict] = []
    for path in sorted(root.rglob("*"), key=lambda p: p.as_posix()):
        if path == mp:
            continue
        if path.is_symlink():
            raise SystemExit(f"omniroute_android_bundle_symlink_forbidden:{path.relative_to(root).as_posix()}")
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if native(path):
            raise SystemExit(f"omniroute_android_bundle_host_native_binary_forbidden:{rel}")
        actual.append({"path": rel, "size": path.stat().st_size, "sha256": sha(path)})
    expected = manifest.get("files")
    if actual != expected:
        raise SystemExit("omniroute_android_bundle_file_manifest_mismatch")
    if tree_digest(actual) != manifest.get("tree_sha256"):
        raise SystemExit("omniroute_android_bundle_tree_hash_mismatch")
    if sum(x["size"] for x in actual) != manifest.get("total_bytes"):
        raise SystemExit("omniroute_android_bundle_total_bytes_mismatch")
    if len(actual) != manifest.get("file_count"):
        raise SystemExit("omniroute_android_bundle_file_count_mismatch")
    for req in PROFILE["required_paths"]:
        if not root.joinpath(*PurePosixPath(req).parts).exists():
            raise SystemExit(f"omniroute_android_bundle_required_path_missing:{req}")
    print("OmniRoute Android runtime bundle verification PASSED")
    print(f"files={len(actual)} tree_sha256={manifest['tree_sha256']}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
