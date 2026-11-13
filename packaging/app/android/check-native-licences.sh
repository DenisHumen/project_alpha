#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Licence gate for native libraries inside release APKs (R01-REL, PLAY-013).
#
#   check-native-licences.sh <report.md> <apk>...
#
# Every lib/<abi>/*.so in the APKs is scanned statically. The APK must carry
# our LGPL-2.1 libmpv from tools/mpv (via the local media_kit_libs_android_video
# override), never the upstream prebuilt jars:
#   - FFmpeg code may only live in the shared libav*/libsw* libraries, must embed
#     "license: LGPL version 2.1 or later", and its embedded configure line must
#     not contain --enable-gpl, --enable-nonfree or --enable-version3;
#   - libmpv.so must be configured with -Dgpl=false and must not have FFmpeg
#     linked in statically (that is the upstream LGPL-3.0 build);
#   - an ABI directory with libmpv.so holds the whole tools/mpv set:
#     libmpv.so, libav{codec,filter,format,util}.so, libsw{resample,scale}.so,
#     libmediakitandroidhelper.so; FFmpeg/mpv libraries outside it are rejected.
# Anything else fails the release. A Markdown summary goes to <report.md>
# (every native library per APK and ABI); the last line on stdout is
# "libmpv=yes" when every APK and ABI carries the set, otherwise "libmpv=no".
set -euo pipefail

[ $# -ge 2 ] || { sed -n '4,22p' "$0" >&2; exit 2; }
report="$1"; shift

# Keep in sync with tools/mpv/bundle.sh and
# clients/flutter/packages/media_kit_libs_android_video/android/build.gradle.
MPV_SET="libmpv.so libavcodec.so libavfilter.so libavformat.so libavutil.so libswresample.so libswscale.so libmediakitandroidhelper.so"
# Names that may only appear as part of that set.
MPV_SET_RE='^lib(mpv|av[a-z]*|sw[a-z]*|postproc|mediakitandroidhelper)\.so$'

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail=0
all_mpv=yes
bad() { printf 'licence gate: FAIL: %s\n' "$*" >&2; fail=1; }

{
    echo "| APK | Library | Component | Licence (embedded) |"
    echo "|---|---|---|---|"
} > "$report"
inventory=""

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
    if [ ! -d "$a/lib" ]; then
        bad "$apk has no native libraries"; all_mpv=no; continue
    fi
    for abidir in "$a"/lib/*/; do
        abi="$(basename "$abidir")"
        libs="$(cd "$abidir" && ls -- *.so 2>/dev/null | sort | tr '\n' ' ')"
        inventory="$inventory| $(basename "$apk") | $abi | ${libs% } |"$'\n'
        if [ -f "$abidir/libmpv.so" ]; then
            for want in $MPV_SET; do
                [ -f "$abidir/$want" ] || bad "$apk:lib/$abi: libmpv.so without $want (not the tools/mpv set)"
            done
        else
            all_mpv=no
        fi
        for lib in $libs; do
            if printf '%s\n' "$lib" | grep -Eq "$MPV_SET_RE"; then
                case " $MPV_SET " in
                    *" $lib "*) ;;
                    *) bad "$apk:lib/$abi/$lib is an FFmpeg/mpv library outside the tools/mpv set" ;;
                esac
            fi
        done
    done
    while IFS= read -r -d '' so; do
        rel="${so#"$a"/}"
        lib="$(basename "$so")"
        lic="$(LC_ALL=C grep -aoE 'lib(avutil|avcodec|avformat|avfilter|swscale|swresample|avdevice|postproc) license: [A-Za-z0-9 .]+' "$so" | sed 's/.*license: //' | sort -u | paste -sd ';' -)" || true
        mpvcfg="$(LC_ALL=C grep -aoE -- '-Dgpl=(true|false)' "$so" | sort -u | paste -sd ';' -)" || true
        comp=""
        if [ -n "$lic" ]; then
            comp="FFmpeg"
            case "$lib" in
                libav*.so|libsw*.so) ;;
                *) bad "$apk:$rel has FFmpeg linked in statically (upstream prebuilt libmpv?)" ;;
            esac
            if [ "$lic" != "LGPL version 2.1 or later" ]; then
                bad "$apk:$rel embeds FFmpeg licence '$lic' (expected 'LGPL version 2.1 or later')"
            fi
            if LC_ALL=C grep -aqE -- '--enable-(gpl|nonfree|version3)( |$)' "$so"; then
                bad "$apk:$rel was configured with --enable-gpl, --enable-nonfree or --enable-version3"
            fi
        fi
        if [ -n "$mpvcfg" ] || [ "$lib" = libmpv.so ]; then
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

{
    echo
    echo "All native libraries:"
    echo
    echo "| APK | ABI | Libraries |"
    echo "|---|---|---|"
    printf '%s' "$inventory"
} >> "$report"

[ "$fail" = 0 ] || exit 1
printf 'licence gate: ok\n' >&2
echo "libmpv=$all_mpv"
