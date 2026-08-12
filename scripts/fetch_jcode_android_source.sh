#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="https://github.com/1jehuang/jcode.git"
COMMIT="44ffa55281fad71c02be984c0674d92412210452"
DEST="${1:-$ROOT/.toolchains/jcode-v0.73.0}"

fail() { printf 'fetch_jcode_android_source: %s\n' "$1" >&2; exit 1; }
command -v git >/dev/null 2>&1 || fail "git_not_found"

if [[ -e "$DEST" ]] && ! git -C "$DEST" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  fail "destination_exists_but_is_not_git_checkout:$DEST"
fi
if ! git -C "$DEST" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  mkdir -p "$(dirname "$DEST")"
  git clone --filter=blob:none --no-checkout "$REPO" "$DEST"
fi

git -C "$DEST" fetch --depth=1 origin "$COMMIT"
git -C "$DEST" checkout --detach "$COMMIT"
bash "$ROOT/scripts/verify_jcode_android_source.sh" "$DEST"
printf 'jcode source ready: %s\n' "$DEST"
