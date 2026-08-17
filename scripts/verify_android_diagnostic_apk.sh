#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:-$ROOT/android/app/build/outputs/apk/debug/app-debug.apk}"
MODE="${2:-minimal}"
SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
BUILD_TOOLS_VERSION="${ANDROID_BUILD_TOOLS:-36.0.0}"
EXPECTED_CERT_SHA256="9d73bfaeb16e706723bfc417ce43a9ed6b10286835e8a3050a8ddded67506445"
fail() { printf 'verify_android_diagnostic_apk: %s\n' "$1" >&2; exit 1; }
[[ "$MODE" == "minimal" || "$MODE" == "jcode" || "$MODE" == "node" || "$MODE" == "omniroute_asset" || "$MODE" == "omniroute_service" || "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" || "$MODE" == "alpha" ]] || fail "mode_must_be_minimal_jcode_node_omniroute_asset_omniroute_service_omniroute_gateway_omniroute_inference_or_alpha"
[[ -f "$APK" && -s "$APK" ]] || fail "apk_missing_or_empty:$APK"
[[ -n "$SDK_ROOT" && -d "$SDK_ROOT" ]] || fail "android_sdk_root_missing"
ZIPALIGN="$SDK_ROOT/build-tools/$BUILD_TOOLS_VERSION/zipalign"
APKSIGNER="$SDK_ROOT/build-tools/$BUILD_TOOLS_VERSION/apksigner"
[[ -x "$ZIPALIGN" ]] || fail "zipalign_missing:$ZIPALIGN"
[[ -x "$APKSIGNER" ]] || fail "apksigner_missing:$APKSIGNER"
command -v unzip >/dev/null 2>&1 || fail "unzip_not_found"
"$ZIPALIGN" -c -P 16 -v 4 "$APK" >/dev/null
"$APKSIGNER" verify --verbose --Werr "$APK" >/dev/null
CERT_SHA256="$("$APKSIGNER" verify --print-certs "$APK" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' | head -n1 | tr -d ':[:space:]' | tr 'A-F' 'a-f')"
[[ -n "$CERT_SHA256" ]] || fail "signing_certificate_sha256_missing"
[[ "$CERT_SHA256" == "$EXPECTED_CERT_SHA256" ]] || fail "unexpected_signing_certificate_sha256:$CERT_SHA256"
mapfile -t NATIVE_ENTRIES < <(unzip -Z1 "$APK" | grep -E '^lib/[^/]+/[^/]+\.so$' || true)
((${#NATIVE_ENTRIES[@]} > 0)) || fail "apk_has_no_native_entries"
for entry in "${NATIVE_ENTRIES[@]}"; do
  case "$entry" in lib/arm64-v8a/*) ;; *) fail "unexpected_non_arm64_native_entry:$entry" ;; esac
done
for required in lib/arm64-v8a/libvibecoder_shell_jni.so lib/arm64-v8a/libvibecoder_android_host.so; do
  printf '%s\n' "${NATIVE_ENTRIES[@]}" | grep -Fxq "$required" || fail "required_native_entry_missing:$required"
done
if [[ "$MODE" == "jcode" || "$MODE" == "alpha" ]]; then
  printf '%s\n' "${NATIVE_ENTRIES[@]}" | grep -Fxq 'lib/arm64-v8a/libvibecoder_jcode_exec.so' || fail "jcode_native_entry_missing"
fi
if [[ "$MODE" == "node" || "$MODE" == "omniroute_service" || "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" ]]; then
  printf '%s\n' "${NATIVE_ENTRIES[@]}" | grep -Fxq 'lib/arm64-v8a/libvibecoder_node_exec.so' || fail "node_native_entry_missing"
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
for entry in "${NATIVE_ENTRIES[@]}"; do
  case "$entry" in
    */*.so)
      mkdir -p "$TMP/$(dirname "$entry")"
      unzip -p "$APK" "$entry" > "$TMP/$entry"
      python3 "$ROOT/scripts/verify_android_elf.py" "$TMP/$entry" >/dev/null || fail "android_elf_verification_failed:$entry"
      ;;
  esac
done
if [[ "$MODE" == "omniroute_asset" || "$MODE" == "omniroute_service" || "$MODE" == "omniroute_gateway" || "$MODE" == "omniroute_inference" || "$MODE" == "alpha" ]]; then
  OMNI_TMP="$TMP/omniroute-bundle"
  mkdir -p "$OMNI_TMP"
  python3 - "$APK" "$OMNI_TMP" <<'PY'
import pathlib, stat, sys, zipfile
apk, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
prefix = 'assets/omniroute/bundle/'
seen = 0
with zipfile.ZipFile(apk) as z:
    for info in z.infolist():
        if not info.filename.startswith(prefix):
            continue
        rel = info.filename[len(prefix):]
        if not rel or rel.endswith('/'):
            continue
        p = pathlib.PurePosixPath(rel)
        if p.is_absolute() or '..' in p.parts or '.' in p.parts or '\\' in rel:
            raise SystemExit(f'omniroute_apk_asset_unsafe_path:{rel}')
        mode = (info.external_attr >> 16) & 0xFFFF
        if stat.S_ISLNK(mode):
            raise SystemExit(f'omniroute_apk_asset_symlink_forbidden:{rel}')
        target = out.joinpath(*p.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(z.read(info))
        seen += 1
if seen == 0:
    raise SystemExit('omniroute_apk_asset_missing')
PY
  python3 "$ROOT/scripts/verify_omniroute_android_bundle.py" "$OMNI_TMP" >/dev/null || fail "omniroute_apk_asset_bundle_verification_failed"
fi
APK_SHA256="$(sha256sum "$APK" | awk '{print $1}')"
printf 'APK verification PASSED\nmode=%s\nsha256=%s\nsigning_certificate_sha256=%s\nfile=%s\n' "$MODE" "$APK_SHA256" "$CERT_SHA256" "$APK"
