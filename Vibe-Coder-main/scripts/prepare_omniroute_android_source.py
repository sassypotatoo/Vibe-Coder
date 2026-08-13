#!/usr/bin/env python3
"""Admit the exact reviewed OmniRoute 3.8.50 archive and prepare patched source fail-closed.

This script does not build or package a runtime bundle. It establishes the source authority used by
Part 34.3: exact reviewed ZIP bytes, safe archive shape, exact package/runtime metadata, and the
hash-pinned VibeCoder deterministic-routing patch. Generated/native build artifacts in the reviewed
source archive are rejected rather than trusted.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import stat
import subprocess
import sys
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
PROVENANCE = ROOT / "third_party" / "provenance" / "omniroute-3.8.50-reviewed.json"
PATCH_META = ROOT / "third_party" / "patches" / "omniroute-3.8.50-vibecoder-deterministic-routing.json"
PATCHER = ROOT / "scripts" / "apply_omniroute_runtime_patches.py"

MAX_ARCHIVE_BYTES = 80 * 1024 * 1024
MAX_ENTRIES = 20_000
MAX_UNCOMPRESSED_BYTES = 256 * 1024 * 1024
MAX_ENTRY_BYTES = 8 * 1024 * 1024
FORBIDDEN_GENERATED_ROOTS = {"node_modules", ".next", ".build", "dist"}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def archive_entry_is_symlink(info: zipfile.ZipInfo) -> bool:
    mode = (info.external_attr >> 16) & 0xFFFF
    return stat.S_ISLNK(mode)


def validate_member_name(name: str, expected_root: str) -> PurePosixPath:
    if "\x00" in name:
        raise SystemExit("omniroute_archive_nul_path")
    path = PurePosixPath(name)
    if path.is_absolute() or name.startswith(("/", "\\")):
        raise SystemExit(f"omniroute_archive_absolute_path:{name}")
    if ".." in path.parts:
        raise SystemExit(f"omniroute_archive_parent_traversal:{name}")
    if not path.parts or path.parts[0] != expected_root:
        raise SystemExit(f"omniroute_archive_unexpected_root:{name}")
    return path


def inspect_archive(archive: Path, provenance: dict) -> list[zipfile.ZipInfo]:
    if not archive.is_file():
        raise SystemExit("omniroute_reviewed_archive_missing")
    size = archive.stat().st_size
    if size > MAX_ARCHIVE_BYTES or size != provenance["reviewed_archive_size_bytes"]:
        raise SystemExit(f"omniroute_archive_size_mismatch:{size}")
    digest = sha256_file(archive)
    if digest != provenance["reviewed_archive_sha256"]:
        raise SystemExit(f"omniroute_archive_sha256_mismatch:{digest}")

    expected_root = provenance["reviewed_archive_root"]
    seen: set[str] = set()
    total = 0
    with zipfile.ZipFile(archive) as zf:
        infos = zf.infolist()
        if len(infos) > MAX_ENTRIES or len(infos) != provenance["reviewed_archive_entry_count"]:
            raise SystemExit(f"omniroute_archive_entry_count_mismatch:{len(infos)}")
        observed_max_entry = 0
        for info in infos:
            path = validate_member_name(info.filename, expected_root)
            normalized = path.as_posix().rstrip("/")
            if normalized in seen:
                raise SystemExit(f"omniroute_archive_duplicate_path:{normalized}")
            seen.add(normalized)
            if archive_entry_is_symlink(info):
                raise SystemExit(f"omniroute_archive_symlink_forbidden:{info.filename}")
            if info.file_size > MAX_ENTRY_BYTES:
                raise SystemExit(f"omniroute_archive_entry_too_large:{info.filename}")
            observed_max_entry = max(observed_max_entry, info.file_size)
            total += info.file_size
            if total > MAX_UNCOMPRESSED_BYTES:
                raise SystemExit("omniroute_archive_uncompressed_limit")
            if len(path.parts) >= 2 and path.parts[1] in FORBIDDEN_GENERATED_ROOTS:
                raise SystemExit(f"omniroute_reviewed_archive_contains_generated_runtime:{path.parts[1]}")
        if total != provenance["reviewed_archive_uncompressed_bytes"]:
            raise SystemExit(f"omniroute_archive_uncompressed_size_mismatch:{total}")
        if observed_max_entry != provenance["reviewed_archive_max_entry_bytes"]:
            raise SystemExit(f"omniroute_archive_max_entry_size_mismatch:{observed_max_entry}")
        return infos


def validate_output_directory(output: Path, archive: Path) -> Path:
    expanded = output.expanduser()
    if expanded.is_symlink():
        raise SystemExit("omniroute_output_directory_symlink_forbidden")
    resolved = expanded.resolve()
    archive_resolved = archive.expanduser().resolve()
    protected = {Path("/").resolve(), ROOT.resolve(), ROOT.parent.resolve(), archive_resolved.parent}
    if resolved in protected:
        raise SystemExit("omniroute_output_directory_protected")
    if resolved == archive_resolved or resolved in archive_resolved.parents:
        raise SystemExit("omniroute_output_directory_conflicts_with_archive")
    return resolved


def extract_archive(archive: Path, output: Path, provenance: dict) -> Path:
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True, mode=0o700)
    with zipfile.ZipFile(archive) as zf:
        zf.extractall(output)
    source = output / provenance["reviewed_archive_root"]
    if not source.is_dir():
        raise SystemExit("omniroute_extracted_root_missing")
    return source


def verify_source_metadata(source: Path, provenance: dict) -> None:
    package = json.loads((source / "package.json").read_text(encoding="utf-8"))
    if package.get("name") != provenance["package_name"]:
        raise SystemExit("omniroute_package_name_mismatch")
    if package.get("version") != provenance["package_version"]:
        raise SystemExit("omniroute_package_version_mismatch")
    if package.get("engines", {}).get("node") != provenance["node_engine"]:
        raise SystemExit("omniroute_node_engine_mismatch")
    if (source / ".node-version").read_text(encoding="utf-8").strip() != provenance["node_version_file"]:
        raise SystemExit("omniroute_node_version_file_mismatch")
    if (source / ".nvmrc").read_text(encoding="utf-8").strip() != provenance["nvmrc"]:
        raise SystemExit("omniroute_nvmrc_mismatch")
    lock = json.loads((source / "package-lock.json").read_text(encoding="utf-8"))
    if lock.get("lockfileVersion") != provenance["package_lock_version"]:
        raise SystemExit("omniroute_package_lock_version_mismatch")


def apply_and_verify_patch(source: Path, provenance: dict) -> None:
    result = subprocess.run(
        [sys.executable, str(PATCHER), str(source)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        raise SystemExit(f"omniroute_patch_apply_failed:{result.returncode}")
    patch_meta = json.loads(PATCH_META.read_text(encoding="utf-8"))
    if patch_meta.get("upstream_archive_sha256") != provenance["reviewed_archive_sha256"]:
        raise SystemExit("omniroute_patch_archive_authority_mismatch")
    profile = patch_meta.get("profile", {})
    if profile.get("profile_id") != provenance["vibecoder_patch_profile_id"]:
        raise SystemExit("omniroute_patch_profile_id_mismatch")
    if profile.get("profile_sha256") != provenance["vibecoder_patch_profile_sha256"]:
        raise SystemExit("omniroute_patch_profile_sha256_mismatch")
    for entry in patch_meta.get("files", []):
        target = source / entry["target_path"]
        if not target.is_file():
            raise SystemExit(f"omniroute_patched_target_missing:{entry['target_path']}")
        digest = sha256_file(target)
        if digest != entry["expected_patched_sha256"]:
            raise SystemExit(f"omniroute_patched_target_hash_mismatch:{entry['target_path']}:{digest}")


def write_evidence(archive: Path, source: Path, output: Path, provenance: dict) -> None:
    patch_meta = json.loads(PATCH_META.read_text(encoding="utf-8"))
    evidence = {
        "schema": 1,
        "status": "reviewed_source_prepared",
        "archive_name": archive.name,
        "archive_sha256": provenance["reviewed_archive_sha256"],
        "archive_size_bytes": provenance["reviewed_archive_size_bytes"],
        "version": provenance["version"],
        "node_engine": provenance["node_engine"],
        "patch_profile_id": patch_meta["profile"]["profile_id"],
        "patch_profile_sha256": patch_meta["profile"]["profile_sha256"],
        "patched_targets": [
            {
                "path": entry["target_path"],
                "sha256": sha256_file(source / entry["target_path"]),
            }
            for entry in patch_meta["files"]
        ],
        "runtime_bundle_built": False,
        "android_native_dependency_resolution_proven": False,
        "apk_asset_packaged": False,
        "service_round_trip_proven": False,
    }
    output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()

    provenance = json.loads(PROVENANCE.read_text(encoding="utf-8"))
    output_dir = validate_output_directory(args.output_dir, args.archive)
    inspect_archive(args.archive, provenance)
    source = extract_archive(args.archive, output_dir, provenance)
    verify_source_metadata(source, provenance)
    apply_and_verify_patch(source, provenance)
    evidence = args.evidence or (output_dir / "VIBECODER_OMNIROUTE_SOURCE_EVIDENCE.json")
    write_evidence(args.archive, source, evidence, provenance)
    print(f"OmniRoute reviewed source prepared: {source}")
    print(f"Evidence: {evidence}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
