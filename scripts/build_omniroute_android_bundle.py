#!/usr/bin/env python3
"""Build the reviewed OmniRoute 3.8.50 source and seal an Android-safe backend bundle."""
from __future__ import annotations
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROFILE = json.loads((ROOT / "config" / "omniroute-android-runtime-profile.json").read_text())
PREP = ROOT / "scripts" / "prepare_omniroute_android_source.py"
SEAL = ROOT / "scripts" / "prepare_omniroute_android_bundle.py"
VERIFY = ROOT / "scripts" / "verify_omniroute_android_bundle.py"


def run(cmd, *, cwd=None, env=None, log=None):
    if log:
        with log.open("a", encoding="utf-8") as f:
            f.write("$ " + " ".join(map(str, cmd)) + "\n")
            result = subprocess.run(cmd, cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT)
    else:
        result = subprocess.run(cmd, cwd=cwd, env=env)
    if result.returncode != 0:
        raise RuntimeError(f"command_failed:{result.returncode}:{cmd[0]}")


def version(exe: str, *args: str) -> str:
    r = subprocess.run([exe, *args], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if r.returncode != 0:
        raise SystemExit(f"omniroute_build_tool_unavailable:{exe}")
    return r.stdout.strip()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("reviewed_archive", type=Path)
    ap.add_argument("output_bundle", type=Path)
    ap.add_argument("--node", default="node")
    ap.add_argument("--npm", default="npm")
    ap.add_argument("--work-root", type=Path)
    ap.add_argument("--skip-install", action="store_true")
    ap.add_argument("--evidence", type=Path)
    args = ap.parse_args()

    required_node = PROFILE["build"]["required_node_version"]
    node_exe = shutil.which(args.node) or args.node
    npm_exe = shutil.which(args.npm) or args.npm
    node_version = version(node_exe, "--version").lstrip("v")
    npm_version = version(npm_exe, "--version")
    evidence = {
        "schema": 1,
        "step": "34.3.2-android-backend-runtime-bundle",
        "node_version": node_version,
        "npm_version": npm_version,
        "profile_id": PROFILE["profile_id"],
        "source_admitted": False,
        "npm_install_completed": False,
        "backend_build_completed": False,
        "android_bundle_sealed": False,
        "android_bundle_verified": False,
        "apk_asset_packaged": False,
        "service_round_trip_proven": False,
    }
    if node_version != required_node:
        evidence["failure"] = "node_version_mismatch"
        evidence["expected_node_version"] = required_node
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        raise SystemExit(f"omniroute_android_build_node_version_mismatch:expected={required_node}:actual={node_version}")

    owned_temp = args.work_root is None
    work = args.work_root.resolve() if args.work_root else Path(tempfile.mkdtemp(prefix="vibecoder-omniroute-android-"))
    work.mkdir(parents=True, exist_ok=True)
    prepared = work / "prepared"
    admission_evidence = work / "source-admission.json"
    build_log = work / "build.log"
    try:
        run([sys.executable, str(PREP), str(args.reviewed_archive), str(prepared), "--evidence", str(admission_evidence)], log=build_log)
        evidence["source_admitted"] = True
        source = prepared / "OmniRoute-release-v3.8.50"
        env = os.environ.copy()
        env["PATH"] = str(Path(node_exe).resolve().parent) + os.pathsep + env.get("PATH", "")
        env.update(PROFILE["build"]["environment"])
        # Runtime-only flags are also safe during static generation and make optional
        # feature imports fail-open rather than materialising desktop-native state.
        env.update({"VECTOR_STORE_DISABLE_VEC": "true", "OMNIROUTE_MITM_STUB": "1"})
        if not args.skip_install:
            run([npm_exe, "ci"], cwd=source, env=env, log=build_log)
            evidence["npm_install_completed"] = True
        else:
            if not (source / "node_modules" / "next" / "package.json").is_file():
                raise SystemExit("omniroute_android_build_skip_install_without_node_modules")
            evidence["npm_install_completed"] = True
        run([npm_exe, "run", "build:backend"], cwd=source, env=env, log=build_log)
        evidence["backend_build_completed"] = True
        standalone = source / ".build" / "next" / "standalone"
        run([sys.executable, str(SEAL), str(standalone), str(args.output_bundle), "--allowed-symlink-root", str(source)], log=build_log)
        evidence["android_bundle_sealed"] = True
        run([sys.executable, str(VERIFY), str(args.output_bundle)], log=build_log)
        evidence["android_bundle_verified"] = True
        manifest = json.loads((args.output_bundle / ".vibecoder-omniroute-bundle.json").read_text())
        evidence["bundle_tree_sha256"] = manifest["tree_sha256"]
        evidence["bundle_file_count"] = manifest["file_count"]
        evidence["bundle_total_bytes"] = manifest["total_bytes"]
        print(f"OmniRoute Android backend bundle ready: {args.output_bundle}")
        return 0
    except RuntimeError as exc:
        evidence["failure"] = str(exc)
        raise SystemExit(str(exc))
    finally:
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        if owned_temp:
            shutil.rmtree(work, ignore_errors=True)

if __name__ == "__main__":
    sys.exit(main())
