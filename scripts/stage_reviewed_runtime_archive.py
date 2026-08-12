#!/usr/bin/env python3
"""Verify a reviewed Jcode/OmniRoute source archive before any Part-28 staging work."""
from __future__ import annotations
import argparse, hashlib, json, pathlib, sys, zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCK = ROOT / "third_party" / "SOURCES.lock.json"
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_MEMBERS = 100_000


def die(code: str) -> "NoReturn":
    print(code, file=sys.stderr)
    raise SystemExit(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("name", choices=("jcode", "OmniRoute"))
    parser.add_argument("archive", type=pathlib.Path)
    args = parser.parse_args()
    data = json.loads(LOCK.read_text(encoding="utf-8"))
    entry = next((x for x in data["sources"] if x["name"] == args.name), None)
    if not entry:
        die("reviewed_source_not_found")
    archive = args.archive.resolve(strict=True)
    if not archive.is_file() or archive.stat().st_size > MAX_ARCHIVE_BYTES:
        die("reviewed_archive_size_invalid")
    hasher = hashlib.sha256()
    with archive.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            hasher.update(chunk)
    digest = hasher.hexdigest()
    if digest != entry["sha256"]:
        die("reviewed_archive_sha256_mismatch")
    if archive.suffix.lower() == ".zip":
        with zipfile.ZipFile(archive) as zf:
            members = zf.infolist()
            if len(members) > MAX_MEMBERS:
                die("reviewed_archive_member_limit")
            for member in members:
                path = pathlib.PurePosixPath(member.filename)
                if path.is_absolute() or ".." in path.parts:
                    die("reviewed_archive_unsafe_path")
    print(json.dumps({
        "name": entry["name"],
        "version": entry["version"],
        "archive": str(archive),
        "sha256": digest,
        "verified": True,
        "note": "verification_does_not_build_or_package_the_runtime",
    }, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
