#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JCODE="${1:-}"
OUT="$ROOT/android/app/build/outputs/jcode-runtime/vibecoder-jcode-runtime-arm64-v8a.apk"
AAB="$ROOT/android/app/build/outputs/bundle/debug/app-debug.aab"
APKS="$ROOT/android/app/build/outputs/jcode-runtime/jcode-runtime.apks"
BUNDLETOOL="$ROOT/android/app/build/outputs/jcode-runtime/bundletool-all-1.18.3.jar"
BUNDLETOOL_SHA="a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29"
fail(){ printf 'build_jcode_runtime_release: %s\n' "$1" >&2; exit 1; }
[[ -n "$JCODE" && -f "$JCODE" && -s "$JCODE" ]] || fail jcode_binary_missing
python3 "$ROOT/scripts/verify_android_elf.py" "$JCODE" >/dev/null
python3 "$ROOT/scripts/stage_jcode_runtime_split.py" "$JCODE"
# Jcode belongs only to the downloadable feature split in development builds.
rm -f "$ROOT/android/app/build/generated/jniLibs/arm64-v8a/libvibecoder_jcode_exec.so"
mkdir -p "$(dirname "$OUT")" "$ROOT/android/signing"
base64 -d "$ROOT/scripts/diagnostic_keystore.b64" > "$ROOT/android/signing/vibecoder-diagnostic-debug.jks"
[[ "$(sha256sum "$ROOT/android/signing/vibecoder-diagnostic-debug.jks" | awk '{print $1}')" == "8144fe738427be8e69e2a880fcefa170daecbddaad3929f7639d628bb14395a6" ]] || fail keystore_integrity_mismatch
cd "$ROOT/android"
if [[ -x ./gradlew && -f gradle/wrapper/gradle-wrapper.jar ]]; then GRADLE=(./gradlew); else GRADLE=(gradle); fi
rm -f "$AAB" "$APKS" "$OUT"
"${GRADLE[@]}" --no-daemon --stacktrace :app:bundleDebug
[[ -s "$AAB" ]] || fail aab_missing
if [[ ! -s "$BUNDLETOOL" ]]; then
  curl -fL --retry 3 --connect-timeout 20 \
    "https://github.com/google/bundletool/releases/download/1.18.3/bundletool-all-1.18.3.jar" -o "$BUNDLETOOL"
fi
[[ "$(sha256sum "$BUNDLETOOL" | awk '{print $1}')" == "$BUNDLETOOL_SHA" ]] || fail bundletool_integrity_mismatch
java -jar "$BUNDLETOOL" build-apks \
  --bundle="$AAB" --output="$APKS" --overwrite \
  --ks="$ROOT/android/signing/vibecoder-diagnostic-debug.jks" \
  --ks-key-alias=vibecoder-diagnostic \
  --ks-pass=pass:vibecoder-debug --key-pass=pass:vibecoder-debug
python3 "$ROOT/scripts/extract_jcode_runtime_split.py" "$APKS" "$OUT"
python3 "$ROOT/scripts/verify_jcode_runtime_split.py" "$OUT" "$JCODE"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
APKSIGNER="$SDK_ROOT/build-tools/${ANDROID_BUILD_TOOLS:-36.0.0}/apksigner"
ZIPALIGN="$SDK_ROOT/build-tools/${ANDROID_BUILD_TOOLS:-36.0.0}/zipalign"
[[ -x "$APKSIGNER" ]] || fail apksigner_missing
[[ -x "$ZIPALIGN" ]] || fail zipalign_missing
"$ZIPALIGN" -c -P 16 -v 4 "$OUT" >/dev/null || fail split_zipalign_invalid
"$APKSIGNER" verify --verbose --Werr "$OUT" >/dev/null || fail split_signature_invalid
EXPECTED_CERT="9d73bfaeb16e706723bfc417ce43a9ed6b10286835e8a3050a8ddded67506445"
ACTUAL_CERT="$("$APKSIGNER" verify --print-certs "$OUT" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' | head -n1 | tr -d ':[:space:]' | tr 'A-F' 'a-f')"
[[ "$ACTUAL_CERT" == "$EXPECTED_CERT" ]] || fail split_signing_certificate_mismatch
printf 'Jcode runtime release APK: %s\n' "$OUT"
