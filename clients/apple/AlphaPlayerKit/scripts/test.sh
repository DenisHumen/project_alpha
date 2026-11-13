#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-APL-001: XCTest of AlphaPlayerKit on a simulator or macOS, then the
# licence check of the linked binaries.
#
# Usage: scripts/test.sh ios|tvos|macos [fixtures-dir [qa-media-dir]]
#   fixtures-dir: from scripts/make-fixtures.sh (contract suite on MPVEngine)
#   qa-media-dir: from scripts/make-qa-media.py (QA-002 MKV playback)
#   Without them the media tests are skipped.
# Needs PACoreFFI.xcframework: tools/bindings/build-xcframework.sh first.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pkg="$(cd "$here/.." && pwd)"
repo="$(cd "$pkg/../../.." && pwd)"
[ -d "$repo/tools/bindings/PACore/PACoreFFI.xcframework" ] \
  || { echo "run tools/bindings/build-xcframework.sh first" >&2; exit 1; }

platform="${1:-}"
fixtures="${2:-}"
qa_media="${3:-}"

simulator() {
  # Newest runtime of the platform, first available device on it.
  xcrun simctl list devices available -j | jq -r --arg p "$1" '
    [.devices | to_entries[]
      | select(.key | test("SimRuntime\\." + $p + "-"))
      | select(.value | length > 0)]
    | sort_by(.key | capture("-(?<v>[0-9-]+)$").v | split("-") | map(tonumber))
    | last | .value[0] | "\(.udid) \(.name)"'
}

case "$platform" in
  macos) destination="platform=macOS" ;;
  ios | tvos)
    name=$([ "$platform" = ios ] && echo iOS || echo tvOS)
    read -r udid device < <(simulator "$name")
    [ -n "$udid" ] && [ "$udid" != null ] || { echo "no $name simulator available" >&2; exit 1; }
    echo "==> $name simulator: $device ($udid)"
    destination="id=$udid"
    ;;
  *) echo "usage: $0 ios|tvos|macos [fixtures-dir [qa-media-dir]]" >&2; exit 2 ;;
esac

"$here/check-license.sh" manifest "$pkg/Package.swift"

derived="${PA_DERIVED_DATA:-$repo/target/alphaplayerkit-$platform}"
# Simulator tests get TEST_RUNNER_<NAME> as <NAME>; macOS tests see the
# environment directly.
export TEST_RUNNER_PA_PLAYER_FIXTURES="$fixtures" PA_PLAYER_FIXTURES="$fixtures"
export TEST_RUNNER_PA_QA_MEDIA="$qa_media" PA_QA_MEDIA="$qa_media"

cd "$pkg"
scheme=AlphaPlayerKit
xcodebuild -list | grep -qx '[[:space:]]*AlphaPlayerKit-Package' && scheme=AlphaPlayerKit-Package
args=(-scheme "$scheme" -destination "$destination" -derivedDataPath "$derived" -skipPackagePluginValidation)
[ -n "${PA_SPM_CACHE:-}" ] && args+=(-clonedSourcePackagesDirPath "$PA_SPM_CACHE")
xcodebuild test "${args[@]}"

"$here/check-license.sh" binary "$derived/Build/Products"
