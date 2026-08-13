#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

EXPECTED_NODE_VERSION = "24.19.0"
EXPECTED_NODE_SOURCE_SHA256 = "f6d95e10a0431ee1067fc6aabe9f762908b4716dd35324e1ddb4b1466b76659f"
EXPECTED_NDK_REVISION = "28.2.13676358"
EXPECTED_ANDROID_API = 29
EXPECTED_ABI = "arm64-v8a"


def fail(message: str) -> None:
    raise SystemExit(f"write_node_cross_build_attempt: {message}")


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    if len(sys.argv) != 7:
        fail("usage: write_node_cross_build_attempt.py STATUS EXIT_CODE CLASSIFICATION DETAIL EXECUTION_LOG OUTPUT_JSON")
    status, exit_text, classification, detail, log_text, output_text = sys.argv[1:]
    try:
        exit_code = int(exit_text)
    except ValueError:
        fail(f"exit_code_not_integer:{exit_text}")
    if status not in {"succeeded", "failed"}:
        fail(f"status_invalid:{status}")
    if status == "succeeded" and exit_code != 0:
        fail("success_with_nonzero_exit")
    if status == "failed" and exit_code == 0:
        fail("failure_with_zero_exit")
    if not classification or len(classification) > 128:
        fail("classification_invalid")
    if not detail or len(detail) > 512 or "\n" in detail or "\r" in detail:
        fail("detail_invalid")
    log = Path(log_text).resolve()
    output = Path(output_text).resolve()
    if not log.is_file() or log.stat().st_size <= 0:
        fail("execution_log_missing_or_empty")

    evidence = {
        "schema": 1,
        "part": 34,
        "step": "34.2.3",
        "mode": "node_android_cross_build_attempt",
        "claim": "execution_attempt_only_not_binary_or_device_proof",
        "status": status,
        "exit_code": exit_code,
        "classification": classification,
        "detail": detail,
        "timestamp_utc": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "target": {
            "node_version": EXPECTED_NODE_VERSION,
            "node_source_sha256": EXPECTED_NODE_SOURCE_SHA256,
            "android_ndk_revision": EXPECTED_NDK_REVISION,
            "android_api": EXPECTED_ANDROID_API,
            "abi": EXPECTED_ABI,
            "libc": "bionic",
        },
        "runner": {
            "os": platform.system().lower(),
            "machine": platform.machine().lower(),
            "python": ".".join(map(str, sys.version_info[:3])),
            "android_ndk_root_supplied": bool(os.environ.get("ANDROID_NDK_ROOT") or os.environ.get("ANDROID_NDK_HOME")),
        },
        "execution_log_sha256": sha256_file(log),
        "binary_produced": status == "succeeded",
        "apk_packaging_proven": False,
        "device_execution_proven": False,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(output.suffix + ".tmp")
    temp.write_text(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    os.replace(temp, output)
    print(json.dumps({"node_cross_build_attempt_evidence": "WRITTEN", "status": status, "classification": classification}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
