#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# LGPL-2.1+ libmpv build for project_alpha clients (PLAY-002).
# mpv -Dgpl=false + static libplacebo and libass, dynamically linked against
# our LGPL FFmpeg (tools/ffmpeg, built in the same run with the same flags).
#
# Usage:
#   build.sh list              list known targets
#   build.sh fetch             download and verify all pinned sources (FFmpeg + mpv)
#   build.sh <target>          build FFmpeg + libmpv for <target>
#   build.sh sources           pack the corresponding-source bundle (LGPL §6)
#
# Environment:
#   PA_FF_JOBS          parallel jobs (default: all cores)
#   PA_MPV_CACHE        download cache  (default: tools/mpv/.cache)
#   PA_MPV_WORK         build tree      (default: tools/mpv/build)
#   PA_MPV_OUT          artifacts       (default: tools/mpv/out)
#   PA_MPV_EXTRA_FLAGS  extra mpv meson options (still licence-checked)
#   PA_FF_EXTRA_FLAGS   extra FFmpeg configure flags (still licence-checked)
#   ANDROID_NDK_HOME    Android NDK for android-* targets
set -euo pipefail

PA_MPV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PA_FF_ROOT="$(cd "$PA_MPV_ROOT/../ffmpeg" && pwd)"
: "${PA_MPV_CACHE:=$PA_MPV_ROOT/.cache}"
: "${PA_MPV_WORK:=$PA_MPV_ROOT/build}"
: "${PA_MPV_OUT:=$PA_MPV_ROOT/out}"
PA_FF_CACHE="$PA_MPV_CACHE"
export PA_MPV_ROOT PA_FF_ROOT PA_FF_CACHE

# The FFmpeg recipe is reused as is: same versions, deps, flags and licence gate.
# shellcheck source=../ffmpeg/versions.sh
. "$PA_FF_ROOT/versions.sh"
# shellcheck source=../ffmpeg/lib/common.sh
. "$PA_FF_ROOT/lib/common.sh"
# shellcheck source=../ffmpeg/lib/targets.sh
. "$PA_FF_ROOT/lib/targets.sh"
# shellcheck source=../ffmpeg/lib/deps.sh
. "$PA_FF_ROOT/lib/deps.sh"
# shellcheck source=../ffmpeg/lib/ffmpeg.sh
. "$PA_FF_ROOT/lib/ffmpeg.sh"
# shellcheck source=versions.sh
. "$PA_MPV_ROOT/versions.sh"
# shellcheck source=lib/mpv.sh
. "$PA_MPV_ROOT/lib/mpv.sh"

# Targets with a libmpv build. iOS/tvOS use MPVKit or the native engine
# (ADR-0003) and are not built here.
PA_MPV_TARGETS="linux-x86_64 windows-x86_64 \
android-arm64-v8a android-armeabi-v7a android-x86_64 \
macos-arm64 macos-x86_64"

install_licenses() {
    local dst="$OUT_DIR/share/licenses"
    mkdir -p "$dst"
    cp "$SRC_DIR/mpv/LICENSE.LGPL" "$dst/mpv-LICENSE.LGPL"
    cp "$SRC_DIR/mpv/Copyright" "$dst/mpv-Copyright"
    cp "$SRC_DIR/libplacebo/LICENSE" "$dst/libplacebo-LICENSE"
    cp "$SRC_DIR/libplacebo/3rdparty/fast_float/LICENSE-MIT" "$dst/fast_float-LICENSE-MIT"
    cp "$SRC_DIR/libplacebo/3rdparty/glad/LICENSE" "$dst/glad-LICENSE"
    cp "$SRC_DIR/libplacebo/3rdparty/Vulkan-Headers/LICENSE.md" "$dst/Vulkan-Headers-LICENSE.md"
    cp "$SRC_DIR/ffmpeg/COPYING.LGPLv2.1" "$dst/ffmpeg-COPYING.LGPLv2.1"
    cp "$SRC_DIR/ffmpeg/LICENSE.md" "$dst/ffmpeg-LICENSE.md"
    cp "$SRC_DIR/dav1d/COPYING" "$dst/dav1d-COPYING"
    cp "$SRC_DIR/libass/COPYING" "$dst/libass-COPYING"
    cp "$SRC_DIR/fribidi/COPYING" "$dst/fribidi-COPYING"
    cp "$SRC_DIR/harfbuzz/COPYING" "$dst/harfbuzz-COPYING"
    cp "$SRC_DIR/freetype/docs/FTL.TXT" "$dst/freetype-FTL.TXT"
    if [ "$T_OS" = linux ]; then
        cp "$SRC_DIR/libdisplay_info/LICENSE" "$dst/libdisplay-info-LICENSE"
        cp /usr/share/doc/hwdata/copyright "$dst/hwdata-pnp.ids-copyright"
    fi
    if [ "$T_OS" = windows ]; then cp "$SRC_DIR/zlib/LICENSE" "$dst/zlib-LICENSE"; fi
    if [ "$T_OS" = android ]; then
        cp "$SRC_DIR/mediakit_helper/LICENSE" "$dst/media-kit-android-helper-LICENSE"
    fi
}

