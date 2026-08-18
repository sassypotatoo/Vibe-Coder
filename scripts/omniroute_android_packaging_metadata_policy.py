#!/usr/bin/env python3
"""Runtime-inert metadata that Gradle's file-tree layer excludes before AAPT packaging.

The sealed OmniRoute tree is a runtime payload, not a source checkout. SCM/editor metadata has
no runtime semantics and must never be part of the manifest-bound Android payload because
Gradle/Ant default excludes can remove it before AAPT sees the generated asset directory.
"""
from __future__ import annotations

import os
from pathlib import Path

# Mirrors the source-control/editor metadata class present in Gradle/Ant default excludes.
# Keep this intentionally narrow: do NOT treat generic dotfiles, `.next`, `_not-found`, or
# `.well-known` as metadata; those can be legitimate runtime paths and must survive packaging.
GRADLE_DEFAULT_EXCLUDED_EXACT_NAMES = frozenset({
    ".DS_Store",
    ".bzr",
    ".bzrignore",
    ".cvsignore",
    ".git",
    ".gitattributes",
    ".gitignore",
    ".gitmodules",
    ".hg",
    ".hgignore",
    ".hgsub",
    ".hgsubstate",
    ".hgtags",
    ".svn",
    "CVS",
    "SCCS",
    "vssver.scc",
})


def is_gradle_default_excluded_metadata_name(name: str) -> bool:
    if name in GRADLE_DEFAULT_EXCLUDED_EXACT_NAMES:
        return True
    # Remaining Gradle/Ant editor/OS default-exclude shapes.
    if name.endswith("~"):
        return True
    if name.startswith(".#") or name.startswith("._"):
        return True
    if len(name) >= 2 and name.startswith("#") and name.endswith("#"):
        return True
    if len(name) >= 2 and name.startswith("%") and name.endswith("%"):
        return True
    return False


def scan_gradle_default_excluded_metadata(root: Path) -> list[Path]:
    """Return top-level conflicting paths, skipping children once an excluded dir is found."""
    root = root.resolve()
    conflicts: list[Path] = []
    for current, dirs, files in os.walk(root, topdown=True):
        base = Path(current)
        kept_dirs: list[str] = []
        for name in sorted(dirs):
            path = base / name
            if is_gradle_default_excluded_metadata_name(name):
                conflicts.append(path)
            else:
                kept_dirs.append(name)
        dirs[:] = kept_dirs
        for name in sorted(files):
            if is_gradle_default_excluded_metadata_name(name):
                conflicts.append(base / name)
    return sorted(conflicts, key=lambda path: path.relative_to(root).as_posix())
