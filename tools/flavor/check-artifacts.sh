#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Store/full artifact check (ADR-0007, CORE-013).
#
#   tools/flavor/check-artifacts.sh store PATH...   # fails on any full-only hit
#   tools/flavor/check-artifacts.sh full  PATH...   # positive control: fails
#                                                   # if nothing full-only is seen
#
# PATH is a binary (ELF, Mach-O, PE, static archive), a directory (an
# .xcframework, an unpacked bundle), a zip (.apk, .aab, .ipa, .zip) or a tar
# archive; archives and directories are scanned recursively. Every binary is
# grepped with `strings -a` and `nm -C` for the fixed strings of
# tools/flavor/full-only-patterns.txt (override: PA_FULL_ONLY_PATTERNS).
#
# The full mode proves that the check sees full-only code in that kind of
# artifact at all, so a store pass is not an artefact of stripping or
# packaging. Portable: bash 3.2 (macOS), GNU and BSD userland.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
patterns_file="${PA_FULL_ONLY_PATTERNS:-$here/full-only-patterns.txt}"

usage() {
  sed -n '4,8p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

[ $# -ge 2 ] || usage
mode="$1"
shift
case "$mode" in
  store | full) ;;
  *) usage ;;
esac
command -v strings >/dev/null || { echo "error: strings not found (binutils)" >&2; exit 2; }
have_nm=false
command -v nm >/dev/null && have_nm=true

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
pat="$work/patterns"
grep -v '^[[:space:]]*#' "$patterns_file" | grep -v '^[[:space:]]*$' >"$pat" || true
[ -s "$pat" ] || { echo "error: no patterns in $patterns_file" >&2; exit 2; }
: >"$work/scanned"
: >"$work/hits"

magic() { LC_ALL=C od -An -tx1 -N4 "$1" 2>/dev/null | tr -d ' \n'; }

is_binary() {
  case "$(magic "$1")" in
    # ELF; Mach-O 32/64 both endians; universal Mach-O; ar (static libs); PE.
    7f454c46 | cffaedfe | cefaedfe | feedfacf | feedface | cafebabe | 213c6172 | 4d5a*) return 0 ;;
  esac
  return 1
}

scan_binary() {
  local file="$1" label="$2" out="$work/out"
  {
    strings -a "$file" 2>/dev/null || true
    if $have_nm; then nm -C "$file" 2>/dev/null || true; fi
  } >"$out"
  echo "$label" >>"$work/scanned"
  grep -F -o -f "$pat" "$out" 2>/dev/null | sort -u | while IFS= read -r hit; do
    printf '%s\t%s\n' "$label" "$hit" >>"$work/hits"
  done
}

scan_path() {
  local path="$1" label="$2" dir
  if [ -d "$path" ]; then
    local list
    list="$(mktemp "$work/list.XXXXXX")"
    find "$path" -type f | LC_ALL=C sort >"$list"
    while IFS= read -r f; do
      scan_path "$f" "$label${f#"$path"}"
    done <"$list"
    rm -f "$list"
    return
  fi
  case "$path" in
    *.tar | *.tar.gz | *.tgz | *.tar.xz | *.tar.zst)
      dir="$(mktemp -d "$work/x.XXXXXX")"
      tar -xf "$path" -C "$dir"
      scan_path "$dir" "$label!"
      return
      ;;
  esac
  if [ "$(magic "$path")" = 504b0304 ]; then
    dir="$(mktemp -d "$work/x.XXXXXX")"
    unzip -q -o "$path" -d "$dir"
    scan_path "$dir" "$label!"
  elif is_binary "$path"; then
    scan_binary "$path" "$label"
  fi
}

for p in "$@"; do
  [ -e "$p" ] || { echo "error: $p does not exist" >&2; exit 2; }
  scan_path "$p" "$p"
done

scanned=$(wc -l <"$work/scanned" | tr -d ' ')
hits=$(wc -l <"$work/hits" | tr -d ' ')
if [ "$scanned" -eq 0 ]; then
  echo "error: no binaries found in: $*" >&2
  exit 1
fi
echo "Scanned $scanned binaries ($mode check, $(wc -l <"$pat" | tr -d ' ') patterns, nm: $have_nm)."
if [ "$hits" -gt 0 ]; then
  echo "Full-only strings and symbols found:"
  sort -u "$work/hits" | sed 's/^/  /'
fi
if [ "$mode" = store ] && [ "$hits" -gt 0 ]; then
  echo "error: store artifact contains full-only code (ADR-0007); see docs/dev/flavors.md" >&2
  exit 1
fi
if [ "$mode" = full ] && [ "$hits" -eq 0 ]; then
  echo "error: no full-only string or symbol found: not a full build, or the check cannot see into this artifact" >&2
  exit 1
fi
echo "OK: $mode artifact."
