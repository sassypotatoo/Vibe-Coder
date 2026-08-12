#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:-$ROOT/android/app/build/outputs/apk/debug/app-debug.apk}"
MODE="${2:-minimal}"
PACKAGE="com.vibecoder.shell"
ACTIVITY="com.vibecoder.shell/.MainActivity"
REPORT_REL="files/vibecoder-diagnostic-result.json"
fail() { printf 'test_android_diagnostic_device: %s\n' "$1" >&2; exit 1; }
[[ "$MODE" == "minimal" || "$MODE" == "jcode" ]] || fail "mode_must_be_minimal_or_jcode"
[[ -f "$APK" && -s "$APK" ]] || fail "apk_missing_or_empty:$APK"
command -v adb >/dev/null 2>&1 || fail "adb_not_found"
command -v python3 >/dev/null 2>&1 || fail "python3_not_found"
if [[ -n "${ANDROID_SERIAL:-}" ]]; then SERIAL="$ANDROID_SERIAL"; else
  mapfile -t DEVICES < <(adb devices | awk 'NR>1 && $2=="device" {print $1}')
  ((${#DEVICES[@]} == 1)) || fail "expected_exactly_one_authorized_device_or_set_ANDROID_SERIAL"
  SERIAL="${DEVICES[0]}"
fi
ADB=(adb -s "$SERIAL")
[[ "$("${ADB[@]}" get-state 2>/dev/null)" == "device" ]] || fail "device_not_ready:$SERIAL"
ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abilist | tr -d '\r')"
printf '%s' "$ABILIST" | grep -Eq '(^|,)arm64-v8a(,|$)' || fail "device_is_not_arm64_v8a:$ABILIST"
SDK="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
[[ "$SDK" =~ ^[0-9]+$ && "$SDK" -ge 29 ]] || fail "device_sdk_below_29:$SDK"
"${ADB[@]}" install -r -t "$APK" >/dev/null
"${ADB[@]}" shell am force-stop "$PACKAGE" >/dev/null
"${ADB[@]}" shell run-as "$PACKAGE" rm -f "$REPORT_REL" >/dev/null 2>&1 || true
"${ADB[@]}" shell am start -W -n "$ACTIVITY" >/dev/null
TMP="$(mktemp)"; trap 'rm -f "$TMP"' EXIT
READY=0
for _ in $(seq 1 40); do
  if "${ADB[@]}" shell run-as "$PACKAGE" cat "$REPORT_REL" > "$TMP" 2>/dev/null && [[ -s "$TMP" ]]; then READY=1; break; fi
  sleep 0.5
done
if [[ "$READY" != 1 ]]; then
  printf '%s\n' '--- AndroidRuntime log tail ---' >&2
  "${ADB[@]}" logcat -d -t 120 'AndroidRuntime:E' '*:S' >&2 || true
  fail "diagnostic_report_not_produced"
fi
python3 - "$TMP" "$MODE" <<'PY'
import json, sys
path, mode = sys.argv[1:]
try: report=json.load(open(path, encoding='utf-8'))
except Exception as exc: raise SystemExit(f"device_report_invalid_json:{exc}")
if report.get('schema') != 1 or report.get('part') != 31: raise SystemExit('device_report_schema_or_part_mismatch')
if report.get('package') != 'com.vibecoder.shell': raise SystemExit('device_report_package_mismatch')
if report.get('device_arm64') is not True: raise SystemExit('device_report_not_arm64')
probe=report.get('probe_snapshot') or {}
if probe.get('native_loaded') is not True: raise SystemExit(f"rust_host_not_loaded:{probe.get('error','unknown')}")
if probe.get('probe_ok') is not True: raise SystemExit(f"rust_host_probe_failed:{probe.get('error','unknown')}")
ready=probe.get('readiness') or {}
if ready.get('core_ready') is not True: raise SystemExit(f"core_not_ready:blockers={ready.get('blockers')}")
if mode == 'jcode':
    if ready.get('agent_ready') is not True: raise SystemExit(f"jcode_agent_not_ready:blockers={ready.get('blockers')}")
    evidence={item.get('component_id'):item for item in probe.get('native_evidence', [])}
    j=evidence.get('jcode') or {}
    keys=('package_presence','arm64_identity','execution','version','unix_socket_round_trip','page_size_16k_compatibility')
    missing=[k for k in keys if j.get(k)!='passed']
    if missing: raise SystemExit(f"jcode_device_proof_incomplete:{missing}")
print(json.dumps({'device_test':'PASSED','mode':mode,'sdk_int':report.get('sdk_int'),'core_ready':ready.get('core_ready'),'agent_ready':ready.get('agent_ready')}, separators=(',',':')))
PY
