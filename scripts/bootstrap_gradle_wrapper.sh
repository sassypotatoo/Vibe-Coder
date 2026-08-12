#!/usr/bin/env bash
set -euo pipefail
# Generates wrapper files from a caller-supplied official Gradle 9.5.0 binary distribution.
# The distribution is verified before execution; this script intentionally does not fetch it.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE="${1:-}"
EXPECTED="553c78f50dafcd54d65b9a444649057857469edf836431389695608536d6b746"
EXPECTED_WRAPPER="497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7"
[[ -n "$ARCHIVE" && -f "$ARCHIVE" ]] || { echo 'usage: bootstrap_gradle_wrapper.sh /path/to/gradle-9.5.0-bin.zip' >&2; exit 2; }
printf '%s  %s\n' "$EXPECTED" "$ARCHIVE" | sha256sum --check --status || { echo 'gradle_distribution_sha256_mismatch' >&2; exit 1; }
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
unzip -q "$ARCHIVE" -d "$TMP/dist"
mkdir -p "$TMP/project"
printf 'rootProject.name = "wrapper-bootstrap"\n' > "$TMP/project/settings.gradle"
"$TMP/dist/gradle-9.5.0/bin/gradle" -p "$TMP/project" wrapper --gradle-version 9.5.0 --distribution-type bin
printf '%s  %s\n' "$EXPECTED_WRAPPER" "$TMP/project/gradle/wrapper/gradle-wrapper.jar" | sha256sum --check --status || { echo 'generated_wrapper_sha256_mismatch' >&2; exit 1; }
printf '\ndistributionSha256Sum=%s\n' "$EXPECTED" >> "$TMP/project/gradle/wrapper/gradle-wrapper.properties"
mkdir -p "$ROOT/android/gradle/wrapper"
cp "$TMP/project/gradlew" "$ROOT/android/gradlew"
cp "$TMP/project/gradlew.bat" "$ROOT/android/gradlew.bat"
cp "$TMP/project/gradle/wrapper/gradle-wrapper.jar" "$ROOT/android/gradle/wrapper/gradle-wrapper.jar"
cp "$TMP/project/gradle/wrapper/gradle-wrapper.properties" "$ROOT/android/gradle/wrapper/gradle-wrapper.properties"
chmod +x "$ROOT/android/gradlew"
printf 'wrapper_generated_from_verified_gradle_9_5_0\n'
