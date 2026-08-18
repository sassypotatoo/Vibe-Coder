#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
[[ -n "$SDK_ROOT" && -d "$SDK_ROOT" ]] || { echo "part34_10_strict_java_compile: android_sdk_root_missing" >&2; exit 1; }
[[ -f "$SDK_ROOT/platforms/android-36/android.jar" ]] || { echo "part34_10_strict_java_compile: android_jar_missing" >&2; exit 1; }
cd "$ROOT/android"
if [[ -x ./gradlew && -f gradle/wrapper/gradle-wrapper.jar ]]; then
  EXEC=(./gradlew)
elif command -v gradle >/dev/null 2>&1; then
  EXEC=(gradle)
else
  echo "part34_10_strict_java_compile: gradle_missing" >&2; exit 1
fi
INFO="$("${EXEC[@]}" --version)"
printf '%s\n' "$INFO" | grep -Eq '^Gradle 9\.5\.0$' || { echo "part34_10_strict_java_compile: gradle_version_must_be_9_5_0" >&2; exit 1; }
# JavaCompile is configured with -Xlint:all -Werror in app/build.gradle.kts. This compiles against
# the actual Android application source and SDK instead of relying on source-only inspection.
"${EXEC[@]}" --no-daemon --stacktrace :app:compileDebugJavaWithJavac
echo "Part 34.10 strict Java compile PASSED"