write_buildinfo() {
    local f="$OUT_DIR/BUILDINFO.txt" pkg h="$BUILD_DIR/mpv/config.h"
    {
        echo "project_alpha LGPL libmpv build (tools/mpv, PLAY-002)"
        echo "target: $TARGET"
        echo "mpv licence: LGPL-2.1-or-later (-Dgpl=false)"
        echo "ffmpeg licence: $(grep -F '#define FFMPEG_LICENSE' "$BUILD_DIR/config.h" | cut -d'"' -f2)"
        echo
        echo "components:"
        echo "  shared: libmpv, FFmpeg libav*/libsw*"
        echo "  static inside libmpv: libplacebo, libass (+ freetype, harfbuzz, fribidi), fast_float"
        echo "  static inside FFmpeg: dav1d, libass (+ deps)"
        for pkg in $PA_FFMPEG_PACKAGES $PA_MPV_PACKAGES; do
            [ "$pkg" = zlib ] && [ "$T_OS" != windows ] && { echo "  zlib: system library (dynamic)"; continue; }
            printf '  %s %s (%s) sha256=%s\n' "$pkg" "$(pkg_var "$pkg" VERSION)" \
                "$(pkg_var "$pkg" LICENSE)" "$(pkg_var "$pkg" SHA256)"
        done
        echo
        echo "patches:"
        (cd "$PA_FF_ROOT/patches" && find . -name '*.patch' | sort | sed 's|^\./|  ffmpeg-recipe/|') || true
        (cd "$PA_MPV_ROOT/patches" && find . -name '*.patch' | sort | sed 's|^\./|  |') || true
        echo
        echo "mpv features: $(grep -F '#define FULLCONFIG' "$h" | cut -d'"' -f2)"
        echo "mpv options: $MPV_FLAGS"
        echo "ffmpeg configure: $(grep -F '#define FFMPEG_CONFIGURATION' "$BUILD_DIR/config.h" | cut -d'"' -f2-)"
        echo
        echo "source: https://github.com/DenisHumen/project_alpha/tree/main/tools/mpv"
    } > "$f"
    log "wrote $f"
}

# Drop what clients never load: static archives, libtool files, and the
# headers/pkg-config of FFmpeg stay (needed to build against libmpv).
prune_output() {
    find "$OUT_DIR" -name '*.a' ! -name '*.dll.a' -delete
    find "$OUT_DIR" -name '*.la' -delete
}

