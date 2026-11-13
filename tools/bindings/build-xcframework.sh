#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-003: build the Rust core (pa-ffi) as PACoreFFI.xcframework and generate
# the UniFFI Swift wrapper for the SwiftPM package tools/bindings/PACore.
#
# Slices (static libraries, one per platform variant):
#   ios       aarch64-apple-ios
#   ios-sim   aarch64-apple-ios-sim + x86_64-apple-ios (universal)
#   tvos      aarch64-apple-tvos            (Rust tier 3: -Zbuild-std)
#   tvos-sim  aarch64-apple-tvos-sim        (Rust tier 3: -Zbuild-std)
#   macos     aarch64-apple-darwin + x86_64-apple-darwin (universal)
#
# Usage: tools/bindings/build-xcframework.sh [--debug] [slice ...]
#   No slices = all of them. Needs macOS with Xcode and rustup.
#
# Output (not committed, ADR-0001):
#   tools/bindings/PACore/PACoreFFI.xcframework
#   tools/bindings/PACore/Sources/PACore/PACore.swift

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PKG="$ROOT/tools/bindings/PACore"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
WORK="$TARGET_DIR/xcframework"

# Keep in sync with `platforms:` in tools/bindings/PACore/Package.swift.
export IPHONEOS_DEPLOYMENT_TARGET=15.0
export TVOS_DEPLOYMENT_TARGET=15.0
export MACOSX_DEPLOYMENT_TARGET=12.0

profile=release
slices=()
for arg in "$@"; do
  case "$arg" in
    --debug) profile=dev ;;
    ios | ios-sim | tvos | tvos-sim | macos) slices+=("$arg") ;;
    -h | --help) sed -n '3,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done
[ ${#slices[@]} -gt 0 ] || slices=(ios ios-sim tvos tvos-sim macos)
profile_dir=$([ "$profile" = dev ] && echo debug || echo release)

[ "$(uname -s)" = Darwin ] || { echo "build-xcframework.sh needs macOS with Xcode" >&2; exit 1; }

slice_targets() {
  case "$1" in
    ios) echo aarch64-apple-ios ;;
    ios-sim) echo aarch64-apple-ios-sim x86_64-apple-ios ;;
    tvos) echo aarch64-apple-tvos ;;
    tvos-sim) echo aarch64-apple-tvos-sim ;;
    macos) echo aarch64-apple-darwin x86_64-apple-darwin ;;
  esac
}

cd "$ROOT"
# Installs the toolchain pinned in rust-toolchain.toml if it is missing.
rustup show active-toolchain >/dev/null
installable="$(rustup target list | awk '{print $1}')"

build_target() {
  local target="$1"
  if grep -qx "$target" <<<"$installable"; then
    rustup target add "$target" >/dev/null
    cargo build -p pa-ffi --lib --locked --profile "$profile" --target "$target"
  else
    # Tier 3 target (tvOS): no prebuilt std, build it from rust-src. Stable
    # toolchain + RUSTC_BOOTSTRAP keeps the pinned compiler version.
    rustup component add rust-src >/dev/null
    RUSTC_BOOTSTRAP=1 cargo build -p pa-ffi --lib --locked --profile "$profile" \
      --target "$target" -Zbuild-std=std,panic_abort
  fi
}

rm -rf "$WORK"
mkdir -p "$WORK/headers"
xcf_args=()
for slice in "${slices[@]}"; do
  libs=()
  for target in $(slice_targets "$slice"); do
    echo "==> $slice: $target ($profile)"
    build_target "$target"
    libs+=("$TARGET_DIR/$target/$profile_dir/libpa_ffi.a")
  done
  mkdir -p "$WORK/$slice"
  lipo -create "${libs[@]}" -output "$WORK/$slice/libpa_ffi.a"
  xcf_args+=(-library "$WORK/$slice/libpa_ffi.a" -headers "$WORK/headers")
done

# UniFFI Swift bindings in library mode, read from a host cdylib.
echo "==> Swift bindings"
cargo build -p pa-ffi --lib --locked --profile "$profile"
cargo run -q -p pa-ffi --locked --features cli --bin uniffi-bindgen -- \
  generate --library "$TARGET_DIR/$profile_dir/libpa_ffi.dylib" --no-format \
  --language swift --out-dir "$WORK/swift"
cp "$WORK/swift/PACoreFFI.h" "$WORK/headers/"
# The xcframework exposes the C module through Headers/module.modulemap.
cp "$WORK/swift/PACoreFFI.modulemap" "$WORK/headers/module.modulemap"
mkdir -p "$PKG/Sources/PACore"
cp "$WORK/swift/PACore.swift" "$PKG/Sources/PACore/PACore.swift"

echo "==> PACoreFFI.xcframework"
rm -rf "$PKG/PACoreFFI.xcframework"
xcodebuild -create-xcframework "${xcf_args[@]}" -output "$PKG/PACoreFFI.xcframework"
echo "done: $PKG"
