#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Put a libsmb2 artifact (tools/libsmb2/out/<target>, or the CI artifact
# libsmb2-<target>) where the app loads it from (LIB-019), the same way
# tools/mpv/bundle.sh does for libmpv. Packaging scripts call this as a hook;
# it never rebuilds anything.
#
#   bundle.sh linux <libsmb2-out> <flutter-linux-bundle>
#       Copy libsmb2.so.1 into <bundle>/lib and its licence texts and
#       BUILDINFO into <bundle>/licenses/libsmb2. pa-smb2 looks for
#       lib/libsmb2.so.1 next to the executable first (bundled_paths).
#
#   bundle.sh android <jniLibs-dir> <libsmb2-out>...
#       Copy lib/libsmb2.so of each android-<abi> artifact into
#       <jniLibs-dir>/<abi>/, after check-android.
#
#   bundle.sh check-android <libsmb2-out>
#       Packaging gate for an Android artifact: one libsmb2.so, a real file
#       with SONAME libsmb2.so, only NDK system libraries as NEEDED,
#       16 KB-aligned LOAD segments (Android 15+). Needs readelf.
set -euo pipefail

die() { printf 'bundle.sh: error: %s\n' "$*" >&2; exit 1; }
log() { printf 'bundle.sh: %s\n' "$*" >&2; }

target_of() { sed -n 's/^target: //p' "$1/BUILDINFO.txt" 2>/dev/null; }

ANDROID_SYSTEM_LIBS_RE='^lib(c|m|dl|log)\.so$'

licences() { # out dest
    mkdir -p "$2"
    cp "$1"/share/licenses/* "$2/"
    cp "$1/BUILDINFO.txt" "$2/BUILDINFO.txt"
}

bundle_linux() {
    local out="$1" bundle="$2"
    [ "$(target_of "$out")" = linux-x86_64 ] || die "$out is not a linux-x86_64 libsmb2 artifact"
    [ -d "$bundle/lib" ] && [ -d "$bundle/data" ] || die "$bundle is not a Flutter Linux bundle"
    # The real file, installed under its SONAME (what pa-smb2 opens).
    local f
    f="$(readlink -f "$out/lib/libsmb2.so.1")" || true
    [ -f "$f" ] || die "no lib/libsmb2.so.1 in $out"
    install -m 0644 "$f" "$bundle/lib/libsmb2.so.1"
    licences "$out" "$bundle/licenses/libsmb2"
    log "lib/libsmb2.so.1, licenses/libsmb2"
}

bundle_android() {
    local jni="$1"; shift
    local out t abi
    for out in "$@"; do
        t="$(target_of "$out")"
        case "$t" in android-*) abi="${t#android-}" ;; *) die "$out is not an android libsmb2 artifact" ;; esac
        check_android "$out"
        mkdir -p "$jni/$abi"
        install -m 0644 "$out/lib/libsmb2.so" "$jni/$abi/libsmb2.so"
        log "$abi: libsmb2.so"
    done
}

check_android() {
    local out="$1" f="$1/lib/libsmb2.so" soname need align fail=0
    command -v readelf >/dev/null || die "readelf is required (binutils or llvm-readelf)"
    [ -f "$f" ] && [ ! -L "$f" ] || die "$out: lib/libsmb2.so missing or a symlink (APKs do not keep symlinks)"
    soname="$(readelf -d "$f" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
    [ "$soname" = libsmb2.so ] || { echo "FAIL: libsmb2.so has SONAME '$soname'" >&2; fail=1; }
    while IFS= read -r need; do
        printf '%s\n' "$need" | grep -Eq "$ANDROID_SYSTEM_LIBS_RE" \
            || { echo "FAIL: libsmb2.so needs $need, not an NDK system library" >&2; fail=1; }
    done < <(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
    for align in $(readelf -lW "$f" | awk '$1 == "LOAD" { print $NF }'); do
        if [ "$((align))" -lt 16384 ]; then
            echo "FAIL: libsmb2.so has a LOAD segment aligned to $align (< 16 KB)" >&2; fail=1; break
        fi
    done
    [ "$fail" = 0 ] || die "android packaging check failed for $out"
    echo "ok: libsmb2.so (SONAME $soname) in $(target_of "$out")" >&2
}

case "${1:-}" in
    linux) [ $# -eq 3 ] || die "usage: bundle.sh linux <libsmb2-out> <flutter-linux-bundle>"; bundle_linux "$2" "$3" ;;
    android) [ $# -ge 3 ] || die "usage: bundle.sh android <jniLibs-dir> <libsmb2-out>..."; shift; bundle_android "$@" ;;
    check-android) [ $# -eq 2 ] || die "usage: bundle.sh check-android <libsmb2-out>"; check_android "$2" ;;
    *) sed -n '4,21p' "$0"; exit 2 ;;
esac
