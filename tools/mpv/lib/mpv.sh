# SPDX-License-Identifier: MPL-2.0
# libplacebo and mpv build steps and the mpv meson flags (single source of truth).
#
# Licence policy (docs/research/02 §2.2, §3.1; ADR-0003): libmpv MUST be built
# with -Dgpl=false, which makes it LGPL-2.1-or-later. mpv itself refuses to
# enable its GPL-only parts (X11, DVD, CDDA, DVB, JACK, OSS, caca, direct3d)
# in that mode; we additionally reject any of them here and in check-license.sh.
#
# Expects (from build.sh): SRC_DIR BUILD_DIR DEPS_PREFIX OUT_DIR MESON_CROSS
# and the target variables from tools/ffmpeg/lib/targets.sh.

# Licence switch, kept separate so the checker and a reviewer can see it.
MPV_LICENSE_FLAGS="-Dgpl=false"

# auto_features=disabled: like FFmpeg's --disable-autodetect, only what is
# enabled explicitly below gets in, never whatever the build host happens to have.
MPV_COMMON_FLAGS="\
-Dauto_features=disabled \
-Dlibmpv=true -Dcplayer=false -Dtests=false -Dfuzzers=false \
-Dbuild-date=false -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled \
-Dlua=disabled -Djavascript=disabled -Dcplugins=disabled \
-Dlibavdevice=disabled -Dzlib=enabled -Dvector=enabled \
-Dgl=enabled -Dplain-gl=enabled"

# mpv features that are GPL (or link GPL libraries). Their presence in the
# options, in config.h or in the embedded feature list fails the build.
# Matching is on whole feature names (see check-license.sh).
MPV_FORBIDDEN_FEATURES="gpl cdda dvbin dvda dvdnav jack oss-audio caca direct3d x11 xv \
x11-clipboard gl-x11 egl-x11 vdpau-gl-x11 vaapi-x11 rubberband"

# Libraries that must never be linked (GPL or nonfree); scanned in the binaries.
MPV_FORBIDDEN_LIBS_RE='lib(rubberband|cdio|cdio_paranoia|dvdnav|dvdread|jack|caca|x264|x265|xvidcore|fdk-aac|vidstab|postproc)[.-]'

# Per-target features: platform video/audio outputs and hardware decoding.
mpv_target_flags() {
    case "$T_OS" in
        linux)
            # Render API (media_kit, ADR-0009 texture path) over EGL, VA-API
            # interop, PulseAudio and ALSA. X11 is GPL in mpv and stays off;
            # Wayland windows (PLAY-DSK-003) are not needed for the render API.
            echo "-Diconv=enabled -Degl=enabled -Ddrm=enabled \
-Dvaapi=enabled -Dvaapi-drm=enabled -Dpulse=enabled -Dalsa=enabled" ;;
        android)
            # media_kit draws into an Android Surface: EGL + MediaCodec.
            echo "-Degl-android=enabled -Dandroid-media-ndk=enabled \
-Daudiotrack=enabled -Daaudio=enabled -Dopensles=enabled" ;;
        windows)
            echo "-Dwin32-threads=enabled -Dgl-win32=enabled -Dgl-dxinterop=enabled \
-Dd3d-hwaccel=enabled -Dwasapi=enabled -Diconv=disabled" ;;
        darwin)
            # No Swift (cocoa-cb) and no Cocoa window: the render API is enough
            # for libmpv clients; the native window is PLAY-APL/PLAY-DSK work.
            # VideoToolbox decodes through FFmpeg (hwdec=videotoolbox-copy).
            echo "-Dswift-build=disabled -Dcoreaudio=enabled" ;;
        *) die "no mpv flags for os $T_OS" ;;
    esac
}

assert_mpv_lgpl_flags() {
    local args=" $* " f
    case "$args" in *" -Dgpl=false "*) ;; *) die "mpv options must contain -Dgpl=false" ;; esac
    case "$args" in *" -Dgpl=true "*) die "forbidden (GPL) mpv option -Dgpl=true" ;; esac
    for f in $MPV_FORBIDDEN_FEATURES; do
        [ "$f" = gpl ] && continue
        case "$args" in *" -D$f=enabled "*|*" -D$f=auto "*)
            die "forbidden (GPL) mpv option -D$f in: $*" ;;
        esac
    done
}

