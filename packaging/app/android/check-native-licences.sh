#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Licence gate for native libraries inside release APKs (R01-REL).
#
#   check-native-licences.sh <report.md> <apk>...
#
# Every lib/<abi>/*.so in the APKs is scanned statically:
#   - FFmpeg code (in libav*.so or linked statically into libmpv.so) must embed
#     "license: LGPL version 2.1 or later" or "... 3 or later", and its embedded
#     configure line must not contain --enable-gpl or --enable-nonfree;
#   - libmpv must be configured with -Dgpl=false.
# Anything else fails the release. A Markdown summary goes to <report.md>;
# the last line on stdout is "libmpv=yes" or "libmpv=no".
set -euo pipefail

[ $# -ge 2 ] || { sed -n '4,15p' "$0" >&2; exit 2; }
report="$1"; shift

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0
has_mpv=no
bad() { printf 'licence gate: FAIL: %s\n' "$*" >&2; fail=1; }

{
    echo "| APK | Library | Component | Licence (embedded) |"
    echo "|---|---|---|---|"
} > "$report"

declare -A seen=()
for apk in "$@"; do
    a="$work/$(basename "$apk" .apk)"
    mkdir -p "$a"
    python3 -I - "$apk" "$a" <<'PY'
import sys, zipfile
apk, dest = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk) as z:
    for n in z.namelist():
        if n.startswith("lib/") and n.endswith(".so") and ".." not in n:
            z.extract(n, dest)
PY
    while IFS= read -r -d '' so; do
        rel="${so#"$a"/}"
        lib="$(basename "$so")"
        lic="$(LC_ALL=C grep -aoE 'lib(avutil|avcodec|avformat|avfilter|swscale|swresample|avdevice) license: [A-Za-z0-9 .]+' "$so" | sed 's/.*license: //' | sort -u | paste -sd ';' -)" || true
        mpvcfg="$(LC_ALL=C grep -aoE -- '-Dgpl=(true|false)' "$so" | sort -u | paste -sd ';' -)" || true
        comp=""
        if [ -n "$lic" ]; then
            comp="FFmpeg"
            if printf '%s\n' "$lic" | tr ';' '\n' | grep -Eqv '^LGPL version (2\.1|3) or later$'; then
                bad "$apk:$rel embeds FFmpeg licence '$lic'"
            fi
            if LC_ALL=C grep -aqE -- '--enable-(gpl|nonfree)( |$)' "$so"; then
                bad "$apk:$rel was configured with --enable-gpl or --enable-nonfree"
            fi
        fi
        if [ -n "$mpvcfg" ] || [ "$lib" = libmpv.so ]; then
            has_mpv=yes
            comp="${comp:+$comp + }mpv ($mpvcfg)"
            [ "$mpvcfg" = "-Dgpl=false" ] || bad "$apk:$rel: libmpv is not built with -Dgpl=false ('${mpvcfg:-unknown}')"
        fi
        key="$lib|$comp|$lic"
        if [ -n "$comp" ] && [ -z "${seen[$key]:-}" ]; then
            seen[$key]=1
            echo "| $(basename "$apk") | \`$rel\` | $comp | ${lic:-n/a} |" >> "$report"
        fi
    done < <(find "$a" -name '*.so' -type f -print0 | sort -z)
done

[ "$fail" = 0 ] || exit 1
printf 'licence gate: ok\n' >&2
echo "libmpv=$has_mpv"
