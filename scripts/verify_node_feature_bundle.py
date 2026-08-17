#!/usr/bin/env python3
import hashlib
import json
import sys
import zipfile
from pathlib import Path

NODE_ENTRY = "node_runtime/lib/arm64-v8a/libvibecoder_node_exec.so"
BASE_NODE_ENTRY = "base/lib/arm64-v8a/libvibecoder_node_exec.so"
BASE_JCODE_ENTRY = "base/lib/arm64-v8a/libvibecoder_jcode_exec.so"
BASE_HOST_ENTRY = "base/lib/arm64-v8a/libvibecoder_android_host.so"
OMNI_MANIFEST_ENTRY = "base/assets/omniroute/bundle/.vibecoder-omniroute-bundle.json"
FEATURE_MANIFEST_ENTRY = "node_runtime/manifest/AndroidManifest.xml"
NODE_METADATA_ENTRY = "node_runtime/assets/node-runtime/manifest.json"


def fail(code: str) -> None:
    raise SystemExit(f"verify_node_feature_bundle: {code}")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    if len(sys.argv) not in (2, 3):
        fail("usage:AAB [EXPECTED_NODE_BINARY]")
    bundle = Path(sys.argv[1])
    expected_node = Path(sys.argv[2]) if len(sys.argv) == 3 else None
    if not bundle.is_file() or bundle.stat().st_size <= 0:
        fail("bundle_missing")
    try:
        with zipfile.ZipFile(bundle) as archive:
            names = set(archive.namelist())
            for entry in (
                NODE_ENTRY,
                BASE_JCODE_ENTRY,
                BASE_HOST_ENTRY,
                OMNI_MANIFEST_ENTRY,
                FEATURE_MANIFEST_ENTRY,
                NODE_METADATA_ENTRY,
            ):
                if entry not in names:
                    fail("required_entry_missing:" + entry)
            if BASE_NODE_ENTRY in names:
                fail("node_leaked_into_base_module")
            node_bytes = archive.read(NODE_ENTRY)
            try:
                metadata = json.loads(archive.read(NODE_METADATA_ENTRY).decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                fail("node_feature_metadata_invalid_json")
    except zipfile.BadZipFile:
        fail("bundle_not_zip")
    if not node_bytes:
        fail("node_feature_payload_empty")
    node_hash = sha256(node_bytes)
    expected_metadata = {
        "schema": 1,
        "component_id": "node",
        "version": "24.19.0",
        "abi": "arm64-v8a",
        "libc": "bionic",
        "file_name": "libvibecoder_node_exec.so",
        "size": len(node_bytes),
        "sha256": node_hash,
        "delivery": "play_feature_on_demand",
        "module": "node_runtime",
    }
    for key, expected in expected_metadata.items():
        if metadata.get(key) != expected:
            fail("node_feature_metadata_mismatch:" + key)
    if expected_node is not None:
        if not expected_node.is_file() or expected_node.stat().st_size <= 0:
            fail("expected_node_binary_missing")
        expected_hash = hashlib.sha256(expected_node.read_bytes()).hexdigest()
        if node_hash != expected_hash:
            fail("node_feature_payload_hash_mismatch")
    print(json.dumps({
        "schema": 1,
        "bundle": str(bundle),
        "node_module": "node_runtime",
        "node_entry": NODE_ENTRY,
        "node_sha256": node_hash,
        "node_bundled_in_base": False,
        "jcode_bundled_in_base": True,
        "omniroute_bundled_in_base": True,
        "verification": "PASSED",
    }, sort_keys=True))


if __name__ == "__main__":
    main()
