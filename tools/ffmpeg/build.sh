#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# LGPL-2.1+ FFmpeg build for project_alpha clients (PLAY-001).
#
# Usage:
#   build.sh list              list known targets
#   build.sh fetch             download and verify all pinned sources
#   build.sh <target>          build FFmpeg (shared) + static deps for <target>
#   build.sh sources           pack the corresponding-source bundle (LGPL §6)
#
# Environment:
#   PA_FF_JOBS          parallel jobs (default: all cores)
#   PA_FF_CACHE         download cache          (default: tools/ffmpeg/.cache)
#   PA_FF_WORK          build tree              (default: tools/ffmpeg/build)
#   PA_FF_OUT           artifacts               (default: tools/ffmpeg/out)
#   PA_FF_EXTRA_FLAGS   extra FFmpeg configure flags (still licence-checked)
#   ANDROID_NDK_HOME    Android NDK for android-* targets
set -euo pipefail

PA_FF_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${PA_FF_CACHE:=$PA_FF_ROOT/.cache}"
: "${PA_FF_WORK:=$PA_FF_ROOT/build}"
: "${PA_FF_OUT:=$PA_FF_ROOT/out}"
export PA_FF_ROOT PA_FF_CACHE

# shellcheck source=versions.sh
. "$PA_FF_ROOT/versions.sh"
# shellcheck source=lib/common.sh
. "$PA_FF_ROOT/lib/common.sh"
# shellcheck source=lib/targets.sh
. "$PA_FF_ROOT/lib/targets.sh"
# shellcheck source=lib/deps.sh
. "$PA_FF_ROOT/lib/deps.sh"
# shellcheck source=lib/ffmpeg.sh
. "$PA_FF_ROOT/lib/ffmpeg.sh"

install_licenses() {
    local dst="$OUT_DIR/share/licenses"
    mkdir -p "$dst"
    cp "$SRC_DIR/ffmpeg/COPYING.LGPLv2.1" "$dst/ffmpeg-COPYING.LGPLv2.1"
    cp "$SRC_DIR/ffmpeg/LICENSE.md" "$dst/ffmpeg-LICENSE.md"
    cp "$SRC_DIR/dav1d/COPYING" "$dst/dav1d-COPYING"
    cp "$SRC_DIR/libass/COPYING" "$dst/libass-COPYING"
    cp "$SRC_DIR/fribidi/COPYING" "$dst/fribidi-COPYING"
    cp "$SRC_DIR/harfbuzz/COPYING" "$dst/harfbuzz-COPYING"
    cp "$SRC_DIR/freetype/docs/FTL.TXT" "$dst/freetype-FTL.TXT"
    if [ "$T_OS" = windows ]; then cp "$SRC_DIR/zlib/LICENSE" "$dst/zlib-LICENSE"; fi
}

write_buildinfo() {
    local f="$OUT_DIR/BUILDINFO.txt" pkg
    {
        echo "project_alpha LGPL FFmpeg build (tools/ffmpeg, PLAY-001)"
        echo "target: $TARGET"
        echo "license: $(grep -F '#define FFMPEG_LICENSE' "$BUILD_DIR/config.h" | cut -d'"' -f2)"
        echo
        echo "components (statically linked into the FFmpeg shared libraries unless noted):"
        for pkg in $PA_FFMPEG_PACKAGES; do
            [ "$pkg" = zlib ] && [ "$T_OS" != windows ] && { echo "  zlib: system library (dynamic)"; continue; }
            printf '  %s %s (%s) sha256=%s\n' "$pkg" "$(pkg_var "$pkg" VERSION)" \
                "$(pkg_var "$pkg" LICENSE)" "$(pkg_var "$pkg" SHA256)"
        done
        echo
        echo "patches:"
        (cd "$PA_FF_ROOT/patches" && find . -name '*.patch' | sort | sed 's/^/  /') || true
        echo
        echo "configure: $(grep -F '#define FFMPEG_CONFIGURATION' "$BUILD_DIR/config.h" | cut -d'"' -f2-)"
        echo
        echo "source: https://github.com/DenisHumen/project_alpha/tree/main/tools/ffmpeg"
    } > "$f"
    log "wrote $f"
}

build_target() {
    TARGET="$1"
    setup_target "$TARGET"
    # Reject GPL/nonfree/v3 options before spending time on the dependencies.
    # shellcheck disable=SC2086
    assert_lgpl_flags $FF_LICENSE_FLAGS $FF_COMMON_FLAGS $FF_TARGET_FLAGS ${PA_FF_EXTRA_FLAGS:-}
    SRC_DIR="$PA_FF_WORK/$TARGET/src"
    BUILD_DIR="$PA_FF_WORK/$TARGET/build"
    DEPS_PREFIX="$PA_FF_WORK/$TARGET/deps"
    OUT_DIR="$PA_FF_OUT/$TARGET"
    mkdir -p "$SRC_DIR" "$BUILD_DIR"
    rm -rf "$DEPS_PREFIX"

    MESON_CROSS=""
    if [ "$T_CROSS" = 1 ]; then
        MESON_CROSS="$BUILD_DIR/meson-cross.ini"
        write_meson_cross "$MESON_CROSS"
        # Cross builds see only our deps, never host libraries.
        export PKG_CONFIG_LIBDIR="$DEPS_PREFIX/lib/pkgconfig"
        unset PKG_CONFIG_PATH
    else
        export CFLAGS="$T_CFLAGS" CXXFLAGS="$T_CFLAGS" LDFLAGS="$T_LDFLAGS"
        export PKG_CONFIG_PATH="$DEPS_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    fi

    log "target $TARGET (os=$T_OS cpu=$T_CPU cross=$T_CROSS jobs=$(jobs))"
    build_deps
    build_ffmpeg
    install_licenses
    write_buildinfo

    "$PA_FF_ROOT/check-license.sh" --artifact "$OUT_DIR"
    if [ "$T_CROSS" = 0 ]; then
        "$PA_FF_ROOT/check-license.sh" --run "$OUT_DIR"
    fi
    log "done: $OUT_DIR"
}

pack_sources() {
    local name="project_alpha-ffmpeg-sources-${FFMPEG_VERSION}"
    local stage="$PA_FF_WORK/$name" pkg file
    rm -rf "$stage"; mkdir -p "$stage/upstream"
    for pkg in $PA_FFMPEG_PACKAGES; do
        file="$(fetch "$pkg")"
        cp "$file" "$stage/upstream/"
    done
    # Our build scripts and patches, i.e. everything needed to rebuild/relink.
    tar -C "$PA_FF_ROOT" --exclude=.cache --exclude=build --exclude=out -cf - . \
        | { mkdir -p "$stage/tools-ffmpeg" && tar -C "$stage/tools-ffmpeg" -xf -; }
    (cd "$stage/upstream" && for file in *; do printf '%s  %s\n' "$(sha256_of "$file")" "$file"; done) \
        > "$stage/upstream/SHA256SUMS"
    mkdir -p "$PA_FF_OUT"
    tar -C "$PA_FF_WORK" -cJf "$PA_FF_OUT/$name.tar.xz" "$name"
    log "wrote $PA_FF_OUT/$name.tar.xz"
}

case "${1:-}" in
    list) printf '%s\n' $PA_FF_TARGETS ;;
    fetch) for p in $PA_FFMPEG_PACKAGES; do fetch "$p" >/dev/null; done; log "all sources verified" ;;
    sources) pack_sources ;;
    ""|-h|--help) sed -n '4,19p' "$0" ;;
    *) build_target "$1" ;;
esac
