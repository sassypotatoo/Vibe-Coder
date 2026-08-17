#!/usr/bin/env python3
"""Regression for live + persistent OmniRoute child-process build logging."""
from __future__ import annotations
import contextlib
import importlib.util
import io
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILDER = ROOT / "scripts" / "build_omniroute_android_bundle.py"

spec = importlib.util.spec_from_file_location("vibecoder_omniroute_builder_logging", BUILDER)
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
run = module.run


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="vibecoder-part343-live-log-") as td:
        log = Path(td) / "persisted.log"
        capture = io.StringIO()
        child = [
            sys.executable,
            "-c",
            "import sys; print('child-stdout', flush=True); print('child-stderr', file=sys.stderr, flush=True)",
        ]
        with contextlib.redirect_stdout(capture):
            run(child, log=log)

        console = capture.getvalue()
        persisted = log.read_text(encoding="utf-8")
        for token in ("[omniroute-build] START", "child-stdout", "child-stderr", "[omniroute-build] DONE"):
            if token not in console:
                raise AssertionError(f"live console stream missing token: {token}")
        for token in ("child-stdout", "child-stderr"):
            if token not in persisted:
                raise AssertionError(f"persistent log missing token: {token}")

        capture = io.StringIO()
        failing = [sys.executable, "-c", "print('before-failure', flush=True); raise SystemExit(7)"]
        try:
            with contextlib.redirect_stdout(capture):
                run(failing, log=log)
        except RuntimeError as exc:
            if "command_failed:7" not in str(exc):
                raise AssertionError(f"wrong failure: {exc}")
        else:
            raise AssertionError("failing child command unexpectedly passed")
        if "before-failure" not in log.read_text(encoding="utf-8"):
            raise AssertionError("failed child output was not retained")

    source = BUILDER.read_text(encoding="utf-8")
    for token in (
        "still running after",
        "stop_heartbeat.wait(60)",
        "vibecoder-part34-omniroute-build.log",
        "bufsize=1",
    ):
        if token not in source:
            raise AssertionError(f"builder missing live-build contract token: {token}")

    print("Part 34.3 live build logging regression PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
