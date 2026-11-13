#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Regenerates docs/screenshots/0.1 from the widget-test renderer (demo library,
# art drawn in code). Needs Flutter and ImageMagick (`convert`).
# PNGs are quantised to 256 colours and TV shots scaled to 1440x810 so each
# file stays well under 300 KB.
set -euo pipefail
app="$(cd "$(dirname "$0")/.." && pwd)"
out="$(cd "$app/../../.." && pwd)/docs/screenshots/0.1"
raw="$(mktemp -d)"
trap 'rm -rf "$raw"' EXIT

cd "$app"
KINOLUNE_SCREENSHOTS_DIR="$raw" nice flutter test -j 1 test/tool/screenshots_test.dart

mkdir -p "$out"
for f in "$raw"/*.png; do
  name="$(basename "$f")"
  resize=()
  [[ $name == tv-* ]] && resize=(-resize 1440x810)
  [[ $name == phone-* ]] && resize=(-resize 585x1266)
  convert "$f" "${resize[@]}" -dither FloydSteinberg -colors 256 "PNG8:$out/$name"
done
ls -l "$out"
