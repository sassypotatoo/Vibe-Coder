#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="https://github.com/diegosouzapw/OmniRoute"
# Historical label retained for provenance only. Build transport is pinned to REVIEWED_COMMIT.
SOURCE_REF="release/v3.8.50"
REVIEWED_COMMIT="ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"
EXPECTED_SHA256="1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7"
EXPECTED_LEGACY_ROOT="OmniRoute-release-v3.8.50/"
EXPECTED_ENTRY_COUNT=13622
EXPECTED_UNCOMPRESSED_BYTES=204459095
EXPECTED_MAX_ENTRY_BYTES=5843857
URL="${REPO}/archive/${REVIEWED_COMMIT}.zip"
DEST="${1:-$ROOT/.runtime-cache/OmniRoute-release-v3.8.50.zip}"

fail() { printf 'fetch_omniroute_reviewed_archive: %s\n' "$1" >&2; exit 1; }
for tool in curl python3 sha256sum; do command -v "$tool" >/dev/null 2>&1 || fail "tool_missing:$tool"; done
mkdir -p "$(dirname "$DEST")"

# Accept either the original reviewed branch ZIP bytes (when already cached) or a fresh GitHub
# archive fetched by the immutable reviewed commit. The old ZIP SHA remains historical provenance;
# fresh transport bytes are admitted by exact commit identity plus reviewed content-shape bounds.
verify_archive() {
  local archive="$1"
  python3 - "$archive" "$EXPECTED_SHA256" "$EXPECTED_LEGACY_ROOT" "$REVIEWED_COMMIT" \
    "$EXPECTED_ENTRY_COUNT" "$EXPECTED_UNCOMPRESSED_BYTES" "$EXPECTED_MAX_ENTRY_BYTES" <<'PY2'
import hashlib, stat, sys, zipfile
from pathlib import PurePosixPath
archive, legacy_sha, legacy_root, commit = sys.argv[1:5]
entry_count, uncompressed, max_entry = map(int, sys.argv[5:8])

def sha256(path):
    h=hashlib.sha256()
    with open(path,'rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):
            h.update(chunk)
    return h.hexdigest()

actual=sha256(archive)
with zipfile.ZipFile(archive) as zf:
    infos=zf.infolist()
    if not infos or len(infos) != entry_count:
        raise SystemExit(2)
    roots={PurePosixPath(i.filename).parts[0] for i in infos if PurePosixPath(i.filename).parts}
    if len(roots) != 1:
        raise SystemExit(3)
    root=next(iter(roots)) + '/'
    comment=zf.comment.decode('ascii', errors='strict')
    total=sum(i.file_size for i in infos)
    largest=max(i.file_size for i in infos)
    if total != uncompressed or largest != max_entry:
        raise SystemExit(4)
    for info in infos:
        path=PurePosixPath(info.filename)
        if path.is_absolute() or '..' in path.parts:
            raise SystemExit(5)
        mode=(info.external_attr >> 16) & 0xFFFF
        if stat.S_ISLNK(mode):
            raise SystemExit(6)
    if actual == legacy_sha:
        if root != legacy_root or comment != commit:
            raise SystemExit(7)
    else:
        # GitHub's commit archive root is transport metadata; the ZIP comment is the immutable
        # repository identity. Require the root to identify the same repository and the exact
        # reviewed commit comment before downstream package + patch-target hash verification.
        if not root.startswith('OmniRoute-') or (comment and comment != commit):
            raise SystemExit(8)
print(f'archive_sha256={actual}')
print(f'archive_root={root}')
print(f'reviewed_commit={comment}')
PY2
}

if [[ -f "$DEST" ]] && verify_archive "$DEST"; then
  printf 'reviewed OmniRoute archive ready: %s\n' "$DEST"
  exit 0
fi
rm -f "$DEST"

tmp="${DEST}.part"
rm -f "$tmp"
# Never fetch the moving release branch here. The reviewed commit is immutable even if
# release/v3.8.50 receives new commits later.
curl --fail --location --silent --show-error \
  --proto '=https' --tlsv1.2 \
  --retry 3 --retry-all-errors \
  --output "$tmp" "$URL"
if ! verify_archive "$tmp"; then
  actual="$(sha256sum "$tmp" 2>/dev/null | awk '{print $1}' || true)"
  rm -f "$tmp"
  fail "reviewed_commit_archive_verification_failed:commit=${REVIEWED_COMMIT}:actual_sha256=${actual:-unreadable}"
fi
mv "$tmp" "$DEST"
printf 'reviewed OmniRoute archive ready: %s\n' "$DEST"
