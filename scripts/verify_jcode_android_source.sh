#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPECTED_REPO="https://github.com/1jehuang/jcode.git"
EXPECTED_TAG="v0.73.0"
EXPECTED_COMMIT="44ffa55281fad71c02be984c0674d92412210452"
EXPECTED_VERSION="0.73.0"
SOURCE_DIR="${1:-}"

fail() { printf 'verify_jcode_android_source: %s\n' "$1" >&2; exit 1; }
[[ -n "$SOURCE_DIR" ]] || fail "usage: $0 /path/to/exact-jcode-checkout"
command -v git >/dev/null 2>&1 || fail "git_not_found"
command -v python3 >/dev/null 2>&1 || fail "python3_not_found"

SOURCE_DIR="$(cd "$SOURCE_DIR" && pwd -P)"
[[ "$(git -C "$SOURCE_DIR" rev-parse --is-inside-work-tree 2>/dev/null || true)" == "true" ]] || fail "git_checkout_required"
HEAD="$(git -C "$SOURCE_DIR" rev-parse HEAD 2>/dev/null || true)"
[[ "$HEAD" == "$EXPECTED_COMMIT" ]] || fail "wrong_commit:$HEAD"
[[ -z "$(git -C "$SOURCE_DIR" status --porcelain --untracked-files=all)" ]] || fail "source_checkout_not_clean"

# A detached checkout is fine, but if the tag resolves locally it must resolve to the same commit.
TAG_COMMIT="$(git -C "$SOURCE_DIR" rev-list -n 1 "$EXPECTED_TAG" 2>/dev/null || true)"
if [[ -n "$TAG_COMMIT" && "$TAG_COMMIT" != "$EXPECTED_COMMIT" ]]; then
  fail "tag_commit_mismatch:$TAG_COMMIT"
fi

python3 - "$SOURCE_DIR/Cargo.toml" "$EXPECTED_VERSION" <<'PY'
import sys, re
path, expected = sys.argv[1:]
with open(path, 'r', encoding='utf-8') as f:
    text = f.read()
name_match = re.search(r'^name\s*=\s*"([^"]+)"', text, re.MULTILINE)
version_match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
if not name_match or name_match.group(1) != 'jcode':
    raise SystemExit('verify_jcode_android_source: cargo_package_not_jcode')
if not version_match or version_match.group(1) != expected:
    raise SystemExit(f"verify_jcode_android_source: wrong_version:{version_match.group(1) if version_match else 'None'}")
PY

for required in \
  Cargo.lock \
  crates/jcode-harness-api/Cargo.toml \
  crates/jcode-harness-api-server/Cargo.toml \
  crates/jcode-sdk/Cargo.toml \
  src/main.rs; do
  [[ -f "$SOURCE_DIR/$required" ]] || fail "required_source_missing:$required"
done

# Re-prove the exact reviewed public SDK/harness boundary against the source that will be compiled.
(
  cd "$SOURCE_DIR"
  sha256sum -c "$ROOT/third_party/jcode/VENDORED_MANIFEST.sha256" >/dev/null
) || fail "vendored_boundary_does_not_match_exact_source"

printf 'verified jcode source\n'
printf 'repository=%s\n' "$EXPECTED_REPO"
printf 'tag=%s\n' "$EXPECTED_TAG"
printf 'commit=%s\n' "$EXPECTED_COMMIT"
printf 'version=%s\n' "$EXPECTED_VERSION"
