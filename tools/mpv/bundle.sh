#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Put a libmpv artifact (tools/mpv/out/<target>, or the CI artifact
# libmpv-<target>) where media_kit loads it from (PLAY-002). Packaging scripts
# call this as a hook; it never rebuilds anything.
#
#   bundle.sh linux <libmpv-out> <flutter-linux-bundle>
#       Copy libmpv.so.2 and the FFmpeg libraries into <bundle>/lib, the licence
#       texts and BUILDINFO into <bundle>/licenses/libmpv, and make the
#       media_kit_video plugin find libmpv next to itself (RUNPATH=$ORIGIN,
#       needs patchelf). The app must then open <bundle>/lib/libmpv.so.2:
#       MediaKit.ensureInitialized(libmpv: ...) or LIBMPV_LIBRARY_PATH.
#
#   bundle.sh android <jniLibs-dir> <libmpv-out>...
#       Copy lib/*.so of each android-<abi> artifact into <jniLibs-dir>/<abi>/
#       (libmpv.so, libav*.so, libsw*.so, libmediakitandroidhelper.so), and the
#       licences into <jniLibs-dir>/../assets/licenses/libmpv/ is left to the caller.
#
#   bundle.sh check-android <libmpv-out>
#       Packaging gate for an Android artifact: unversioned SONAMEs equal to the
#       file names, every NEEDED library either in the set or an NDK system
#       library, 16 KB-aligned LOAD segments (Android 15+). Needs readelf.
set -euo pipefail

die() { printf 'bundle.sh: error: %s\n' "$*" >&2; exit 1; }
log() { printf 'bundle.sh: %s\n' "$*" >&2; }

target_of() { sed -n 's/^target: //p' "$1/BUILDINFO.txt" 2>/dev/null; }

# Libraries an Android app may link from the platform (NDK stable APIs).
ANDROID_SYSTEM_LIBS_RE='^lib(c|m|dl|log|android|EGL|GLESv[123]|mediandk|aaudio|OpenSLES|z|jnigraphics|vulkan|nativewindow|camera2ndk|sync)\.so$'

bundle_linux() {
    local out="$1" bundle="$2"
    [ "$(target_of "$out")" = linux-x86_64 ] || die "$out is not a linux-x86_64 libmpv artifact"
    [ -d "$bundle/lib" ] && [ -d "$bundle/data" ] || die "$bundle is not a Flutter Linux bundle"
    command -v patchelf >/dev/null || die "patchelf is required (apt install patchelf)"
    local f name soname
    # Real files, installed under their SONAME (what the dynamic linker looks for).
    for f in "$out"/lib/lib*.so.*; do
        [ -f "$f" ] && [ ! -L "$f" ] || continue
        soname="$(patchelf --print-soname "$f")"
        name="${soname:-$(basename "$f")}"
        install -m 0644 "$f" "$bundle/lib/$name"
        # FFmpeg libraries need each other; resolve them next to themselves too
        # (libmpv.so.2 already has RUNPATH=$ORIGIN from its build).
        case "$name" in libmpv.so*) ;; *) patchelf --set-rpath '$ORIGIN' "$bundle/lib/$name" ;; esac
        log "lib/$name"
    done
    [ -f "$bundle/lib/libmpv.so.2" ] || die "no libmpv.so.2 in $out/lib"
    # The plugin links libmpv.so.2; let it find our copy in the same directory
    # instead of a system libmpv.
    for f in "$bundle"/lib/libmedia_kit_video_plugin.so; do
        [ -f "$f" ] || continue
        patchelf --set-rpath '$ORIGIN' "$f"
        log "RUNPATH=\$ORIGIN on $(basename "$f")"
    done
    mkdir -p "$bundle/licenses/libmpv"
    cp "$out"/share/licenses/* "$bundle/licenses/libmpv/"
    cp "$out/BUILDINFO.txt" "$bundle/licenses/libmpv/BUILDINFO.txt"
    log "done; have the app load lib/libmpv.so.2 (MediaKit.ensureInitialized(libmpv: ...) or LIBMPV_LIBRARY_PATH)"
}

bundle_android() {
    local jni="$1"; shift
    local out t abi f
    for out in "$@"; do
        t="$(target_of "$out")"
        case "$t" in android-*) abi="${t#android-}" ;; *) die "$out is not an android libmpv artifact" ;; esac
        check_android "$out"
        mkdir -p "$jni/$abi"
        for f in "$out"/lib/*.so; do
            install -m 0644 "$f" "$jni/$abi/$(basename "$f")"
        done
        log "$abi: $(cd "$out/lib" && ls -- *.so | tr '\n' ' ')"
    done
}

check_android() {
    local out="$1" f name soname need fail=0 set=" "
    command -v readelf >/dev/null || die "readelf is required (binutils or llvm-readelf)"
    for f in "$out"/lib/*.so; do set="$set$(basename "$f") "; done
    [ "$set" != " " ] || die "no .so files in $out/lib"
    case "$set" in *" libmpv.so "*) ;; *) echo "FAIL: libmpv.so missing" >&2; fail=1 ;; esac
    case "$set" in *" libavcodec.so "*) ;; *) echo "FAIL: libavcodec.so missing" >&2; fail=1 ;; esac
    case "$set" in *" libmediakitandroidhelper.so "*) ;; *) echo "FAIL: libmediakitandroidhelper.so missing" >&2; fail=1 ;; esac
    for f in "$out"/lib/*.so; do
        name="$(basename "$f")"
        [ -L "$f" ] && { echo "FAIL: $name is a symlink (APKs do not keep symlinks)" >&2; fail=1; continue; }
        soname="$(readelf -d "$f" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
        if [ "$soname" != "$name" ]; then echo "FAIL: $name has SONAME '$soname'" >&2; fail=1; fi
        while IFS= read -r need; do
            case "$set" in *" $need "*) continue ;; esac
            if ! printf '%s\n' "$need" | grep -Eq "$ANDROID_SYSTEM_LIBS_RE"; then
                echo "FAIL: $name needs $need, which is neither bundled nor an NDK system library" >&2; fail=1
            fi
        done < <(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
        # Every PT_LOAD alignment must be >= 16 KB (0x4000).
        local align
        for align in $(readelf -lW "$f" | awk '$1 == "LOAD" { print $NF }'); do
            if [ "$((align))" -lt 16384 ]; then
                echo "FAIL: $name has a LOAD segment aligned to $align (< 16 KB)" >&2; fail=1; break
            fi
        done
        echo "ok: $name (SONAME $soname)" >&2
    done
    [ "$fail" = 0 ] || die "android packaging check failed for $out"
}

case "${1:-}" in
    linux) [ $# -eq 3 ] || die "usage: bundle.sh linux <libmpv-out> <flutter-linux-bundle>"; bundle_linux "$2" "$3" ;;
    android) [ $# -ge 3 ] || die "usage: bundle.sh android <jniLibs-dir> <libmpv-out>..."; shift; bundle_android "$@" ;;
    check-android) [ $# -eq 2 ] || die "usage: bundle.sh check-android <libmpv-out>"; check_android "$2" ;;
    *) sed -n '4,25p' "$0"; exit 2 ;;
esac
