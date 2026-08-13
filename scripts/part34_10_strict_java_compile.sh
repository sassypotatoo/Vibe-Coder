#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLATFORM="${ANDROID_PLATFORM:-36}"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
[[ -n "$SDK_ROOT" ]] || { echo "part34_10_strict_java_compile: android_sdk_root_missing" >&2; exit 1; }
ANDROID_JAR="$SDK_ROOT/platforms/android-$PLATFORM/android.jar"
[[ -f "$ANDROID_JAR" ]] || { echo "part34_10_strict_java_compile: android_jar_missing:$ANDROID_JAR" >&2; exit 1; }
command -v javac >/dev/null 2>&1 || { echo "part34_10_strict_java_compile: javac_missing" >&2; exit 1; }
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mapfile -d '' JAVA_SOURCES < <(find "$ROOT/android/app/src/main/java" -type f -name '*.java' -print0 | sort -z)
(( ${#JAVA_SOURCES[@]} > 0 )) || { echo "part34_10_strict_java_compile: java_sources_missing" >&2; exit 1; }
javac --release 17 -Xlint:all -Werror -cp "$ANDROID_JAR" -d "$TMP/classes" "${JAVA_SOURCES[@]}"
echo "Part 34.10 strict Java compile PASSED"
