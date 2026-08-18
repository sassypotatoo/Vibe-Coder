#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:-$ROOT/android/app/build/outputs/apk/debug/app-debug.apk}"
MODE="${2:-minimal}"
PACKAGE="com.vibecoder.shell"
ACTIVITY="com.vibecoder.shell/.MainActivity"
REPORT_REL="files/vibecoder-diagnostic-result.json"
fail() { printf 'test_android_diagnostic_device: %s\n' "$1" >&2; exit 1; }
[[ "$MODE" == "minimal" || "$MODE" == "jcode" || "$MODE" == "node" || "$MODE" == "omniroute_asset" || "$MODE" == "omniroute_service" || "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" || "$MODE" == "alpha" || "$MODE" == "sideload_alpha" ]] || fail "mode_must_be_minimal_jcode_node_omniroute_asset_omniroute_service_omniroute_gateway_omniroute_inference_alpha_or_sideload_alpha"
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
if [[ "$MODE" == "omniroute_service" || "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" ]]; then
  EXTRA_GATEWAY=()
  EXTRA_INFERENCE=()
  if [[ "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" ]]; then
    EXTRA_GATEWAY=(--ez vibecoder_omniroute_gateway_test true)
  fi
  if [[ "$MODE" == "omniroute_inference" ]]; then
    [[ -n "${OMNIROUTE_TEST_MODEL_ID:-}" ]] || fail "OMNIROUTE_TEST_MODEL_ID_required_for_omniroute_inference"
    [[ ${#OMNIROUTE_TEST_MODEL_ID} -le 512 ]] || fail "OMNIROUTE_TEST_MODEL_ID_too_long"
    EXTRA_INFERENCE=(--ez vibecoder_omniroute_inference_test true --es vibecoder_omniroute_model "$OMNIROUTE_TEST_MODEL_ID")
  fi
  "${ADB[@]}" shell am start -W -n "$ACTIVITY" \
    --ez vibecoder_diagnostic_test true \
    --ez vibecoder_omniroute_service_test true \
    --ez vibecoder_omniroute_service_stop_after_probe true \
    "${EXTRA_GATEWAY[@]}" \
    "${EXTRA_INFERENCE[@]}" >/dev/null
else
  "${ADB[@]}" shell am start -W -n "$ACTIVITY" \
    --ez vibecoder_diagnostic_test true >/dev/null
fi
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
evidence={item.get('component_id'):item for item in probe.get('native_evidence', [])}
if mode in ('jcode', 'alpha'):
    if ready.get('agent_ready') is not True: raise SystemExit(f"jcode_agent_not_ready:blockers={ready.get('blockers')}")
    j=evidence.get('jcode') or {}
    keys=('package_presence','arm64_identity','execution','version','unix_socket_round_trip','page_size_16k_compatibility')
    missing=[k for k in keys if j.get(k)!='passed']
    if missing: raise SystemExit(f"jcode_device_proof_incomplete:{missing}")
if mode == 'node':
    n=evidence.get('node') or {}
    keys=('package_presence','arm64_identity','execution','version','page_size_16k_compatibility')
    missing=[k for k in keys if n.get(k)!='passed']
    if missing: raise SystemExit(f"node_device_proof_incomplete:{missing}")
    if n.get('observed_version') != '24.19.0':
        raise SystemExit(f"node_device_version_mismatch:{n.get('observed_version')}")
if mode in ('omniroute_asset', 'alpha'):
    omni=report.get('omniroute_asset_installation') or {}
    if omni.get('packaged') is not True: raise SystemExit('omniroute_asset_not_packaged')
    if omni.get('verified') is not True: raise SystemExit(f"omniroute_asset_not_verified:{omni.get('status')}:{omni.get('error','')}")
    if omni.get('status') not in ('installed_verified','verified_existing'):
        raise SystemExit(f"omniroute_asset_install_status_invalid:{omni.get('status')}")
    if omni.get('service_round_trip_proven') is not False:
        raise SystemExit('omniroute_asset_step_overclaimed_service_proof')
if mode in ('omniroute_service', 'omniroute_gateway', 'omniroute_inference'):
    n=evidence.get('node') or {}
    keys=('package_presence','arm64_identity','execution','version','page_size_16k_compatibility')
    missing=[k for k in keys if n.get(k)!='passed']
    if missing: raise SystemExit(f"omniroute_service_node_device_proof_incomplete:{missing}")
    if n.get('observed_version') != '24.19.0':
        raise SystemExit(f"omniroute_service_node_version_mismatch:{n.get('observed_version')}")
    omni=report.get('omniroute_asset_installation') or {}
    if omni.get('packaged') is not True or omni.get('verified') is not True:
        raise SystemExit(f"omniroute_service_asset_not_verified:{omni.get('status')}:{omni.get('error','')}")
    svc=report.get('omniroute_service') or {}
    if svc.get('component_id') != 'omniroute': raise SystemExit('omniroute_service_component_mismatch')
    if svc.get('active') is not True or svc.get('ready') is not True:
        raise SystemExit(f"omniroute_service_not_ready:{svc.get('status')}:{svc.get('error','')}")
    if svc.get('runtime_profile_round_trip_proven') is not True:
        raise SystemExit('omniroute_runtime_profile_round_trip_not_proven')
    if svc.get('base_url') != 'http://127.0.0.1:20128/v1':
        raise SystemExit(f"omniroute_service_base_url_mismatch:{svc.get('base_url')}")
    if svc.get('exact_model_only') is not True or svc.get('hidden_model_reroutes_disabled') is not True:
        raise SystemExit('omniroute_service_exact_routing_attestation_failed')
    if svc.get('signed_manifest_sha256') != omni.get('manifest_sha256'):
        raise SystemExit('omniroute_service_signed_manifest_binding_mismatch')
    if svc.get('last_exit') is not None:
        raise SystemExit('omniroute_service_unexpected_exit_before_acceptance')
    live=svc.get('live_status') or {}
    if live.get('active') is not True or live.get('ready') is not True:
        raise SystemExit(f"omniroute_service_live_status_not_ready:{live.get('status')}:{live.get('error','')}")
    stopped=svc.get('stop_result') or {}
    if stopped.get('active') is not False or stopped.get('ready') is not False or stopped.get('status') != 'stopped':
        raise SystemExit(f"omniroute_service_explicit_stop_failed:{stopped.get('status')}:{stopped.get('error','')}")
    last_exit=stopped.get('last_exit') or {}
    if last_exit.get('termination') not in ('cancelled','exited','signaled'):
        raise SystemExit(f"omniroute_service_stop_termination_invalid:{last_exit.get('termination')}")
    if svc.get('service_round_trip_proven') is not True:
        raise SystemExit('omniroute_service_round_trip_not_proven')
    if mode in ('omniroute_gateway', 'omniroute_inference'):
        gateway=report.get('omniroute_gateway_transport') or {}
        if gateway.get('component_id') != 'omniroute': raise SystemExit('omniroute_gateway_component_mismatch')
        if gateway.get('base_url') != 'http://127.0.0.1:20128/v1':
            raise SystemExit(f"omniroute_gateway_base_url_mismatch:{gateway.get('base_url')}")
        if gateway.get('credential_mode') != 'anonymous': raise SystemExit('omniroute_gateway_diagnostic_credential_mode_not_anonymous')
        if gateway.get('credential_persisted') is not False: raise SystemExit('omniroute_gateway_credential_persistence_overclaim')
        if gateway.get('service_attested') is not True: raise SystemExit('omniroute_gateway_service_not_attested')
        if gateway.get('runtime_profile_verified') is not True: raise SystemExit('omniroute_gateway_runtime_profile_not_verified')
        if gateway.get('runtime_profile_id') != 'vibecoder-omniroute-exact-model-v1':
            raise SystemExit(f"omniroute_gateway_profile_id_mismatch:{gateway.get('runtime_profile_id')}")
        if gateway.get('local_transport_round_trip_proven') is not True:
            raise SystemExit(f"omniroute_gateway_transport_round_trip_not_proven:{gateway.get('status')}:{gateway.get('detail')}")
        if gateway.get('catalog_probe_attempted') is not True or gateway.get('catalog_round_trip_reached') is not True:
            raise SystemExit(f"omniroute_gateway_catalog_round_trip_not_reached:{gateway.get('status')}:{gateway.get('detail')}")
        if gateway.get('health_status') not in ('ready','authentication_required','authentication_rejected','access_denied','no_usable_models','rate_limited','endpoint_not_found','invalid_response','unavailable'):
            raise SystemExit(f"omniroute_gateway_health_status_invalid:{gateway.get('health_status')}")
        if gateway.get('inference_request_sent') is not False or gateway.get('first_model_request_proven') is not False:
            raise SystemExit('omniroute_gateway_step_overclaimed_inference')
    if mode == 'omniroute_inference':
        inference=report.get('omniroute_inference') or {}
        if inference.get('component_id') != 'omniroute': raise SystemExit('omniroute_inference_component_mismatch')
        if inference.get('base_url') != 'http://127.0.0.1:20128/v1':
            raise SystemExit(f"omniroute_inference_base_url_mismatch:{inference.get('base_url')}")
        if inference.get('credential_mode') != 'anonymous': raise SystemExit('omniroute_inference_diagnostic_credential_mode_not_anonymous')
        for field in ('credential_persisted','prompt_persisted','response_text_persisted','automatic_retry_or_model_fallback'):
            if inference.get(field) is not False: raise SystemExit(f'omniroute_inference_privacy_or_retry_contract_failed:{field}')
        if inference.get('service_attested') is not True or inference.get('runtime_profile_verified') is not True:
            raise SystemExit('omniroute_inference_runtime_not_attested')
        if inference.get('catalog_model_verified') is not True:
            raise SystemExit(f"omniroute_inference_model_not_verified:{inference.get('requested_model_id')}")
        if inference.get('inference_request_sent') is not True or inference.get('inference_requests_count') != 1:
            raise SystemExit('omniroute_inference_exactly_one_request_not_proven')
        if inference.get('response_received') is not True or inference.get('response_nonempty') is not True or int(inference.get('response_utf8_bytes') or 0) <= 0:
            raise SystemExit(f"omniroute_inference_response_not_proven:{inference.get('status')}:{inference.get('detail')}")
        if inference.get('observed_model_matches_request') is False:
            raise SystemExit(f"omniroute_inference_observed_model_mismatch:{inference.get('observed_model_id')}")
        if inference.get('first_model_request_proven') is not True:
            raise SystemExit(f"omniroute_first_model_request_not_proven:{inference.get('status')}:{inference.get('detail')}")
print(json.dumps({'device_test':'PASSED','mode':mode,'sdk_int':report.get('sdk_int'),'core_ready':ready.get('core_ready'),'agent_ready':ready.get('agent_ready'),'node_ready': all((evidence.get('node') or {}).get(k)=='passed' for k in ('package_presence','arm64_identity','execution','version','page_size_16k_compatibility')),'omniroute_asset_verified': (report.get('omniroute_asset_installation') or {}).get('verified') is True,'omniroute_service_ready': (report.get('omniroute_service') or {}).get('ready') is True,'omniroute_gateway_transport_proven': (report.get('omniroute_gateway_transport') or {}).get('local_transport_round_trip_proven') is True,'omniroute_first_model_request_proven': (report.get('omniroute_inference') or {}).get('first_model_request_proven') is True}, separators=(',',':')))
PY
