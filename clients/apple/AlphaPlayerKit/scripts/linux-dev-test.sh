#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-APL-001: run the AlphaPlayerKit tests on Linux against the distro
# libmpv, for contributors without a Mac. Not a substitute for the Apple CI
# (scripts/test.sh): different mpv version, no MPVKit, no LGPL check.
#
# Usage (repo root, Docker):
#   scripts/linux-dev-test.sh docker [swift test args]
# or inside a container/host with swift, libmpv-dev, ffmpeg and cargo:
#   scripts/linux-dev-test.sh run [swift test args]
#
# Builds pa-ffi (cdylib) and its Swift bindings, assembles a temporary
# SwiftPM package (PACore + a `Libmpv` system module + the sources and tests
# of this package), makes the fixtures and runs `swift test`.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pkg="$(cd "$here/.." && pwd)"
repo="$(cd "$pkg/../../.." && pwd)"
target="${CARGO_TARGET_DIR:-$repo/target}"
work="${PA_LINUX_WORK:-$target/alphaplayerkit-linux}"
image="${PA_SWIFT_IMAGE:-swift:6.1-noble}"

docker_run() {
  # Rust is built on the host (CARGO_TARGET_DIR is mounted), Swift in Docker.
  run_host_rust
  docker run --rm -v "$repo:$repo" -v "$target:$target" -w "$repo" \
    -e CARGO_TARGET_DIR="$target" -e PA_LINUX_WORK="$work" -e PA_SKIP_RUST=1 \
    "$image" bash -c "apt-get update -qq >/dev/null && \
      DEBIAN_FRONTEND=noninteractive apt-get install -y -qq libmpv-dev ffmpeg >/dev/null && \
      $here/linux-dev-test.sh run $*"
}

run_host_rust() {
  cargo build -p pa-ffi --lib --locked
  cargo run -q -p pa-ffi --locked --features cli --bin uniffi-bindgen -- \
    generate --library "$target/debug/libpa_ffi.so" --no-format --language swift --out-dir "$work/bindings"
}

run() {
  [ -n "${PA_SKIP_RUST:-}" ] || run_host_rust
  local p="$work/pkg"
  rm -rf "$p"
  mkdir -p "$p/Sources/PACoreFFI" "$p/Sources/PACore" "$p/Sources/Libmpv" "$p/Tests"
  cp "$work/bindings/PACoreFFI.h" "$p/Sources/PACoreFFI/"
  cat >"$p/Sources/PACoreFFI/module.modulemap" <<'EOF'
module PACoreFFI {
    header "PACoreFFI.h"
    link "pa_ffi"
    export *
}
EOF
  cp "$work/bindings/PACore.swift" "$p/Sources/PACore/"
  cat >"$p/Sources/Libmpv/shim.h" <<'EOF'
#include <mpv/client.h>
EOF
  cat >"$p/Sources/Libmpv/module.modulemap" <<'EOF'
module Libmpv [system] {
    header "shim.h"
    link "mpv"
    export *
}
EOF
  ln -s "$pkg/Sources/AlphaPlayerKit" "$p/Sources/AlphaPlayerKit"
  ln -s "$pkg/Tests/AlphaPlayerKitTests" "$p/Tests/AlphaPlayerKitTests"
  cat >"$p/Package.swift" <<'EOF'
// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "AlphaPlayerKitLinux",
    targets: [
        .systemLibrary(name: "PACoreFFI", path: "Sources/PACoreFFI"),
        .target(name: "PACore", dependencies: ["PACoreFFI"], path: "Sources/PACore"),
        .systemLibrary(name: "Libmpv", path: "Sources/Libmpv"),
        .target(name: "AlphaPlayerKit", dependencies: ["PACore", "Libmpv"], path: "Sources/AlphaPlayerKit"),
        .testTarget(name: "AlphaPlayerKitTests", dependencies: ["AlphaPlayerKit", "PACore"], path: "Tests/AlphaPlayerKitTests"),
    ]
)
EOF
  "$here/make-fixtures.sh" "$work/fixtures" >/dev/null
  cd "$p"
  PA_PLAYER_FIXTURES="$work/fixtures" LD_LIBRARY_PATH="$target/debug" \
    swift test -Xlinker -L"$target/debug" "$@"
}

case "${1:-}" in
  docker) shift; docker_run "$@" ;;
  run) shift; run "$@" ;;
  *) echo "usage: $0 docker|run [swift test args]" >&2; exit 2 ;;
esac
