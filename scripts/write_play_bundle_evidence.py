#!/usr/bin/env python3
import hashlib
import json
import sys
import zipfile
from pathlib import Path

NODE_ENTRY = "node_runtime/lib/arm64-v8a/libvibecoder_node_exec.so"


def fail(code: str) -> None:
    raise SystemExit(f"write_play_bundle_evidence: {code}")


def file_hash(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    if len(sys.argv) != 5:
        fail("usage:AAB NODE_BINARY NODE_CROSS_BUILD_EVIDENCE OUTPUT_JSON")
    bundle, node, cross, output = map(Path, sys.argv[1:])
    for path, label in ((bundle, "bundle"), (node, "node"), (cross, "cross_build_evidence")):
        if not path.is_file() or path.stat().st_size <= 0:
            fail(label + "_missing")
    try:
        cross_data = json.loads(cross.read_text())
    except Exception:
        fail("cross_build_evidence_invalid_json")
    node_claim = cross_data.get("node") or {}
    target = cross_data.get("target") or {}
    if node_claim.get("version") != "24.19.0":
        fail("cross_build_node_version_mismatch")
    if target.get("os") != "android" or target.get("abi") != "arm64-v8a" or target.get("libc") != "bionic":
        fail("cross_build_target_mismatch")
    node_sha = file_hash(node)
    if node_claim.get("output_sha256") != node_sha or node_claim.get("output_size") != node.stat().st_size:
        fail("cross_build_node_hash_mismatch")
    with zipfile.ZipFile(bundle) as archive:
        if NODE_ENTRY not in archive.namelist():
            fail("node_feature_entry_missing")
        module_sha = hashlib.sha256(archive.read(NODE_ENTRY)).hexdigest()
    if module_sha != node_sha:
        fail("bundle_node_hash_mismatch")
    payload = {
        "schema": 1,
        "part": 34,
        "step": "34.10.15",
        "claim": "play_bundle_package_evidence_only_not_device_execution",
        "bundle_sha256": file_hash(bundle),
        "bundle_bytes": bundle.stat().st_size,
        "node_version": "24.19.0",
        "node_delivery": "play_feature_on_demand",
        "node_feature_module": "node_runtime",
        "node_bundled_in_base": False,
        "node_binary_sha256": node_sha,
        "node_cross_build_evidence_sha256": file_hash(cross),
        "device_install_proven": False,
        "node_device_execution_proven": False,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(output.suffix + ".tmp")
    temp.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    temp.replace(output)
    print(output)


if __name__ == "__main__":
    main()
