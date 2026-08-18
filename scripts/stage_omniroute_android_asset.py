#!/usr/bin/env python3
"""Stage a sealed OmniRoute runtime bundle as a generated Android APK asset.

Normal mode independently verifies and copies the bundle. CI fast-path mode consumes a bundle that
was independently verified immediately beforehand by verify_omniroute_android_bundle.py. The
verification stamp is bound to the exact manifest and bundle path, then the directory is atomically
moved on the same filesystem. This removes two redundant full-tree hash passes plus one full copy
without weakening the producer/verifier separation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERIFY = ROOT / "scripts" / "verify_omniroute_android_bundle.py"
MANIFEST_NAME = ".vibecoder-omniroute-bundle.json"
DEFAULT_ASSETS_ROOT = ROOT / "android" / "app" / "build" / "generated" / "omnirouteAssets"
DEFAULT_EVIDENCE = ROOT / "android" / "app" / "build" / "outputs" / "vibecoder-part34-omniroute-asset-staging.json"
ASSET_RELATIVE_ROOT = Path("omniroute") / "bundle"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def is_within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def safe_assets_root(requested: Path, bundle_root: Path) -> Path:
    if requested.exists() and requested.is_symlink():
        raise SystemExit("omniroute_asset_stage_output_symlink_forbidden")
    resolved = requested.resolve(strict=False)
    source = bundle_root.resolve()
    if resolved == Path(resolved.anchor):
        raise SystemExit("omniroute_asset_stage_output_root_forbidden")
    if resolved == source or is_within(resolved, source) or is_within(source, resolved):
        raise SystemExit("omniroute_asset_stage_output_conflicts_with_bundle")
    tracked_android = (ROOT / "android" / "app" / "src").resolve()
    if resolved == tracked_android or is_within(resolved, tracked_android):
        raise SystemExit("omniroute_asset_stage_tracked_source_output_forbidden")
    return resolved


def verify_bundle(path: Path) -> None:
    result = subprocess.run(
        [sys.executable, str(VERIFY), str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        raise SystemExit("omniroute_asset_stage_bundle_verification_failed")


def load_verified_stamp(bundle: Path, manifest: dict, stamp_path: Path) -> dict:
    if not stamp_path.is_file() or stamp_path.is_symlink():
        raise SystemExit("omniroute_asset_stage_verification_stamp_invalid")
    try:
        stamp = json.loads(stamp_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        raise SystemExit("omniroute_asset_stage_verification_stamp_invalid")
    expected = {
        "schema": 1,
        "component_id": "omniroute",
        "version": "3.8.50",
        "profile_id": manifest.get("profile_id"),
        "bundle_root": str(bundle.resolve()),
        "manifest_sha256": sha256_file(bundle / MANIFEST_NAME),
        "tree_sha256": manifest.get("tree_sha256"),
        "file_count": manifest.get("file_count"),
        "total_bytes": manifest.get("total_bytes"),
        "independent_full_tree_verification": True,
    }
    for key, value in expected.items():
        if stamp.get(key) != value:
            raise SystemExit(f"omniroute_asset_stage_verification_stamp_mismatch:{key}")
    return stamp


def copy_tree_verified(source: Path, destination: Path) -> None:
    shutil.copytree(source, destination, symlinks=False)
    verify_bundle(destination)


def atomic_replace_directory(stage: Path, target: Path) -> None:
    backup = target.with_name(target.name + ".previous")
    if backup.exists() or backup.is_symlink():
        if backup.is_symlink() or backup.is_file():
            backup.unlink()
        else:
            shutil.rmtree(backup)
    if target.exists() or target.is_symlink():
        if target.is_symlink() or not target.is_dir():
            raise SystemExit("omniroute_asset_stage_existing_target_invalid")
        os.replace(target, backup)
    try:
        os.replace(stage, target)
    except BaseException:
        if not target.exists() and backup.exists():
            os.replace(backup, target)
        raise
    if backup.exists():
        shutil.rmtree(backup)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle_root", type=Path)
    parser.add_argument("--assets-root", type=Path, default=DEFAULT_ASSETS_ROOT)
    parser.add_argument("--evidence", type=Path, default=DEFAULT_EVIDENCE)
    parser.add_argument("--verification-stamp", type=Path)
    parser.add_argument("--consume-verified-bundle", action="store_true")
    args = parser.parse_args()

    if args.consume_verified_bundle != (args.verification_stamp is not None):
        raise SystemExit("omniroute_asset_stage_fast_path_arguments_incomplete")

    bundle = args.bundle_root.resolve()
    if not bundle.is_dir() or bundle.is_symlink():
        raise SystemExit("omniroute_asset_stage_bundle_invalid")

    manifest_path = bundle / MANIFEST_NAME
    if not manifest_path.is_file():
        raise SystemExit("omniroute_asset_stage_bundle_manifest_missing")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("apk_asset_packaged") is not False or manifest.get("service_round_trip_proven") is not False:
        raise SystemExit("omniroute_asset_stage_input_manifest_overclaims_proof")

    verification_stamp = None
    if args.consume_verified_bundle:
        verification_stamp = load_verified_stamp(bundle, manifest, args.verification_stamp.resolve())
    else:
        verify_bundle(bundle)

    assets_root = safe_assets_root(args.assets_root, bundle)
    target = assets_root / ASSET_RELATIVE_ROOT
    assets_root.mkdir(parents=True, exist_ok=True)
    if assets_root.is_symlink():
        raise SystemExit("omniroute_asset_stage_assets_root_symlink_forbidden")

    temp_parent = target.parent
    temp_parent.mkdir(parents=True, exist_ok=True)
    consumed_verified_bundle = False
    if args.consume_verified_bundle:
        if bundle.stat().st_dev != temp_parent.stat().st_dev:
            raise SystemExit("omniroute_asset_stage_consume_cross_device")
        atomic_replace_directory(bundle, target)
        consumed_verified_bundle = True
    else:
        stage = Path(tempfile.mkdtemp(prefix=".bundle.stage-", dir=temp_parent))
        try:
            shutil.rmtree(stage)
            copy_tree_verified(bundle, stage)
            atomic_replace_directory(stage, target)
        finally:
            if stage.exists():
                shutil.rmtree(stage, ignore_errors=True)

    staged_manifest = target / MANIFEST_NAME
    evidence = {
        "schema": 1,
        "part": 34,
        "step": "34.3.3",
        "component_id": "omniroute",
        "version": "3.8.50",
        "profile_id": manifest["profile_id"],
        "asset_relative_root": ASSET_RELATIVE_ROOT.as_posix(),
        "bundle_manifest_sha256": sha256_file(staged_manifest),
        "tree_sha256": manifest["tree_sha256"],
        "file_count": manifest["file_count"],
        "total_bytes": manifest["total_bytes"],
        "apk_asset_staged": True,
        "apk_asset_packaging_proven": False,
        "device_extraction_proven": False,
        "service_round_trip_proven": False,
        "consumed_independently_verified_bundle": consumed_verified_bundle,
        "verification_stamp_sha256": (
            sha256_file(args.verification_stamp.resolve()) if verification_stamp is not None else None
        ),
    }
    evidence_path = args.evidence.resolve(strict=False)
    if evidence_path.exists() and evidence_path.is_symlink():
        raise SystemExit("omniroute_asset_stage_evidence_symlink_forbidden")
    evidence_path.parent.mkdir(parents=True, exist_ok=True)
    temp_evidence = evidence_path.with_name(evidence_path.name + ".tmp")
    temp_evidence.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temp_evidence, evidence_path)

    print("OmniRoute Android APK asset staging PASSED")
    print(f"asset={target}")
    print(f"tree_sha256={manifest['tree_sha256']}")
    if consumed_verified_bundle:
        print("asset_stage_mode=atomic-consume-independently-verified-bundle")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
