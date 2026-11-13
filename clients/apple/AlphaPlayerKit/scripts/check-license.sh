#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-APL-001: MPVKit may only be used in its LGPL build (CLAUDE.md,
# ADR-0003). Two gates:
#
#   check-license.sh manifest [Package.swift]
#       The package depends on the product `MPVKit` of MPVKit at an exact
#       version, and nothing (outside comments) names an `-GPL` product.
#   check-license.sh binary <dir>
#       Scans every Mach-O file under <dir> (built products): libmpv must say
#       `-Dgpl=false`; `-Dgpl=true`, FFmpeg `--enable-gpl` / `--enable-nonfree`
#       or a "GPL version" licence string (not LGPL) fail the check.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

fail() { echo "::error::licence check: $*" >&2; exit 1; }

manifest() {
  local file="${1:-$here/../Package.swift}"
  [ -f "$file" ] || fail "$file not found"
  # Drop // comments, then inspect the code only.
  local code
  code="$(sed -e 's#//.*$##' "$file")"
  if grep -n -- '-GPL' <<<"$code"; then
    fail "$file names a GPL product of MPVKit"
  fi
  grep -Eq '\.package\(url: *"https://github.com/mpvkit/MPVKit(\.git)?", *exact: *"[^"]+"\)' <<<"$code" \
    || fail "$file must pin MPVKit with exact:"
  grep -Eq '\.product\(name: *"MPVKit", *package: *"MPVKit"\)' <<<"$code" \
    || fail "$file must depend on the product MPVKit"
  echo "manifest: MPVKit LGPL product, exact version ($file)"
}

binary() {
  local dir="${1:?usage: $0 binary <dir>}" found_lgpl=0 scanned=0 file strs
  [ -d "$dir" ] || fail "$dir not found"
  while IFS= read -r -d '' file; do
    file -b "$file" | grep -q 'Mach-O' || continue
    scanned=$((scanned + 1))
    strs="$(strings -a "$file")"
    if grep -q -- '-Dgpl=false' <<<"$strs"; then found_lgpl=1; fi
    if grep -n -E -- '-Dgpl=true|--enable-gpl|--enable-nonfree|(^|[^L])GPL version [23]' <<<"$strs" | head -5; then
      fail "GPL build markers in $file"
    fi
  done < <(find "$dir" -type f -print0)
  [ "$scanned" -gt 0 ] || fail "no Mach-O files under $dir"
  [ "$found_lgpl" = 1 ] || fail "no libmpv with -Dgpl=false linked under $dir"
  echo "binary: $scanned Mach-O files, libmpv -Dgpl=false, no GPL markers ($dir)"
}

case "${1:-}" in
  manifest) shift; manifest "$@" ;;
  binary) shift; binary "$@" ;;
  *) echo "usage: $0 manifest [Package.swift] | binary <dir>" >&2; exit 2 ;;
esac
