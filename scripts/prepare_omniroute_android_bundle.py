#!/usr/bin/env python3
"""Prune and seal a built OmniRoute standalone tree for VibeCoder Android.

This is deliberately a post-build step. The reviewed OmniRoute source can be built on a
supported desktop CI host, but desktop native addons MUST NOT cross the Android trust boundary.
"""
from __future__ import annotations

import argparse
import errno
import hashlib
import json
import os
import shutil
import stat
import sys
import tempfile
import time
from pathlib import Path, PurePosixPath

from omniroute_android_packaging_metadata_policy import scan_gradle_default_excluded_metadata

ROOT = Path(__file__).resolve().parents[1]
PROFILE_PATH = ROOT / "config" / "omniroute-android-runtime-profile.json"
MANIFEST_NAME = ".vibecoder-omniroute-bundle.json"
OMNIROUTE_REPO_OUTPUT = ROOT / "android" / "app" / "build" / "generated" / "omnirouteBundle"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def clone_or_copy_file(src: str, dst: str, *, follow_symlinks: bool = True) -> str:
    source = Path(src)
    # copytree(symlinks=False) promises materialized symlinks. Preserve that contract
    # instead of hard-linking the symlink inode itself.
    if source.is_symlink():
        return shutil.copy2(src, dst, follow_symlinks=True)
    try:
        os.link(src, dst, follow_symlinks=follow_symlinks)
        return dst
    except OSError as exc:
        if exc.errno not in {errno.EXDEV, errno.EPERM, errno.EACCES, errno.EMLINK, errno.EINVAL}:
            raise
        return shutil.copy2(src, dst, follow_symlinks=follow_symlinks)


def phase_done(label: str, started: float) -> None:
    print(f"[omniroute-seal] DONE {label} elapsed={time.monotonic() - started:.1f}s", flush=True)


def load_profile() -> dict:
    data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
    if data.get("schema") != 1:
        raise SystemExit("omniroute_android_profile_schema_mismatch")
    if data.get("profile_id") != "vibecoder-omniroute-android-backend-v1":
        raise SystemExit("omniroute_android_profile_id_mismatch")
    return data


def safe_output(output: Path, source: Path) -> Path:
    # Keep repository source/config immutable while admitting the one generated
    # destination this producer is explicitly responsible for.  The Alpha and
    # Play lanes both seal to android/app/build/generated/omnirouteBundle.
    # Anything else inside the repository remains fail-closed.
    expanded = output.expanduser()
    if expanded.is_symlink():
        raise SystemExit("omniroute_bundle_output_symlink_forbidden")

    lexical = expanded.absolute()
    repo_lexical = ROOT.absolute()
    admitted_lexical = OMNIROUTE_REPO_OUTPUT.absolute()

    if lexical == repo_lexical or repo_lexical in lexical.parents:
        if lexical != admitted_lexical:
            raise SystemExit("omniroute_bundle_output_protected")

        # Do not let an existing parent symlink redirect the admitted generated
        # destination outside android/app/build/generated.  Check lexical path
        # components before resolve(), otherwise the redirect would be hidden.
        cursor = repo_lexical
        for part in lexical.relative_to(repo_lexical).parts[:-1]:
            cursor = cursor / part
            if cursor.is_symlink():
                raise SystemExit("omniroute_bundle_output_parent_symlink_forbidden")

    resolved = lexical.resolve(strict=False)
    source_resolved = source.resolve()
    if resolved in {Path("/").resolve(), ROOT.resolve(), source_resolved}:
        raise SystemExit("omniroute_bundle_output_protected")

    # For the repository-local admitted destination, resolution must still land
    # on that exact path after the parent-symlink check above.
    if lexical == admitted_lexical and resolved != admitted_lexical.resolve(strict=False):
        raise SystemExit("omniroute_bundle_output_parent_symlink_forbidden")

    if resolved in source_resolved.parents or source_resolved in resolved.parents:
        raise SystemExit("omniroute_bundle_output_conflicts_with_source")
    return resolved


def validate_source_symlinks(source: Path, allowed_root: Path | None) -> None:
    allowed = [source.resolve()]
    if allowed_root is not None:
        allowed.append(allowed_root.resolve())
    for path in source.rglob("*"):
        if not path.is_symlink():
            continue
        try:
            target = path.resolve(strict=True)
        except OSError:
            raise SystemExit(f"omniroute_bundle_dangling_symlink_forbidden:{path.relative_to(source).as_posix()}")
        if not any(target == root or root in target.parents for root in allowed):
            raise SystemExit(f"omniroute_bundle_external_symlink_forbidden:{path.relative_to(source).as_posix()}")


def iter_node_modules(root: Path):
    for current, dirs, _files in os.walk(root):
        dirs[:] = [d for d in dirs if d != ".git"]
        if Path(current).name == "node_modules":
            yield Path(current)


