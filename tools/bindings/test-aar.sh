#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-004: run the instrumented tests of pa-core-android (Kotlin -> UniFFI ->
# libpa_ffi.so) on a connected emulator or device (ANDROID_SERIAL picks one
# when several are attached).
# Run tools/bindings/build-aar.sh first (any ABI set that includes the
# device's ABI; x86_64 for the usual emulator).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${CARGO_TARGET_DIR:-$ROOT/target}/android"
[ -d "$OUT/kotlin" ] && [ -d "$OUT/jniLibs" ] || { echo "run tools/bindings/build-aar.sh first" >&2; exit 1; }

# Gradle runs connected tests on every attached device; with several attached,
# require an explicit choice instead of installing on all of them.
if [ -z "${ANDROID_SERIAL:-}" ]; then
  count=$(adb devices | awk 'NR > 1 && $2 == "device"' | wc -l)
  [ "$count" -le 1 ] || { echo "$count devices attached: set ANDROID_SERIAL to the one to test on" >&2; exit 1; }
fi

cd "$ROOT/tools/bindings/pa-core-android"
gradle connectedDebugAndroidTest -PpaGenDir="$OUT" --console=plain
