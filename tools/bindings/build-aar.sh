#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-004: build the Rust core (pa-ffi) for Android with cargo-ndk, generate
# the UniFFI Kotlin bindings and pack both into the AAR `pa-core-android`
# (Gradle project tools/bindings/pa-core-android).
#
# ABIs: arm64-v8a, armeabi-v7a, x86_64 (minSdk 24).
#
# Usage: tools/bindings/build-aar.sh [--debug] [abi ...]
#   No ABIs = all of them. Needs rustup, cargo-ndk, JDK 17+, `gradle` on PATH,
#   ANDROID_HOME (or ANDROID_SDK_ROOT) and an NDK: ANDROID_NDK_HOME, or the
#   pinned version (NDK_VERSION below) under $ANDROID_HOME/ndk.
#   PA_FLAVOR=store|full (default store): build flavor of the core, the cargo
#   feature of pa-ffi (ADR-0007, CORE-013).
#
# Output (not committed, ADR-0001), under $CARGO_TARGET_DIR (default target/):
#   android/jniLibs/<abi>/libpa_ffi.so
#   android/kotlin/dev/projectalpha/core/pa_ffi.kt
#   android/pa-core-android.aar

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="$ROOT/tools/bindings/pa-core-android"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT="$TARGET_DIR/android"

# Keep in sync with minSdk and ndkVersion in pa-core-android/build.gradle.kts.
MIN_SDK=24
NDK_VERSION=29.0.14206865

profile=release
flavor="${PA_FLAVOR:-store}"
case "$flavor" in
  store | full) ;;
  *) echo "PA_FLAVOR must be store or full, not '$flavor'" >&2; exit 2 ;;
esac
abis=()
for arg in "$@"; do
  case "$arg" in
    --debug) profile=dev ;;
    arm64-v8a | armeabi-v7a | x86_64) abis+=("$arg") ;;
    -h | --help) sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done
[ ${#abis[@]} -gt 0 ] || abis=(arm64-v8a armeabi-v7a x86_64)
variant=$([ "$profile" = dev ] && echo debug || echo release)

rust_target() {
  case "$1" in
    arm64-v8a) echo aarch64-linux-android ;;
    armeabi-v7a) echo armv7-linux-androideabi ;;
    x86_64) echo x86_64-linux-android ;;
  esac
}

sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[ -n "$sdk" ] || { echo "set ANDROID_HOME to the Android SDK" >&2; exit 1; }
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  export ANDROID_NDK_HOME="$sdk/ndk/$NDK_VERSION"
fi
[ -d "$ANDROID_NDK_HOME" ] || {
  echo "NDK not found at $ANDROID_NDK_HOME; install it: sdkmanager \"ndk;$NDK_VERSION\"" >&2
  exit 1
}
command -v cargo-ndk >/dev/null || { echo "cargo-ndk not found: cargo install cargo-ndk --locked" >&2; exit 1; }

cd "$ROOT"
# Installs the toolchain pinned in rust-toolchain.toml if it is missing.
rustup show active-toolchain >/dev/null

rm -rf "$OUT"
mkdir -p "$OUT/jniLibs"
ndk_args=()
for abi in "${abis[@]}"; do
  rustup target add "$(rust_target "$abi")" >/dev/null
  ndk_args+=(-t "$abi")
done

echo "==> cargo-ndk: ${abis[*]} ($profile, $flavor, API $MIN_SDK)"
cargo ndk --platform "$MIN_SDK" "${ndk_args[@]}" -o "$OUT/jniLibs" \
  build -p pa-ffi --lib --locked --profile "$profile" --features "$flavor"

# UniFFI Kotlin bindings in library mode, read from the first Android library
# (metadata symbols are the same for every ABI).
echo "==> Kotlin bindings"
cargo run -q -p pa-ffi --locked --features cli --bin uniffi-bindgen -- \
  generate --library "$OUT/jniLibs/${abis[0]}/libpa_ffi.so" --no-format \
  --language kotlin --out-dir "$OUT/kotlin"

echo "==> AAR ($variant)"
(cd "$PROJECT" && gradle "assemble${variant^}" -PpaGenDir="$OUT" --console=plain)
cp "$PROJECT/build/outputs/aar/pa-core-android-$variant.aar" "$OUT/pa-core-android.aar"
echo "done: $OUT/pa-core-android.aar"
