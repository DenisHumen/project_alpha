#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Licence gate for the libmpv build (PLAY-002).
#
#   check-license.sh --config <mpv-build-dir>   check a configured mpv tree (config.h)
#   check-license.sh --artifact <out-dir>       static scan of libmpv + FFmpeg in an artifact
#   check-license.sh --run <out-dir>            compile check_libmpv.c against the artifact
#                                               and run it (native target only)
#
# Fails (exit 1) unless mpv is built with -Dgpl=false, none of its GPL-only
# features is enabled, no GPL/nonfree library is linked, and the bundled FFmpeg
# passes tools/ffmpeg/check-license.sh ("LGPL version 2.1 or later").
set -euo pipefail

PA_MPV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PA_FF_ROOT="$(cd "$PA_MPV_ROOT/../ffmpeg" && pwd)"
# shellcheck source=../ffmpeg/lib/common.sh
. "$PA_FF_ROOT/lib/common.sh"
# shellcheck source=lib/mpv.sh
. "$PA_MPV_ROOT/lib/mpv.sh"

fail=0
bad() { printf 'libmpv licence check: FAIL: %s\n' "$*" >&2; fail=1; }
ok() { printf 'libmpv licence check: ok: %s\n' "$*" >&2; }

# check_features <space-separated feature list> <where>
check_features() {
    local list=" $1 " f hit=""
    for f in $MPV_FORBIDDEN_FEATURES; do
        case "$list" in *" $f "*) hit="$hit $f" ;; esac
    done
    if [ -n "$hit" ]; then bad "GPL feature(s) enabled in $2:$hit"
    else ok "no GPL features in $2"; fi
}

check_config() {
    local h="$1/config.h"
    [ -f "$h" ] || die "no config.h in $1 (run meson setup first)"
    if grep -Eq '^#define HAVE_GPL 0$' "$h"; then ok "HAVE_GPL=0"
    else bad "HAVE_GPL is not 0"; fi
    local conf feats
    conf="$(grep -F '#define CONFIGURATION ' "$h" || true)"
    case "$conf" in
        *-Dgpl=false*) ok "configured with -Dgpl=false" ;;
        *) bad "configuration does not contain -Dgpl=false" ;;
    esac
    feats="$(grep -F '#define FULLCONFIG ' "$h" | cut -d'"' -f2)"
    [ -n "$feats" ] || bad "no FULLCONFIG in config.h"
    check_features "$feats" "config.h"
    local f v
    for f in $MPV_FORBIDDEN_FEATURES; do
        v="$(printf '%s' "$f" | tr 'a-z-' 'A-Z_')"
        if grep -Eq "^#define HAVE_${v} 1$" "$h"; then bad "HAVE_${v}=1"; fi
    done
}

# Static scan: works for every target, including cross builds.
check_artifact() {
    local out="$1" found=0 f
    "$PA_FF_ROOT/check-license.sh" --artifact "$out" || fail=1
    local feats=""
    if [ -f "$out/BUILDINFO.txt" ]; then
        feats="$(sed -n 's/^mpv features: //p' "$out/BUILDINFO.txt")"
    fi
    while IFS= read -r -d '' f; do
        found=1
        local name; name="$(basename "$f")"
        # mpv embeds its meson options (CONFIGURATION) in the library.
        if LC_ALL=C grep -aqF -- "-Dgpl=false" "$f" && ! LC_ALL=C grep -aqF -- "-Dgpl=true" "$f"; then
            ok "$name embeds -Dgpl=false"
        else bad "$name does not embed -Dgpl=false (or embeds -Dgpl=true)"; fi
        # ... and its feature list (FULLCONFIG): tie it to BUILDINFO and re-check.
        if [ -n "$feats" ]; then
            if LC_ALL=C grep -aqF -- "$feats" "$f"; then ok "$name embeds the feature list from BUILDINFO.txt"
            else bad "$name does not embed the feature list recorded in BUILDINFO.txt"; fi
            check_features "$feats" "$name"
        else bad "no 'mpv features:' line in $out/BUILDINFO.txt"; fi
    done < <(find "$out" \( -name 'libmpv.so*' -o -name 'libmpv*.dylib' -o -name 'libmpv-*.dll' \) -type f -print0)
    [ "$found" = 1 ] || bad "no libmpv shared library found under $out"
    # No GPL/nonfree library may be referenced by anything we ship.
    while IFS= read -r -d '' f; do
        if LC_ALL=C grep -aEo -- "$MPV_FORBIDDEN_LIBS_RE" "$f" | grep -q .; then
            bad "$(basename "$f") references a GPL/nonfree library: $(LC_ALL=C grep -aEo -- "$MPV_FORBIDDEN_LIBS_RE" "$f" | sort -u | tr '\n' ' ')"
        fi
    done < <(find "$out" \( -name '*.so*' -o -name '*.dylib' -o -name '*.dll' \) -type f -print0)
}

run_check() {
    local out="$1" tmp
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" RETURN
    # shellcheck disable=SC2086 # CFLAGS/LDFLAGS are word lists
    ${CC:-cc} ${CFLAGS:-} "$PA_MPV_ROOT/check_libmpv.c" -o "$tmp/check_libmpv" \
        -I"$out/include" ${LDFLAGS:-} -L"$out/lib" -Wl,-rpath,"$out/lib" -lmpv \
        || die "failed to compile check_libmpv.c"
    if "$tmp/check_libmpv"; then ok "mpv_client_api_version() and runtime configuration"
    else bad "runtime libmpv check failed"; fi
    CFLAGS="${CFLAGS:-}" LDFLAGS="${LDFLAGS:-}" "$PA_FF_ROOT/check-license.sh" --run "$out" || fail=1
}

[ $# -eq 2 ] || { sed -n '4,13p' "$0"; exit 2; }
case "$1" in
    --config) check_config "$2" ;;
    --artifact) check_artifact "$2" ;;
    --run) run_check "$2" ;;
    *) sed -n '4,13p' "$0"; exit 2 ;;
esac
exit "$fail"