def package_relatives(node_modules: Path):
    for child in sorted(node_modules.iterdir(), key=lambda p: p.name):
        if child.name.startswith("@") and child.is_dir():
            for scoped in sorted(child.iterdir(), key=lambda p: p.name):
                yield f"{child.name}/{scoped.name}", scoped
        else:
            yield child.name, child


def is_forbidden_package_identity(rel: str, exact: set[str], prefixes: tuple[str, ...]) -> bool:
    if rel in exact or rel.startswith(prefixes):
        return True
    # Next.js standalone tracing may co-locate duplicate package roots under a
    # deterministic content-hash suffix, e.g. better-sqlite3-90e2652d1716b047.
    # Treat only hexadecimal trace suffixes as aliases of an exact forbidden
    # package. This avoids broad prefix matching such as better-sqlite3-helper.
    for package_root in exact:
        marker = package_root + "-"
        if not rel.startswith(marker):
            continue
        suffix = rel[len(marker):]
        if 8 <= len(suffix) <= 64 and all(ch in "0123456789abcdef" for ch in suffix):
            return True
    return False


def prune_packages(root: Path, profile: dict) -> list[str]:
    exact = set(profile["forbidden_package_roots"])
    prefixes = tuple(profile["forbidden_package_prefixes"])
    removed: list[str] = []

    # Process nested node_modules before their ancestors. A forbidden package can
    # legitimately contain its own node_modules (for example sharp/node_modules).
    # If an ancestor package is removed first, a pre-collected nested path becomes
    # stale and pathlib.iterdir() raises FileNotFoundError. Deepest-first pruning
    # keeps the traversal deterministic while preserving the fail-closed package
    # policy. The existence guard is defensive against an already-removed subtree.
    node_module_roots = sorted(
        iter_node_modules(root),
        key=lambda path: len(path.relative_to(root).parts),
        reverse=True,
    )
    for nm in node_module_roots:
        if not nm.is_dir():
            continue
        for rel, path in list(package_relatives(nm)):
            if is_forbidden_package_identity(rel, exact, prefixes):
                if path.exists() or path.is_symlink():
                    shutil.rmtree(path, ignore_errors=False) if path.is_dir() and not path.is_symlink() else path.unlink()
                    removed.append(path.relative_to(root).as_posix())
        # Remove now-empty scopes. The parent package may have disappeared while
        # pruning a deeper forbidden subtree, so re-check before enumerating it.
        if not nm.is_dir():
            continue
        for child in list(nm.iterdir()):
            if child.name.startswith("@") and child.is_dir() and not any(child.iterdir()):
                child.rmdir()
    return sorted(removed)


def prune_relative_roots(root: Path, profile: dict) -> list[str]:
    removed: list[str] = []
    for rel in profile["forbidden_relative_roots"]:
        target = root.joinpath(*PurePosixPath(rel).parts)
        if target.exists() or target.is_symlink():
            if target.is_dir() and not target.is_symlink():
                shutil.rmtree(target)
            else:
                target.unlink()
            removed.append(rel)
    return removed


def prune_gradle_default_excluded_metadata(root: Path) -> list[str]:
    """Remove runtime-inert SCM/editor metadata before the manifest tree is hashed.

    Gradle's generated-assets FileTree applies default excludes before AAPT. If these files
    survive sealing, the APK silently contains fewer files than the independently verified
    manifest. Removing the whole default-excluded metadata class here keeps the seal and APK
    contracts identical without weakening packaging of legitimate `.next` / `_not-found` paths.
    """
    removed: list[str] = []
    for target in scan_gradle_default_excluded_metadata(root):
        rel = target.relative_to(root).as_posix()
        if target.is_dir() and not target.is_symlink():
            shutil.rmtree(target)
        elif target.exists() or target.is_symlink():
            target.unlink()
        removed.append(rel)
    return sorted(removed)


def is_host_native_binary(path: Path, forbidden_ext: set[str]) -> bool:
    if path.suffix.lower() in forbidden_ext:
        return True
    try:
        with path.open("rb") as f:
            head = f.read(8)
    except OSError:
        return False
    if head.startswith(b"\x7fELF") or head.startswith(b"MZ"):
        return True
    # 32/64-bit Mach-O in either endianness plus universal/fat binary.
    if head[:4] in {
        b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
    }:
        return True
    return False


