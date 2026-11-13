#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-004: run the instrumented tests of pa-core-android (Kotlin -> UniFFI ->
# libpa_ffi.so) on a connected emulator or device: `adb devices` must list one.
# Run tools/bindings/build-aar.sh first (any ABI set that includes the
# device's ABI; x86_64 for the usual emulator).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${CARGO_TARGET_DIR:-$ROOT/target}/android"
[ -d "$OUT/kotlin" ] && [ -d "$OUT/jniLibs" ] || { echo "run tools/bindings/build-aar.sh first" >&2; exit 1; }

cd "$ROOT/tools/bindings/pa-core-android"
gradle connectedDebugAndroidTest -PpaGenDir="$OUT" --console=plain
