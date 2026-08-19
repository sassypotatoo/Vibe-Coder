#!/usr/bin/env python3
from __future__ import annotations

import ast
import json
import sys
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f'verify_node_android_configure_output: {message}')


def parse_gypi(path: Path) -> dict:
    if not path.is_file() or path.stat().st_size <= 0:
        fail('config_gypi_missing_or_empty')
    text = path.read_text(encoding='utf-8', errors='strict')
    # Node config.gypi is a generated Python-literal dictionary preceded by comment lines.
    payload = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('#')).strip()
    try:
        value = ast.literal_eval(payload)
    except (SyntaxError, ValueError) as exc:
        fail(f'config_gypi_invalid:{type(exc).__name__}')
    if not isinstance(value, dict):
        fail('config_gypi_root_not_object')
    return value


def main() -> int:
    if len(sys.argv) != 3:
        fail('usage: verify_node_android_configure_output.py CONFIG_GYPI MAKEFILE')
    config_path = Path(sys.argv[1]).resolve()
    makefile = Path(sys.argv[2]).resolve()
    config = parse_gypi(config_path)
    if not makefile.is_file() or makefile.stat().st_size <= 0:
        fail('makefile_missing_or_empty')
    variables = config.get('variables')
    if not isinstance(variables, dict):
        fail('config_variables_missing')
    if variables.get('host_arch') != 'x64':
        fail(f'host_arch_mismatch:{variables.get("host_arch")!r}')
    if variables.get('target_arch') != 'arm64':
        fail(f'target_arch_mismatch:{variables.get("target_arch")!r}')
    if variables.get('want_separate_host_toolset') != 1:
        fail(f'want_separate_host_toolset_mismatch:{variables.get("want_separate_host_toolset")!r}')
    if variables.get('node_target_type') not in (None, 'executable'):
        fail(f'node_target_type_unexpected:{variables.get("node_target_type")!r}')
    print(json.dumps({
        'node_android_configure_output': 'VERIFIED',
        'host_arch': 'x64',
        'target_arch': 'arm64',
        'separate_host_toolset': True,
        'makefile_present': True,
    }, separators=(',', ':')))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
