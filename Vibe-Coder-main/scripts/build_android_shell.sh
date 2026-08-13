#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID="$ROOT/android"

fail() { printf 'build_android_shell: %s\n' "$1" >&2; exit 1; }
SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[[ -n "$SDK_ROOT" && -d "$SDK_ROOT" ]] || fail "android_sdk_root_missing"
[[ -f "$SDK_ROOT/platforms/android-36/android.jar" ]] || fail "android_platform_36_missing"
[[ -d "$SDK_ROOT/build-tools/36.0.0" ]] || fail "android_build_tools_36_0_0_missing"
[[ -d "$SDK_ROOT/ndk/28.2.13676358" ]] || fail "android_ndk_28_2_13676358_missing"
[[ -x "$SDK_ROOT/cmake/3.22.1/bin/cmake" ]] || fail "android_cmake_3_22_1_missing"
command -v java >/dev/null 2>&1 || fail "java_not_found"

cd "$ANDROID"
if [[ -e ./gradlew || -e gradle/wrapper/gradle-wrapper.jar ]]; then
  [[ -x ./gradlew && -f gradle/wrapper/gradle-wrapper.jar ]] \
    || fail "verified_gradle_wrapper_incomplete_run_scripts_bootstrap_gradle_wrapper_sh"
  EXEC=(./gradlew)
elif command -v gradle >/dev/null 2>&1; then
  printf '%s\n' "build_android_shell: verified wrapper absent; using system Gradle only after exact 9.5.0 verification" >&2
  EXEC=(gradle)
else
  fail "verified_gradle_wrapper_missing_run_scripts_bootstrap_gradle_wrapper_sh"
fi

GRADLE_INFO="$("${EXEC[@]}" --version)"
printf '%s\n' "$GRADLE_INFO"
printf '%s\n' "$GRADLE_INFO" | grep -Eq '^Gradle 9\.5\.0$' || fail "gradle_version_must_be_9_5_0"
"${EXEC[@]}" --no-daemon --stacktrace :app:assembleDebug
APK="$ANDROID/app/build/outputs/apk/debug/app-debug.apk"
[[ -f "$APK" ]] || fail "debug_apk_missing_after_successful_gradle_task"
printf '%s\n' "$APK"
