#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Corresponding source for the LGPL libsmb2 shipped in the app (LIB-019): the
# libsmb2.so of every Android ABI in the APKs and libsmb2.so.1 of the Linux
# tar.gz/AppImage. The counterpart of lgpl-sources.sh (libmpv/FFmpeg).
#
#   libsmb2-sources.sh <out.tar.xz> <notice.md> <libsmb2-artifact>...
#
# <libsmb2-artifact> are the tools/libsmb2 outputs that went into the app (CI
# artifacts libsmb2-<target>, each with BUILDINFO.txt). The source bundle is
# PA_LIBSMB2_SOURCES (CI artifact libsmb2-sources, from `tools/libsmb2/build.sh
# sources` in the same run) or, when unset, built here by that command. It
# holds the upstream tarball, our recipe (tools/libsmb2) and patches. Every
# source hash in each artifact's BUILDINFO.txt must be in the bundle's
# SHA256SUMS. <notice.md> gets a short Markdown notice for the release.
set -euo pipefail

die() { printf 'libsmb2-sources.sh: %s\n' "$*" >&2; exit 1; }
[ $# -ge 3 ] || { sed -n '4,16p' "$0" >&2; exit 2; }
out="$1"; notice="$2"; shift 2

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

src="${PA_LIBSMB2_SOURCES:-}"
if [ -z "$src" ]; then
    PA_SMB2_WORK="$work/build" PA_SMB2_OUT="$work/out" "$repo_root/tools/libsmb2/build.sh" sources
    src="$(find "$work/out" -name 'project_alpha-libsmb2-sources-*.tar.xz' | head -n1)"
fi
[ -f "$src" ] || die "no source bundle: '$src'"

tar -xJf "$src" -C "$work" --wildcards '*/upstream/SHA256SUMS'
sums="$(find "$work" -path '*/upstream/SHA256SUMS' | head -n1)"
[ -f "$sums" ] || die "$src has no upstream/SHA256SUMS"

targets=()
for a in "$@"; do
    info="$a/BUILDINFO.txt"
    [ -f "$info" ] || die "$a: no BUILDINFO.txt (not a tools/libsmb2 artifact)"
    grep -q '^project_alpha LGPL libsmb2 build' "$info" || die "$a: not a tools/libsmb2 artifact"
    t="$(sed -n 's/^target: //p' "$info")"
    n=0
    while IFS= read -r h; do
        grep -q "^$h " "$sums" || die "$t: source sha256 $h from BUILDINFO.txt is not in $(basename "$src")"
        n=$((n + 1))
    done < <(grep -oE 'sha256=[0-9a-f]{64}' "$info" | cut -d= -f2)
    [ "$n" -gt 0 ] || die "$t: BUILDINFO.txt records no source hash"
    targets+=("$t")
done

mkdir -p "$(dirname "$out")"
cp "$src" "$out"
printf 'wrote %s (%s)\n' "$out" "${targets[*]}" >&2

first="$1/BUILDINFO.txt"
{
    echo "### libsmb2 (LGPL-2.1-or-later)"
    echo
    echo "SMB client library, our shared build from [\`tools/libsmb2\`](https://github.com/DenisHumen/project_alpha/tree/main/tools/libsmb2)"
    echo "(no Kerberos, no other dependencies); the Rust core loads it at run time for SMB sources."
    echo "Version: $(sed -nE 's/^libsmb2 ([^ ]+) sha256=.*/\1/p' "$first")."
    echo "Targets: $(printf '`%s`, ' "${targets[@]}" | sed 's/, $//')."
    echo "Corresponding sources (upstream tarball, build recipe and patches): \`$(basename "$out")\` in this release."
    echo "To relink, rebuild with \`tools/libsmb2/build.sh <target>\` and replace the library"
    echo "(Android: \`lib/<abi>/libsmb2.so\` in the APK, then re-sign it; Linux: \`lib/libsmb2.so.1\` of the bundle)."
} > "$notice"
