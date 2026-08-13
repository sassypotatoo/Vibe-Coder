#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="https://github.com/diegosouzapw/OmniRoute"
SOURCE_REF="release/v3.8.50"
REVIEWED_COMMIT="ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"
EXPECTED_SHA256="1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7"
EXPECTED_ROOT="OmniRoute-release-v3.8.50/"
URL="${REPO}/archive/refs/heads/${SOURCE_REF}.zip"
DEST="${1:-$ROOT/.runtime-cache/OmniRoute-release-v3.8.50.zip}"

fail() { printf 'fetch_omniroute_reviewed_archive: %s\n' "$1" >&2; exit 1; }
for tool in curl python3 sha256sum; do command -v "$tool" >/dev/null 2>&1 || fail "tool_missing:$tool"; done
mkdir -p "$(dirname "$DEST")"

verify_archive() {
  local archive="$1"
  local actual
  actual="$(sha256sum "$archive" | awk '{print $1}')"
  [[ "$actual" == "$EXPECTED_SHA256" ]] || return 1
  python3 - "$archive" "$EXPECTED_ROOT" "$REVIEWED_COMMIT" <<'PY'
import sys, zipfile
from pathlib import Path
archive=Path(sys.argv[1]); root=sys.argv[2]; commit=sys.argv[3]
with zipfile.ZipFile(archive) as zf:
    names=zf.namelist()
    if not names or any(not n.startswith(root) for n in names):
        raise SystemExit(2)
    comment=zf.comment.decode('ascii', errors='strict')
    if comment != commit:
        raise SystemExit(3)
PY
}

if [[ -f "$DEST" ]] && verify_archive "$DEST"; then
  printf 'reviewed OmniRoute archive ready: %s\n' "$DEST"
  exit 0
fi
rm -f "$DEST"

tmp="${DEST}.part"
rm -f "$tmp"
# This is the exact GitHub branch archive that was reviewed. The branch name explains the
# reviewed root directory OmniRoute-release-v3.8.50. Hash + embedded commit comment are both
# fail-closed before the bytes can enter the build lane.
curl --fail --location --silent --show-error \
  --proto '=https' --tlsv1.2 \
  --retry 3 --retry-all-errors \
  --output "$tmp" "$URL"
if ! verify_archive "$tmp"; then
  actual="$(sha256sum "$tmp" 2>/dev/null | awk '{print $1}' || true)"
  rm -f "$tmp"
  fail "reviewed_archive_integrity_mismatch:expected=${EXPECTED_SHA256}:actual=${actual:-unreadable}:ref=${SOURCE_REF}"
fi
mv "$tmp" "$DEST"
printf 'reviewed OmniRoute archive ready: %s\n' "$DEST"
