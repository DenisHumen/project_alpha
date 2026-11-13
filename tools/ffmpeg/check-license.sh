#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Licence gate for the FFmpeg build (PLAY-001, used by CI-008).
#
#   check-license.sh --config <ffmpeg-build-dir>   check a configured tree (config.h)
#   check-license.sh --artifact <out-dir>          check installed shared libraries
#   check-license.sh --run <out-dir>               compile check_license.c against the
#                                                  artifact and run it (native target only)
#
# Fails (exit 1) unless FFmpeg is "LGPL version 2.1 or later" and no
# --enable-gpl / --enable-nonfree / --enable-version3 or GPL/nonfree external
# library is present in the configure line.
set -euo pipefail

PA_FF_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
. "$PA_FF_ROOT/lib/common.sh"
# shellcheck source=lib/ffmpeg.sh
. "$PA_FF_ROOT/lib/ffmpeg.sh"

EXPECTED="LGPL version 2.1 or later"
fail=0
bad() { printf 'licence check: FAIL: %s\n' "$*" >&2; fail=1; }
ok() { printf 'licence check: ok: %s\n' "$*" >&2; }

check_config() {
    local h="$1/config.h"
    [ -f "$h" ] || die "no config.h in $1 (run configure first)"
    local v
    for v in GPL NONFREE VERSION3; do
        if grep -Eq "^#define CONFIG_${v} 0$" "$h"; then ok "CONFIG_${v}=0"
        else bad "CONFIG_${v} is not 0"; fi
    done
    if grep -Fq "#define FFMPEG_LICENSE \"$EXPECTED\"" "$h"; then ok "FFMPEG_LICENSE=\"$EXPECTED\""
    else bad "FFMPEG_LICENSE is $(grep -F '#define FFMPEG_LICENSE' "$h" || echo missing)"; fi
    local conf
    conf="$(grep -F '#define FFMPEG_CONFIGURATION' "$h" || true)"
    if printf '%s\n' "$conf" | grep -Eq -- "$FF_FORBIDDEN_RE"; then
        bad "forbidden option in configuration: $(printf '%s\n' "$conf" | grep -Eo -- "$FF_FORBIDDEN_RE" | tr '\n' ' ')"
    else ok "configure line has no GPL/nonfree/v3 options"; fi
}

# Static scan of the binaries: works for cross targets we cannot execute.
check_artifact() {
    local out="$1" found=0 f
    while IFS= read -r -d '' f; do
        found=1
        local name; name="$(basename "$f")"
        # Each lib embeds "<lib> license: <licence>" (LICENSE_PREFIX + FFMPEG_LICENSE).
        if LC_ALL=C grep -aqF "license: $EXPECTED" "$f"; then ok "$name embeds \"$EXPECTED\""
        else bad "$name does not embed \"$EXPECTED\""; fi
        if LC_ALL=C grep -aEq 'license: (GPL|LGPL version 3|nonfree)' "$f"; then
            bad "$name embeds a non-LGPLv2.1 licence string"
        fi
        if LC_ALL=C grep -aEo -- "$FF_FORBIDDEN_RE" "$f" | grep -q .; then
            bad "$name embeds a forbidden configure option"
        fi
    done < <(find "$out" \( -name 'libav*' -o -name 'libsw*' -o -name 'av*.dll' -o -name 'sw*.dll' \) \
                 \( -name '*.so*' -o -name '*.dylib' -o -name '*.dll' \) -type f -print0)
    [ "$found" = 1 ] || bad "no FFmpeg shared libraries found under $out"
}

run_check() {
    local out="$1" tmp
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" RETURN
    ${CC:-cc} "$PA_FF_ROOT/check_license.c" -o "$tmp/check_license" \
        -I"$out/include" -L"$out/lib" -Wl,-rpath,"$out/lib" \
        -lavformat -lavcodec -lavfilter -lavdevice -lswresample -lswscale -lavutil \
        || die "failed to compile check_license.c"
    if "$tmp/check_license"; then ok "avutil_license() and friends report \"$EXPECTED\""
    else bad "runtime licence check failed"; fi
}

[ $# -eq 2 ] || { sed -n '4,13p' "$0"; exit 2; }
case "$1" in
    --config) check_config "$2" ;;
    --artifact) check_artifact "$2" ;;
    --run) run_check "$2" ;;
    *) sed -n '4,13p' "$0"; exit 2 ;;
esac
exit "$fail"