def validate_tree(root: Path, profile: dict) -> tuple[list[dict], int]:
    for required in profile["required_paths"]:
        if not root.joinpath(*PurePosixPath(required).parts).exists():
            raise SystemExit(f"omniroute_android_bundle_required_path_missing:{required}")

    package = json.loads((root / "package.json").read_text(encoding="utf-8"))
    if package.get("name") != "omniroute" or package.get("version") != "3.8.50":
        raise SystemExit("omniroute_android_bundle_package_identity_mismatch")

    exact = set(profile["forbidden_package_roots"])
    prefixes = tuple(profile["forbidden_package_prefixes"])
    for nm in iter_node_modules(root):
        for rel, _path in package_relatives(nm):
            if is_forbidden_package_identity(rel, exact, prefixes):
                raise SystemExit(f"omniroute_android_bundle_forbidden_package:{rel}")

    forbidden_ext = {x.lower() for x in profile["forbidden_native_extensions"]}
    files: list[dict] = []
    total_bytes = 0
    for path in sorted(root.rglob("*"), key=lambda p: p.as_posix()):
        if path.name == MANIFEST_NAME:
            continue
        if path.is_symlink():
            raise SystemExit(f"omniroute_android_bundle_symlink_forbidden:{path.relative_to(root).as_posix()}")
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if is_host_native_binary(path, forbidden_ext):
            raise SystemExit(f"omniroute_android_bundle_host_native_binary_forbidden:{rel}")
        size = path.stat().st_size
        total_bytes += size
        files.append({"path": rel, "size": size, "sha256": sha256_file(path)})
    if not files:
        raise SystemExit("omniroute_android_bundle_empty")
    return files, total_bytes


def tree_digest(files: list[dict]) -> str:
    h = hashlib.sha256()
    for item in files:
        h.update(item["path"].encode("utf-8"))
        h.update(b"\0")
        h.update(str(item["size"]).encode("ascii"))
        h.update(b"\0")
        h.update(item["sha256"].encode("ascii"))
        h.update(b"\n")
    return h.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("standalone_root", type=Path)
    parser.add_argument("output_root", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--allowed-symlink-root", type=Path)
    args = parser.parse_args()

    source = args.standalone_root.resolve()
    if not source.is_dir() or source.is_symlink():
        raise SystemExit("omniroute_standalone_root_invalid")
    profile = load_profile()
    started = time.monotonic()
    print("[omniroute-seal] START source-symlink-validation", flush=True)
    validate_source_symlinks(source, args.allowed_symlink_root)
    phase_done("source-symlink-validation", started)
    output = safe_output(args.output_root, source)
    output.parent.mkdir(parents=True, exist_ok=True)

    temp = Path(tempfile.mkdtemp(prefix=f".{output.name}.tmp-", dir=output.parent))
    try:
        shutil.rmtree(temp)
        # Dereference symlinks while cloning. On the common same-filesystem CI path,
        # hard links avoid copying immutable build bytes. Cross-device/unsupported files
        # fall back to copy2 without weakening the standalone isolation boundary.
        started = time.monotonic()
        print("[omniroute-seal] START tree-clone", flush=True)
        shutil.copytree(source, temp, symlinks=False, copy_function=clone_or_copy_file)
        phase_done("tree-clone", started)
        started = time.monotonic()
        print("[omniroute-seal] START forbidden-prune", flush=True)
        removed_packages = prune_packages(temp, profile)
        removed_roots = prune_relative_roots(temp, profile)
        removed_gradle_metadata = prune_gradle_default_excluded_metadata(temp)
        phase_done("forbidden-prune", started)
        started = time.monotonic()
        print("[omniroute-seal] START full-tree-hash", flush=True)
        files, total_bytes = validate_tree(temp, profile)
        phase_done("full-tree-hash", started)
        manifest = {
            "schema": 1,
            "component_id": "omniroute",
            "version": "3.8.50",
            "profile_id": profile["profile_id"],
            "source_archive_sha256": profile["source"]["reviewed_archive_sha256"],
            "routing_patch_profile_sha256": profile["source"]["routing_patch_profile_sha256"],
            "required_node_version": profile["build"]["required_node_version"],
            "runtime": profile["runtime"],
            "feature_degradations": profile["feature_degradations"],
            "removed_package_paths": removed_packages,
            "removed_relative_roots": removed_roots,
            "removed_gradle_default_excluded_metadata_paths": removed_gradle_metadata,
            "file_count": len(files),
            "total_bytes": total_bytes,
            "tree_sha256": tree_digest(files),
            "files": files,
            "apk_asset_packaged": False,
            "service_round_trip_proven": False,
        }
        (temp / MANIFEST_NAME).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        if output.exists():
            if output.is_symlink():
                raise SystemExit("omniroute_bundle_existing_output_symlink_forbidden")
            shutil.rmtree(output)
        temp.rename(output)
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"OmniRoute Android runtime bundle sealed: {output}")
        print(f"files={len(files)} bytes={total_bytes} tree_sha256={manifest['tree_sha256']}")
        return 0
    finally:
        if temp.exists():
            shutil.rmtree(temp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
