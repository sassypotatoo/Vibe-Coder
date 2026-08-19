#!/usr/bin/env python3
"""Independently verify a sealed VibeCoder OmniRoute Android bundle."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import sys
from pathlib import Path, PurePosixPath

from omniroute_android_packaging_metadata_policy import scan_gradle_default_excluded_metadata

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
    ap.add_argument("--write-verification-stamp", type=Path)
    ap.add_argument("--write-mismatch-report", type=Path)
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
    metadata_conflicts = scan_gradle_default_excluded_metadata(root)
    if metadata_conflicts:
        rel = metadata_conflicts[0].relative_to(root).as_posix()
        raise SystemExit(f"omniroute_android_bundle_gradle_default_excluded_metadata_forbidden:{rel}")
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
    if not isinstance(expected, list):
        raise SystemExit("omniroute_android_bundle_manifest_files_invalid")
    if actual != expected:
        expected_by_path = {item.get("path"): item for item in expected if isinstance(item, dict) and isinstance(item.get("path"), str)}
        actual_by_path = {item["path"]: item for item in actual}
        missing = sorted(set(expected_by_path) - set(actual_by_path))
        unexpected = sorted(set(actual_by_path) - set(expected_by_path))
        changed = []
        for rel in sorted(set(expected_by_path) & set(actual_by_path)):
            before = expected_by_path[rel]
            after = actual_by_path[rel]
            if before.get("size") != after.get("size") or before.get("sha256") != after.get("sha256"):
                changed.append({"path": rel, "expected": before, "actual": after})
        report = {
            "schema": 1,
            "error": "omniroute_android_bundle_file_manifest_mismatch",
            "expected_file_count": len(expected),
            "actual_file_count": len(actual),
            "missing_count": len(missing),
            "unexpected_count": len(unexpected),
            "changed_count": len(changed),
            "order_only_mismatch": not missing and not unexpected and not changed,
            "missing_first_100": missing[:100],
            "unexpected_first_100": unexpected[:100],
            "changed_first_50": changed[:50],
        }
        if args.write_mismatch_report is not None:
            report_path = args.write_mismatch_report.expanduser().resolve(strict=False)
            if report_path == root or root in report_path.parents:
                raise SystemExit("omniroute_android_mismatch_report_inside_bundle_forbidden")
            if report_path.exists() and report_path.is_symlink():
                raise SystemExit("omniroute_android_mismatch_report_symlink_forbidden")
            report_path.parent.mkdir(parents=True, exist_ok=True)
            temp_report = report_path.with_name(report_path.name + ".tmp")
            temp_report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            os.replace(temp_report, report_path)
        print(
            "omniroute_android_bundle_manifest_diff:"
            f"expected={len(expected)}:actual={len(actual)}:"
            f"missing={len(missing)}:unexpected={len(unexpected)}:changed={len(changed)}",
            file=sys.stderr,
        )
        for rel in missing[:20]:
            print(f"omniroute_android_bundle_missing_from_packaged_tree:{rel}", file=sys.stderr)
        for rel in unexpected[:20]:
            print(f"omniroute_android_bundle_unexpected_in_packaged_tree:{rel}", file=sys.stderr)
        for item in changed[:10]:
            print(f"omniroute_android_bundle_changed_in_packaged_tree:{item['path']}", file=sys.stderr)
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

    if args.write_verification_stamp is not None:
        stamp_path = args.write_verification_stamp.expanduser().resolve(strict=False)
        if stamp_path == root or root in stamp_path.parents:
            raise SystemExit("omniroute_android_verification_stamp_inside_bundle_forbidden")
        if stamp_path.exists() and stamp_path.is_symlink():
            raise SystemExit("omniroute_android_verification_stamp_symlink_forbidden")
        stamp = {
            "schema": 1,
            "component_id": "omniroute",
            "version": "3.8.50",
            "profile_id": PROFILE["profile_id"],
            "bundle_root": str(root),
            "manifest_sha256": sha(mp),
            "tree_sha256": manifest["tree_sha256"],
            "file_count": manifest["file_count"],
            "total_bytes": manifest["total_bytes"],
            "independent_full_tree_verification": True,
        }
        stamp_path.parent.mkdir(parents=True, exist_ok=True)
        temp_stamp = stamp_path.with_name(stamp_path.name + ".tmp")
        temp_stamp.write_text(json.dumps(stamp, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        os.replace(temp_stamp, stamp_path)
        print(f"verification_stamp={stamp_path}")

    print("OmniRoute Android runtime bundle verification PASSED")
    print(f"files={len(actual)} tree_sha256={manifest['tree_sha256']}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