# Static libraries in DEPS_PREFIX are linked into libmpv. Their .pc files list
# transitive libraries under Libs.private / Requires.private, which pkg-config
# only reports with --static; mpv's meson does not ask for that, so merge them
# into Libs / Requires (our prefix has no shared versions anyway).
flatten_static_pc() {
    local pc
    for pc in "$DEPS_PREFIX"/lib/pkgconfig/*.pc; do
        [ -e "$pc" ] || continue
        awk '
            /^Libs:/          { libs = libs " " substr($0, 6); next }
            /^Libs.private:/  { libs = libs " " substr($0, 14); next }
            /^Requires:/      { req = req (req ? ", " : "") substr($0, 10); next }
            /^Requires.private:/ { req = req (req ? ", " : "") substr($0, 18); next }
            { print }
            END { print "Libs:" libs; if (req) print "Requires: " req }
        ' "$pc" > "$pc.tmp"
        mv "$pc.tmp" "$pc"
    done
}

# Like tools/ffmpeg's unpack, but takes patches from tools/mpv/patches.
mpv_unpack() {
    PA_FF_ROOT="$PA_MPV_ROOT" unpack "$@"
}

_mpv_meson() { # <pkg> <src> <default_library> <prefix> args...
    local pkg="$1" src="$2" kind="$3" prefix="$4"; shift 4
    local bdir="$BUILD_DIR/$pkg" cross=()
    rm -rf "$bdir"
    if [ -n "$MESON_CROSS" ]; then
        cross=(--cross-file "$MESON_CROSS")
        [ -n "${MPV_MACHINE_EXTRA:-}" ] && cross+=(--cross-file "$MPV_MACHINE_EXTRA")
    elif [ -n "${MPV_MACHINE_EXTRA:-}" ]; then
        cross=(--native-file "$MPV_MACHINE_EXTRA")
    fi
    log "meson: $pkg"
    meson setup "$bdir" "$src" "${cross[@]}" \
        --prefix "$prefix" --libdir lib \
        --buildtype release --default-library "$kind" \
        -Db_pie=false -Db_staticpic=true \
        --wrap-mode nodownload \
        "$@" >&2 || { tail -n 60 "$bdir/meson-logs/meson-log.txt" >&2 || true; die "meson setup failed for $pkg"; }
    ninja -C "$bdir" -j "$(jobs)" >&2 || die "build failed for $pkg"
    ninja -C "$bdir" install >&2 || die "install failed for $pkg"
}

build_libplacebo() {
    local src third pkg file
    src="$(mpv_unpack libplacebo "$SRC_DIR")"
    third="$src/3rdparty"
    # Submodules missing from the archive: unpack the pinned tarballs in place.
    local dir
    for pkg in glad jinja markupsafe fast_float vulkan_headers; do
        file="$(fetch "$pkg")"
        dir="$pkg"; [ "$pkg" = vulkan_headers ] && dir=Vulkan-Headers
        rm -rf "${third:?}/$dir"; mkdir -p "$third/$dir"
        tar -xf "$file" -C "$third/$dir" --strip-components=1
    done
    # OpenGL only: that is what the libmpv render API (media_kit) uses. Vulkan
    # and D3D11 need shaderc/glslang and come with the native HDR output
    # (PLAY-DSK-003). Dolby Vision RPU reshaping uses libplacebo's built-in
    # implementation (-Ddovi); libdovi (Rust) is not linked yet.
    # MinGW: libplacebo builds a UCRT math import library with dlltool, which
    # meson looks up by its generic name unless the cross file names it.
    if [ "$T_OS" = windows ]; then
        MPV_MACHINE_EXTRA="$BUILD_DIR/libplacebo-tools.ini"
        printf "[binaries]\ndlltool = '%s'\n" "${WINDRES%windres}dlltool" >"$MPV_MACHINE_EXTRA"
    fi
    _mpv_meson libplacebo "$src" static "$DEPS_PREFIX" \
        -Dvulkan=disabled -Dopengl=enabled -Dgl-proc-addr=enabled -Dd3d11=disabled \
        -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled \
        -Ddovi=enabled -Dlibdovi=disabled -Dunwind=disabled -Dxxhash=disabled \
        -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false
    MPV_MACHINE_EXTRA=""
}

build_libdisplay_info() {
    [ "$T_OS" = linux ] || return 0
    local src
    src="$(mpv_unpack libdisplay_info "$SRC_DIR")"
    _mpv_meson libdisplay_info "$src" static "$DEPS_PREFIX"
}

build_mpv() {
    local src
    src="$(mpv_unpack mpv "$SRC_DIR")"
    # shellcheck disable=SC2206 # intentional word splitting of flag lists
    local flags=($MPV_LICENSE_FLAGS $MPV_COMMON_FLAGS $(mpv_target_flags) ${PA_MPV_EXTRA_FLAGS:-})
    assert_mpv_lgpl_flags "${flags[@]}"
    MPV_FLAGS="${flags[*]}"
    MPV_MACHINE_EXTRA="$BUILD_DIR/mpv-link.ini"
    write_mpv_link_file "$MPV_MACHINE_EXTRA"
    _mpv_meson mpv "$src" shared "$OUT_DIR" "${flags[@]}"
    MPV_MACHINE_EXTRA=""
    # Fail on the configured tree too (config.h), not only on our flag list.
    "$PA_MPV_ROOT/check-license.sh" --config "$BUILD_DIR/mpv" || die "licence check failed on configured mpv"
}

# media_kit's JNI helper for Android (MIT): one C++ file, built with the NDK
# like the upstream CMake project (log + android, 16 KB pages, static libc++).
build_android_helper() {
    [ "$T_OS" = android ] || return 0
    local src
    src="$(mpv_unpack mediakit_helper "$SRC_DIR")"
    log "build: libmediakitandroidhelper.so"
    # shellcheck disable=SC2086 # flag lists
    "$CXX" $T_CFLAGS -std=c++17 -shared \
        -o "$OUT_DIR/lib/libmediakitandroidhelper.so" "$src/app/src/main/cpp/native-lib.cpp" \
        $T_LDFLAGS -Wl,-soname,libmediakitandroidhelper.so -Wl,--no-undefined \
        -static-libstdc++ -llog -landroid >&2 || die "media_kit android helper build failed"
}

# Extra link arguments for libmpv, written to an additional meson machine file
# (it overrides c_link_args of the cross file / LDFLAGS).
write_mpv_link_file() { # <file>
    local args="$T_LDFLAGS"
    case "$T_OS" in
        # The library looks for the FFmpeg libraries next to itself. The C++
        # runtime for libplacebo's C++ parts (its static .pc does not list it):
        # system libstdc++ on Linux, static on MinGW, libc++ on macOS.
        linux) args="$args -Wl,-rpath,\$ORIGIN -Wl,--no-undefined -lstdc++" ;;
        windows) args="$args -Wl,-Bstatic -lstdc++ -Wl,-Bdynamic" ;;
        darwin) args="$args -lc++" ;;
        # Unresolved symbols must fail the link, not the dlopen() on a phone.
        # libplacebo has C++ parts: link the NDK's static libc++ into libmpv.
        android) args="$args -Wl,--no-undefined -lc++_static -lc++abi" ;;
    esac
    local out="" w cflags=""
    for w in $args; do out="$out'$w', "; done
    {
        # mpv enables Objective-C on macOS (add_languages('objc')): the cross
        # file must name the compiler, with the same target and sysroot.
        if [ "$T_OS" = darwin ]; then
            for w in $T_CFLAGS; do cflags="$cflags'$w', "; done
            printf "[binaries]\nobjc = '%s'\n\n" "$CC"
        fi
        printf '[built-in options]\nc_link_args = [%s]\n' "${out%, }"
        if [ "$T_OS" = darwin ]; then
            printf 'objc_args = [%s]\nobjc_link_args = [%s]\n' "${cflags%, }" "${out%, }"
        fi
    } > "$1"
}
