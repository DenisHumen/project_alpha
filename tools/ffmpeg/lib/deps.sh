# SPDX-License-Identifier: MPL-2.0
# Static, position-independent builds of FFmpeg's external libraries.
# They are linked *into* the FFmpeg shared libraries, so the shipped artifact
# is only libav*/libsw* (+ the system libs listed in BUILDINFO.txt).
#
# Licences: dav1d BSD-2, libass ISC, fribidi LGPL-2.1+, harfbuzz MIT,
# freetype FTL, zlib Zlib — all compatible with an LGPL-2.1+ FFmpeg.
#
# Expects: SRC_DIR, BUILD_DIR, DEPS_PREFIX, MESON_CROSS (file or empty).

_meson_build() {
    local pkg="$1"; shift
    local src bdir
    src="$(unpack "$pkg" "$SRC_DIR")"
    bdir="$BUILD_DIR/$pkg"
    rm -rf "$bdir"
    local cross=()
    [ -n "$MESON_CROSS" ] && cross=(--cross-file "$MESON_CROSS")
    log "meson: $pkg"
    meson setup "$bdir" "$src" "${cross[@]}" \
        --prefix "$DEPS_PREFIX" --libdir lib \
        --buildtype release --default-library static \
        -Db_pie=false -Db_staticpic=true \
        --wrap-mode nodownload \
        "$@" >&2 || die "meson setup failed for $pkg"
    ninja -C "$bdir" -j "$(jobs)" >&2 || die "build failed for $pkg"
    ninja -C "$bdir" install >&2 || die "install failed for $pkg"
}

build_zlib() {
    # Linux, Android and Apple SDKs ship zlib; only Windows needs our own.
    [ "$T_OS" = windows ] || return 0
    local src
    src="$(unpack zlib "$SRC_DIR")"
    log "make: zlib (mingw)"
    make -C "$src" -f win32/Makefile.gcc -j "$(jobs)" \
        PREFIX=x86_64-w64-mingw32- libz.a >&2 || die "zlib build failed"
    # Install by hand: the makefile's install target also wants the DLL resources.
    mkdir -p "$DEPS_PREFIX/include" "$DEPS_PREFIX/lib"
    cp "$src/zlib.h" "$src/zconf.h" "$DEPS_PREFIX/include/"
    cp "$src/libz.a" "$DEPS_PREFIX/lib/"
    mkdir -p "$DEPS_PREFIX/lib/pkgconfig"
    cat > "$DEPS_PREFIX/lib/pkgconfig/zlib.pc" <<EOF
prefix=$DEPS_PREFIX
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: zlib
Description: zlib compression library
Version: $ZLIB_VERSION
Libs: -L\${libdir} -lz
Cflags: -I\${includedir}
EOF
}

build_fribidi() {
    _meson_build fribidi -Ddocs=false -Dbin=false -Dtests=false
}

build_freetype() {
    _meson_build freetype -Dbrotli=disabled -Dbzip2=disabled -Dharfbuzz=disabled \
        -Dpng=disabled -Dzlib=disabled -Dtests=disabled
}

build_harfbuzz() {
    # with_libstdcxx=false (default) keeps harfbuzz free of a libstdc++ runtime dependency.
    _meson_build harfbuzz -Dglib=disabled -Dgobject=disabled -Dcairo=disabled \
        -Dchafa=disabled -Dpng=disabled -Dzlib=disabled -Dicu=disabled \
        -Dfreetype=disabled -Draster=disabled -Dvector=disabled -Dgpu=disabled \
        -Dgpu_demo=disabled -Dsubset=disabled -Dtests=disabled -Dintrospection=disabled \
        -Ddocs=disabled -Dutilities=disabled -Dbenchmark=disabled
}

build_libass() {
    # shellcheck disable=SC2086 # ASS_FONT_FLAGS is a word list
    _meson_build libass $ASS_FONT_FLAGS -Drequire-system-font-provider=false \
        -Dlibunibreak=disabled -Dtest=disabled -Dcompare=disabled -Dprofile=disabled \
        -Dfuzz=disabled -Dcheckasm=disabled
}

build_dav1d() {
    _meson_build dav1d -Denable_tools=false -Denable_tests=false -Denable_examples=false
}

build_deps() {
    build_zlib
    build_fribidi
    build_freetype
    build_harfbuzz
    build_libass
    build_dav1d
}