# The .pc files carry the absolute build prefix; make them relative to their
# own location so a downloaded artifact works with PKG_CONFIG_PATH
# (media_kit_video's Linux plugin finds libmpv with pkg-config).
relocate_pc() {
    local pc
    for pc in "$OUT_DIR"/lib/pkgconfig/*.pc; do
        [ -e "$pc" ] || continue
        sed -e "s|$OUT_DIR|\${pcfiledir}/../..|g" "$pc" > "$pc.tmp" && mv "$pc.tmp" "$pc"
    done
}

build_target() {
    TARGET="$1"
    case " $PA_MPV_TARGETS " in *" $TARGET "*) ;; *) die "unknown target '$TARGET' (known: $PA_MPV_TARGETS)" ;; esac
    setup_target "$TARGET"
    # Reject GPL/nonfree/v3 options before spending time on the dependencies.
    # shellcheck disable=SC2086
    assert_lgpl_flags $FF_LICENSE_FLAGS $FF_COMMON_FLAGS $FF_TARGET_FLAGS ${PA_FF_EXTRA_FLAGS:-}
    # shellcheck disable=SC2046,SC2086
    assert_mpv_lgpl_flags $MPV_LICENSE_FLAGS $MPV_COMMON_FLAGS $(mpv_target_flags) ${PA_MPV_EXTRA_FLAGS:-}

    SRC_DIR="$PA_MPV_WORK/$TARGET/src"
    BUILD_DIR="$PA_MPV_WORK/$TARGET/build"
    DEPS_PREFIX="$PA_MPV_WORK/$TARGET/deps"
    OUT_DIR="$PA_MPV_OUT/$TARGET"
    mkdir -p "$SRC_DIR" "$BUILD_DIR"
    rm -rf "$DEPS_PREFIX"

    MESON_CROSS=""
    if [ "$T_CROSS" = 1 ]; then
        MESON_CROSS="$BUILD_DIR/meson-cross.ini"
        write_meson_cross "$MESON_CROSS"
        # Cross builds see only our deps and FFmpeg, never host libraries.
        export PKG_CONFIG_LIBDIR="$DEPS_PREFIX/lib/pkgconfig:$OUT_DIR/lib/pkgconfig"
        unset PKG_CONFIG_PATH
    else
        export CFLAGS="$T_CFLAGS" CXXFLAGS="$T_CFLAGS" LDFLAGS="$T_LDFLAGS"
        export PKG_CONFIG_PATH="$DEPS_PREFIX/lib/pkgconfig:$OUT_DIR/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    fi

    log "target $TARGET (os=$T_OS cpu=$T_CPU cross=$T_CROSS jobs=$(jobs))"
    build_deps
    build_ffmpeg            # installs into OUT_DIR (wipes it first)
    build_libplacebo
    build_libdisplay_info
    flatten_static_pc
    build_mpv
    build_android_helper
    prune_output
    relocate_pc
    install_licenses
    write_buildinfo

    "$PA_MPV_ROOT/check-license.sh" --artifact "$OUT_DIR"
    if [ "$T_RUNNABLE" = 1 ]; then
        CFLAGS="$T_CFLAGS" LDFLAGS="$T_LDFLAGS" "$PA_MPV_ROOT/check-license.sh" --run "$OUT_DIR"
    fi
    log "done: $OUT_DIR"
}

pack_sources() {
    local name="project_alpha-libmpv-sources-${MPV_VERSION}"
    local stage="$PA_MPV_WORK/$name" pkg file
    rm -rf "$stage"; mkdir -p "$stage/upstream"
    for pkg in $PA_FFMPEG_PACKAGES $PA_MPV_PACKAGES; do
        file="$(fetch "$pkg")"
        cp "$file" "$stage/upstream/"
    done
    # Both recipes and their patches: everything needed to rebuild and relink.
    local d
    for d in ffmpeg mpv; do
        mkdir -p "$stage/tools/$d"
        tar -C "$PA_MPV_ROOT/../$d" --exclude=.cache --exclude=build --exclude=out -cf - . \
            | tar -C "$stage/tools/$d" -xf -
    done
    (cd "$stage/upstream" && for file in *; do printf '%s  %s\n' "$(sha256_of "$file")" "$file"; done) \
        > "$stage/upstream/SHA256SUMS"
    mkdir -p "$PA_MPV_OUT"
    XZ_OPT="${XZ_OPT:--T0}" tar -C "$PA_MPV_WORK" -cJf "$PA_MPV_OUT/$name.tar.xz" "$name"
    log "wrote $PA_MPV_OUT/$name.tar.xz"
}

case "${1:-}" in
    list) printf '%s\n' $PA_MPV_TARGETS ;;
    fetch) for p in $PA_FFMPEG_PACKAGES $PA_MPV_PACKAGES; do fetch "$p" >/dev/null; done; log "all sources verified" ;;
    sources) pack_sources ;;
    ""|-h|--help) sed -n '4,21p' "$0" ;;
    *) build_target "$1" ;;
esac
