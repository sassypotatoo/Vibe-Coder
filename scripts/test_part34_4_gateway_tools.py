#!/usr/bin/env python3
"""Source-level regression guards for the Part 34.4 Android gateway bridge."""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
transport = (ROOT / "crates/vibecoder-android-host/src/gateway_transport.rs").read_text()
ffi = (ROOT / "crates/vibecoder-android-host/src/omniroute_ffi.rs").read_text()
activity = (ROOT / "android/app/src/main/java/com/vibecoder/shell/MainActivity.java").read_text()

checks = {
    "fixed_loopback": 'const LOOPBACK_BASE_URL: &str = "http://127.0.0.1:20128/v1"' in (ROOT / "crates/vibecoder-android-host/src/omniroute_service.rs").read_text(),
    "service_attested_before_network": "if !service_attested" in transport,
    "profile_before_catalog": transport.find("execution_profile(credential)") < transport.find("client.health(credential)"),
    "credential_not_persisted": "credential_persisted: false" in transport,
    "inference_false": "inference_request_sent: false" in transport and "first_model_request_proven: false" in transport,
    "ffi_one_shot": "output.is_null() || output_capacity == 0" in ffi and "MAX_GATEWAY_CREDENTIAL_BYTES: usize = 8192" in ffi,
    "diagnostic_anonymous": "nativeOmniRouteGatewayProbe(new byte[0])" in activity,
    "probe_before_stop": (lambda start: activity.find("nativeOmniRouteGatewayProbe(new byte[0])", start) < activity.find("NativeBridge.nativeOmniRouteStop()", start))(activity.find("private JSONObject collectOmniRouteServiceState")),
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit("Part 34.4 gateway-tool regression FAILED: " + ",".join(failed))
for forbidden in ("chat/completions", '"/responses"'):
    if forbidden in transport:
        raise SystemExit("Part 34.4 gateway-tool regression FAILED: inference endpoint present")
print("Part 34.4 gateway-tool regression PASSED")
