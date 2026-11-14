#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-APL-001: licence gate for the Apple player (CLAUDE.md, ADR-0003,
# ADR-0005). Two profiles:
#
#   dev (default) — development and CI builds. MPVKit is allowed, LGPL
#     product only. MPVKit's FFmpeg is LGPL-3.0 (--enable-version3), so a dev
#     build is NOT a release build.
#   --store — App Store / release builds: LGPL-2.1 only. MPVKit fails here
#     until PLAY-APL-013 replaces it with our LGPL-2.1 FFmpeg + libmpv.
#
#   check-license.sh manifest [--store] [Package.swift]
#       dev:   the package depends on the product `MPVKit` at an exact
#              version, and nothing (outside comments) names an `-GPL` product.
#       store: additionally, no dependency on MPVKit at all.
#   check-license.sh binary [--store] <dir>
#       Scans every Mach-O file under <dir> (built products).
#       dev:   libmpv must say `-Dgpl=false`; `-Dgpl=true`, FFmpeg
#              `--enable-gpl` / `--enable-nonfree` or a "GPL version" licence
#              string (not LGPL) fail.
#       store: additionally `--enable-version3` or "LGPL version 3" fail.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

fail() { echo "::error::licence check: $*" >&2; exit 1; }

store=0

manifest() {
  local file="${1:-$here/../Package.swift}"
  [ -f "$file" ] || fail "$file not found"
  # Drop // comments (at line start or after whitespace, so URLs survive),
  # then inspect the code only.
  local code
  code="$(sed -E 's#(^|[[:space:]])//.*$##' "$file")"
  if grep -n -- '-GPL' <<<"$code"; then
    fail "$file names a GPL product of MPVKit"
  fi
  if [ "$store" = 1 ]; then
    if grep -nqi 'mpvkit' <<<"$code"; then
      fail "store profile: $file uses MPVKit, whose FFmpeg is LGPL-3.0 (--enable-version3); release builds need our LGPL-2.1 FFmpeg + libmpv (PLAY-APL-013)"
    fi
    echo "manifest (store): no MPVKit ($file)"
    return
  fi
  grep -Eq '\.package\(url: *"https://github.com/mpvkit/MPVKit(\.git)?", *exact: *"[^"]+"\)' <<<"$code" \
    || fail "$file must pin MPVKit with exact:"
  grep -Eq '\.product\(name: *"MPVKit", *package: *"MPVKit"\)' <<<"$code" \
    || fail "$file must depend on the product MPVKit"
  echo "manifest (dev): MPVKit LGPL product, exact version ($file) — not a release build (LGPL-3.0 FFmpeg)"
}

binary() {
  local dir="${1:?usage: $0 binary [--store] <dir>}" found_lgpl=0 scanned=0 file strs
  local forbidden='-Dgpl=true|--enable-gpl|--enable-nonfree|(^|[^L])GPL version [23]'
  [ "$store" = 1 ] && forbidden="$forbidden|--enable-version3|LGPL version 3"
  [ -d "$dir" ] || fail "$dir not found"
  while IFS= read -r -d '' file; do
    file -b "$file" | grep -q 'Mach-O' || continue
    scanned=$((scanned + 1))
    strs="$(strings -a "$file")"
    if grep -q -- '-Dgpl=false' <<<"$strs"; then found_lgpl=1; fi
    if grep -n -o -E -- ".{0,60}($forbidden).{0,40}" <<<"$strs" | head -5; then
      fail "$([ "$store" = 1 ] && echo "store profile: ")forbidden licence markers in $file"
    fi
  done < <(find "$dir" -type f -print0)
  [ "$scanned" -gt 0 ] || fail "no Mach-O files under $dir"
  [ "$found_lgpl" = 1 ] || fail "no libmpv with -Dgpl=false linked under $dir"
  echo "binary ($([ "$store" = 1 ] && echo store || echo dev)): $scanned Mach-O files, libmpv -Dgpl=false, no forbidden markers ($dir)"
}

cmd="${1:-}"
shift || true
if [ "${1:-}" = --store ]; then
  store=1
  shift
fi
case "$cmd" in
  manifest) manifest "$@" ;;
  binary) binary "$@" ;;
  *) echo "usage: $0 manifest [--store] [Package.swift] | binary [--store] <dir>" >&2; exit 2 ;;
esac
