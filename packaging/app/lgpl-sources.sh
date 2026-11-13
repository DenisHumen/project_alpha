#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Corresponding source for the LGPL libraries shipped in the app (R01-REL,
# PLAY-013): libmpv and FFmpeg from tools/mpv in the Android APKs and the Linux
# tar.gz/AppImage.
#
#   lgpl-sources.sh <out.tar.xz> <notice.md> <libmpv-artifact>...
#
# <libmpv-artifact> are the tools/mpv outputs that went into the app (CI
# artifacts libmpv-<target>, each with BUILDINFO.txt). The source bundle is
# PA_LIBMPV_SOURCES (CI artifact libmpv-sources, from `tools/mpv/build.sh
# sources` in the same run) or, when unset, built here by that command. It holds
# every upstream tarball, both recipes (tools/ffmpeg, tools/mpv) and their
# patches. Every source hash recorded in each artifact's BUILDINFO.txt must be
# in the bundle's SHA256SUMS, so the bundle is exactly what the binaries were
# built from. The bundle is attached to the GitHub Release next to the binaries
# (LGPL-2.1 §6(d): equivalent access to the source "from the same place").
# <notice.md> gets a human-readable list of components, versions and licences.
set -euo pipefail

die() { printf 'lgpl-sources.sh: %s\n' "$*" >&2; exit 1; }
[ $# -ge 3 ] || { sed -n '4,19p' "$0" >&2; exit 2; }
out="$1"; notice="$2"; shift 2

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

src="${PA_LIBMPV_SOURCES:-}"
if [ -z "$src" ]; then
    PA_MPV_WORK="$work/build" PA_MPV_OUT="$work/out" "$repo_root/tools/mpv/build.sh" sources
    src="$(find "$work/out" -name 'project_alpha-libmpv-sources-*.tar.xz' | head -n1)"
fi
[ -f "$src" ] || die "no source bundle: '$src'"

tar -xJf "$src" -C "$work" --wildcards '*/upstream/SHA256SUMS'
sums="$(find "$work" -path '*/upstream/SHA256SUMS' | head -n1)"
[ -f "$sums" ] || die "$src has no upstream/SHA256SUMS"

targets=()
for a in "$@"; do
    info="$a/BUILDINFO.txt"
    [ -f "$info" ] || die "$a: no BUILDINFO.txt (not a tools/mpv artifact)"
    t="$(sed -n 's/^target: //p' "$info")"
    while IFS= read -r h; do
        grep -q "^$h " "$sums" || die "$t: source sha256 $h from BUILDINFO.txt is not in $(basename "$src")"
    done < <(grep -oE 'sha256=[0-9a-f]{64}' "$info" | cut -d= -f2)
    targets+=("$t")
done

mkdir -p "$(dirname "$out")"
cp "$src" "$out"
printf 'wrote %s (%s)\n' "$out" "${targets[*]}" >&2

# Component table from the first artifact (all targets share the pins; the
# hash check above guarantees they match the bundle).
first="$1/BUILDINFO.txt"
{
    echo "### libmpv / FFmpeg (LGPL-2.1-or-later)"
    echo
    echo "Our own build from [\`tools/mpv\`](https://github.com/DenisHumen/project_alpha/tree/main/tools/mpv)"
    echo "(mpv \`-Dgpl=false\`, FFmpeg without GPL/nonfree/version3 code), shared libraries loaded by the app."
    echo "Targets: $(printf '`%s` ' "${targets[@]}")."
    echo "Corresponding sources (all upstream tarballs, build recipes and patches): \`$(basename "$out")\` in this release."
    echo "To relink, rebuild with \`tools/mpv/build.sh <target>\` and replace the libraries"
    echo "(Android: \`lib/<abi>/*.so\` in the APK, then re-sign it; Linux: \`lib/\` of the bundle)."
    echo
    echo "| Component | Version | Licence |"
    echo "|---|---|---|"
    sed -n '/^components:/,/^$/p' "$first" \
        | sed -nE 's/^  ([A-Za-z0-9_]+) ([^ ]+) \((.*)\) sha256=.*/| \1 | \2 | \3 |/p'
    echo
    echo "| Target | FFmpeg licence |"
    echo "|---|---|"
    for a in "$@"; do
        echo "| $(sed -n 's/^target: //p' "$a/BUILDINFO.txt") | $(sed -n 's/^ffmpeg licence: //p' "$a/BUILDINFO.txt") |"
    done
} > "$notice"
