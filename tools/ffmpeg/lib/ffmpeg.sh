# SPDX-License-Identifier: MPL-2.0
# FFmpeg configure flags (single source of truth) and the FFmpeg build step.
#
# Licence policy (docs/research/02 §3.1, CLAUDE.md): the result MUST report
# "LGPL version 2.1 or later". GPL, nonfree and (L)GPLv3 code is never enabled.
# check-license.sh verifies this on the configured tree and on the binaries.

# Licence switches. Kept explicit (even though they are the defaults) so that a
# reviewer or a grep can see the intent, and so the checker can assert them.
FF_LICENSE_FLAGS="--disable-gpl --disable-nonfree --disable-version3"

# Encoders we actually need: aac/ac3 for audio re-encoding (transcode, AVR
# passthrough fallback), png/mjpeg for screenshots and trickplay thumbnails.
FF_ENCODERS="aac,ac3,png,mjpeg"

FF_COMMON_FLAGS="\
--enable-shared --disable-static --enable-pic \
--disable-programs --disable-doc --disable-debug \
--disable-autodetect \
--enable-zlib --enable-libdav1d --enable-libass \
--disable-encoders --enable-encoder=$FF_ENCODERS \
--disable-indevs --disable-outdevs"

# Anything on this list in the configure line fails the build.
FF_FORBIDDEN_RE='--enable-(gpl|nonfree|version3)|--enable-lib(x264|x265|xvid|fdk[-_]aac|vidstab|rubberband|davs2|xavs2?|cdio|dvdnav|dvdread|smbclient|mpeghdec)|--enable-(frei0r|avisynth|decklink|cuda-nvcc|cuda-sdk)'

assert_lgpl_flags() {
    if printf '%s\n' "$*" | grep -Eq -- "$FF_FORBIDDEN_RE"; then
        die "forbidden (GPL/nonfree/v3) FFmpeg option in: $(printf '%s\n' "$*" | grep -Eo -- "$FF_FORBIDDEN_RE" | tr '\n' ' ')"
    fi
}

build_ffmpeg() {
    local src bdir
    src="$(unpack ffmpeg "$SRC_DIR")"
    bdir="$BUILD_DIR/ffmpeg"
    rm -rf "$bdir"; mkdir -p "$bdir"

    # shellcheck disable=SC2206 # intentional word splitting of flag lists
    local flags=($FF_LICENSE_FLAGS $FF_COMMON_FLAGS $FF_TARGET_FLAGS ${PA_FF_EXTRA_FLAGS:-})
    assert_lgpl_flags "${flags[@]}"

    log "configure: ffmpeg $FFMPEG_VERSION for $TARGET"
    (
        cd "$bdir"
        "$src/configure" \
            --prefix="$OUT_DIR" \
            --pkg-config=pkg-config --pkg-config-flags=--static \
            --extra-cflags="$T_CFLAGS -I$DEPS_PREFIX/include" \
            --extra-ldflags="$T_LDFLAGS -L$DEPS_PREFIX/lib" \
            "${flags[@]}"
    ) >&2 || { tail -n 60 "$bdir/ffbuild/config.log" >&2 || true; die "ffmpeg configure failed"; }

    # Fail fast, before spending time on compilation.
    "$PA_FF_ROOT/check-license.sh" --config "$bdir" || die "licence check failed on configured tree"

    log "make: ffmpeg"
    make -C "$bdir" -j "$(jobs)" >&2 || die "ffmpeg build failed"
    rm -rf "$OUT_DIR"
    make -C "$bdir" install >&2 || die "ffmpeg install failed"
    cp "$bdir/ffbuild/config.mak" "$bdir/config.h" "$BUILD_DIR/" 2>/dev/null || true
}
