#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-003: run the PACore XCTest (`coreVersion()` over UniFFI) with xcodebuild.
# CORE-015: on ios/tvos also the Keychain secret store across a process
# restart (two more xcodebuild runs on the same simulator).
# Usage: tools/bindings/test-pacore.sh macos|ios|tvos
#   ios/tvos run on the newest available simulator of that platform.
# Run tools/bindings/build-xcframework.sh first.

set -euo pipefail

PKG="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/PACore"
[ -d "$PKG/PACoreFFI.xcframework" ] || { echo "run tools/bindings/build-xcframework.sh first" >&2; exit 1; }

simulator() {
  # Newest runtime of the platform, first available device on it.
  xcrun simctl list devices available -j | jq -r --arg p "$1" '
    [.devices | to_entries[]
      | select(.key | test("SimRuntime\\." + $p + "-"))
      | select(.value | length > 0)]
    | sort_by(.key | capture("-(?<v>[0-9-]+)$").v | split("-") | map(tonumber))
    | last | .value[0] | "\(.udid) \(.name)"'
}

case "${1:-}" in
  macos) destination="platform=macOS" ;;
  ios | tvos)
    platform=$([ "$1" = ios ] && echo iOS || echo tvOS)
    read -r udid name < <(simulator "$platform")
    [ -n "$udid" ] && [ "$udid" != null ] || { echo "no $platform simulator available" >&2; exit 1; }
    echo "==> $platform simulator: $name ($udid)"
    destination="id=$udid"
    ;;
  *) echo "usage: $0 macos|ios|tvos" >&2; exit 2 ;;
esac

cd "$PKG"
# SwiftPM names the scheme with all targets `<name>-Package` when there are several.
scheme=PACore
xcodebuild -list | grep -qx '[[:space:]]*PACore-Package' && scheme=PACore-Package
xcodebuild test -scheme "$scheme" -destination "$destination" -skipPackagePluginValidation

if [ "$1" != macos ]; then
  # Secrets written by one test process are read by the next one.
  for step in testAWriteBeforeRestart testBReadAfterRestart; do
    echo "==> KeychainRestartTests/$step (own process)"
    xcodebuild test -scheme "$scheme" -destination "$destination" -skipPackagePluginValidation \
      -only-testing:"PACoreTests/KeychainRestartTests/$step"
  done
fi
