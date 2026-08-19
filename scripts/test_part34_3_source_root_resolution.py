#!/usr/bin/env python3
"""Regression for immutable OmniRoute archive-root handoff into the Android bundle builder."""
from __future__ import annotations
import importlib.util
import json
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILDER = ROOT / "scripts" / "build_omniroute_android_bundle.py"

spec = importlib.util.spec_from_file_location("vibecoder_omniroute_builder", BUILDER)
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
resolve_prepared_source = module.resolve_prepared_source


def expect_failure(prepared: Path, evidence: Path, token: str) -> None:
    try:
        resolve_prepared_source(prepared, evidence)
    except SystemExit as exc:
        if token not in str(exc):
            raise AssertionError(f"wrong failure: {exc}")
    else:
        raise AssertionError(f"expected failure containing {token}")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-part343-root-") as td:
        base = Path(td)
        prepared = base / "prepared"
        commit_root = "OmniRoute-ab8f3e83b7564c8dca4497cb0e736ceb75d8a40f"
        source = prepared / commit_root
        source.mkdir(parents=True)
        evidence = base / "admission.json"
        evidence.write_text(json.dumps({"archive_root": commit_root}) + "\n", encoding="utf-8")
        if resolve_prepared_source(prepared, evidence) != source.resolve():
            raise AssertionError("commit-named reviewed root was not resolved")

        legacy_root = "OmniRoute-release-v3.8.50"
        legacy = prepared / legacy_root
        legacy.mkdir()
        evidence.write_text(json.dumps({"archive_root": legacy_root}) + "\n", encoding="utf-8")
        if resolve_prepared_source(prepared, evidence) != legacy.resolve():
            raise AssertionError("historical reviewed root was not resolved")

        evidence.write_text(json.dumps({"archive_root": "../escape"}) + "\n", encoding="utf-8")
        expect_failure(prepared, evidence, "omniroute_source_admission_root_invalid")

        evidence.write_text(json.dumps({"archive_root": "OmniRoute-missing"}) + "\n", encoding="utf-8")
        expect_failure(prepared, evidence, "omniroute_source_admission_root_missing")

    print("Part 34.3 source-root handoff regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
