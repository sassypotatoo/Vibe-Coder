#!/usr/bin/env python3
"""Regression for the OmniRoute sealer repository-local output boundary."""
from __future__ import annotations

import importlib.util
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEAL = ROOT / "scripts" / "prepare_omniroute_android_bundle.py"


def load_sealer():
    spec = importlib.util.spec_from_file_location("vibecoder_omniroute_sealer", SEAL)
    if spec is None or spec.loader is None:
        raise AssertionError("unable to load OmniRoute sealer")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def reject(module, output: Path, source: Path, token: str) -> None:
    try:
        module.safe_output(output, source)
    except SystemExit as exc:
        if token not in str(exc):
            raise AssertionError(f"wrong rejection for {output}: {exc}") from exc
    else:
        raise AssertionError(f"unsafe output unexpectedly admitted: {output}")


def main() -> int:
    module = load_sealer()
    with tempfile.TemporaryDirectory(prefix="vibecoder-part343-output-policy-") as raw:
        base = Path(raw)
        source = base / "standalone"
        source.mkdir()

        intended = ROOT / "android" / "app" / "build" / "generated" / "omnirouteBundle"
        if module.safe_output(intended, source) != intended.resolve(strict=False):
            raise AssertionError("intended generated OmniRoute output resolved unexpectedly")

        # No other repository-local path is admitted, including siblings and
        # tracked assets. This prevents the sealer's atomic replacement from
        # deleting unrelated build or source content.
        for bad in (
            ROOT,
            ROOT / "scripts" / "omnirouteBundle",
            ROOT / "config" / "omnirouteBundle",
            ROOT / "android" / "app" / "build" / "generated",
            ROOT / "android" / "app" / "build" / "generated" / "otherBundle",
            ROOT / "android" / "app" / "src" / "main" / "assets" / "omnirouteBundle",
        ):
            reject(module, bad, source, "omniroute_bundle_output_protected")

        # External temporary output remains valid for deterministic unit tests.
        outside = base / "sealed"
        if module.safe_output(outside, source) != outside.resolve(strict=False):
            raise AssertionError("external temporary output unexpectedly rejected")

        # Parent symlink redirection must fail before resolve() can hide it.
        fake_repo = base / "fake-repo"
        fake_repo.mkdir()
        redirect = base / "redirected-build"
        (redirect / "generated").mkdir(parents=True)
        (fake_repo / "android" / "app").mkdir(parents=True)
        (fake_repo / "android" / "app" / "build").symlink_to(redirect, target_is_directory=True)
        old_root = module.ROOT
        old_output = module.OMNIROUTE_REPO_OUTPUT
        try:
            module.ROOT = fake_repo
            module.OMNIROUTE_REPO_OUTPUT = (
                fake_repo / "android" / "app" / "build" / "generated" / "omnirouteBundle"
            )
            reject(
                module,
                module.OMNIROUTE_REPO_OUTPUT,
                source,
                "omniroute_bundle_output_parent_symlink_forbidden",
            )
        finally:
            module.ROOT = old_root
            module.OMNIROUTE_REPO_OUTPUT = old_output

        # Source overlap remains independently fail-closed.
        reject(module, source / "nested-output", source, "omniroute_bundle_output_conflicts_with_source")
        reject(module, source.parent, source, "omniroute_bundle_output_conflicts_with_source")

    print("Part 34.3 bundle-output policy regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
