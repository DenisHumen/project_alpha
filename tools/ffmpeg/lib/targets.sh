# SPDX-License-Identifier: MPL-2.0
# Per-target toolchain setup. setup_target <name> exports:
#   T_OS        linux | windows | android | darwin        (meson host_machine.system)
#   T_CPU_FAMILY, T_CPU                                   (meson cpu_family / cpu)
#   T_CROSS     1 if cross-compiling
#   CC CXX AR NM STRIP RANLIB [WINDRES]
#   T_CFLAGS T_LDFLAGS                                    (applied to deps and FFmpeg)
#   FF_TARGET_FLAGS   FFmpeg configure flags specific to the target (arch, os, hwaccel)
#   ASS_FONT_FLAGS    libass meson flags for the platform font provider
#
# linux-x86_64 (built + run) and windows-x86_64 (cross-built) are verified in the
# Docker image; the rest are written against the
# documented toolchains and must be verified in CI (see README.md).

PA_FF_TARGETS="linux-x86_64 windows-x86_64 \
android-arm64-v8a android-armeabi-v7a android-x86_64 \
macos-arm64 macos-x86_64 ios-arm64 ios-sim-arm64 tvos-arm64 tvos-sim-arm64"

: "${PA_FF_ANDROID_API:=24}"     # minSdk of the Android client
: "${PA_FF_MACOS_MIN:=11.0}"
: "${PA_FF_IOS_MIN:=15.0}"
: "${PA_FF_TVOS_MIN:=15.0}"

_reset_target() {
    T_OS="" T_CPU_FAMILY="" T_CPU="" T_CROSS=0
    T_CFLAGS="-O2" T_LDFLAGS="" FF_TARGET_FLAGS="" ASS_FONT_FLAGS=""
    WINDRES=""
}

_setup_linux_native() {
    T_OS=linux T_CPU_FAMILY=x86_64 T_CPU=x86_64
    CC="${CC:-gcc}" CXX="${CXX:-g++}" AR="${AR:-ar}" NM="${NM:-nm}"
    STRIP="${STRIP:-strip}" RANLIB="${RANLIB:-ranlib}"
    # vaapi + libdrm are linked dynamically against the system libva/libdrm (MIT).
    FF_TARGET_FLAGS="--enable-pthreads --enable-vaapi --enable-libdrm"
    ASS_FONT_FLAGS="-Dfontconfig=disabled -Ddirectwrite=disabled -Dcoretext=disabled"
}

_setup_windows() {
    local p=x86_64-w64-mingw32
    T_OS=windows T_CPU_FAMILY=x86_64 T_CPU=x86_64 T_CROSS=1
    CC=$p-gcc CXX=$p-g++ AR=$p-ar NM=$p-nm STRIP=$p-strip RANLIB=$p-ranlib WINDRES=$p-windres
    # Static libgcc/libstdc++/winpthread so the DLLs only depend on system DLLs.
    T_LDFLAGS="-static-libgcc -static-libstdc++"
    FF_TARGET_FLAGS="--enable-cross-compile --target-os=mingw32 --arch=x86_64 --cross-prefix=$p- \
--enable-w32threads --enable-d3d11va --enable-d3d12va --enable-dxva2"
    ASS_FONT_FLAGS="-Dfontconfig=disabled -Ddirectwrite=enabled -Dcoretext=disabled"
}

_setup_android() {
    local abi="$1" clang_triple arch
    local ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
    [ -n "$ndk" ] && [ -d "$ndk" ] || die "set ANDROID_NDK_HOME to an Android NDK (r27+) for $abi"
    local host_tag
    case "$(uname -s)" in
        Linux) host_tag=linux-x86_64 ;;
        Darwin) host_tag=darwin-x86_64 ;;   # NDK ships a universal binary under this tag
        *) die "unsupported NDK host" ;;
    esac
    local bin="$ndk/toolchains/llvm/prebuilt/$host_tag/bin"
    case "$abi" in
        arm64-v8a)   clang_triple=aarch64-linux-android   arch=aarch64 T_CPU_FAMILY=aarch64 T_CPU=armv8-a ;;
        armeabi-v7a) clang_triple=armv7a-linux-androideabi arch=arm     T_CPU_FAMILY=arm     T_CPU=armv7-a ;;
        x86_64)      clang_triple=x86_64-linux-android    arch=x86_64  T_CPU_FAMILY=x86_64  T_CPU=x86_64 ;;
        *) die "unknown android abi $abi" ;;
    esac
    T_OS=android T_CROSS=1
    CC="$bin/${clang_triple}${PA_FF_ANDROID_API}-clang"
    CXX="$bin/${clang_triple}${PA_FF_ANDROID_API}-clang++"
    AR="$bin/llvm-ar" NM="$bin/llvm-nm" STRIP="$bin/llvm-strip" RANLIB="$bin/llvm-ranlib"
    T_CFLAGS="-O2 -fPIC"
    [ "$abi" = armeabi-v7a ] && T_CFLAGS="$T_CFLAGS -mfpu=neon -mfloat-abi=softfp"
    # 16 KB page alignment is required for apps targeting Android 15+.
    T_LDFLAGS="-Wl,-z,max-page-size=16384"
    FF_TARGET_FLAGS="--enable-cross-compile --target-os=android --arch=$arch \
--cc=$CC --cxx=$CXX --ar=$AR --nm=$NM --strip=$STRIP --ranlib=$RANLIB \
--enable-pthreads --enable-jni --enable-mediacodec"
    ASS_FONT_FLAGS="-Dfontconfig=disabled -Ddirectwrite=disabled -Dcoretext=disabled"
}

_setup_apple() {
    local kind="$1" arch="$2" sdk triple
    command -v xcrun >/dev/null 2>&1 || die "Apple targets must be built on macOS with Xcode"
    case "$kind" in
        macos)   sdk=macosx           triple="$arch-apple-macos${PA_FF_MACOS_MIN}" ;;
        ios)     sdk=iphoneos         triple="$arch-apple-ios${PA_FF_IOS_MIN}" ;;
        ios-sim) sdk=iphonesimulator  triple="$arch-apple-ios${PA_FF_IOS_MIN}-simulator" ;;
        tvos)    sdk=appletvos        triple="$arch-apple-tvos${PA_FF_TVOS_MIN}" ;;
        tvos-sim) sdk=appletvsimulator triple="$arch-apple-tvos${PA_FF_TVOS_MIN}-simulator" ;;
        *) die "unknown apple kind $kind" ;;
    esac
    local sysroot
    sysroot="$(xcrun --sdk "$sdk" --show-sdk-path)"
    T_OS=darwin T_CROSS=1
    case "$arch" in
        arm64)  T_CPU_FAMILY=aarch64 T_CPU=arm64 ;;
        x86_64) T_CPU_FAMILY=x86_64  T_CPU=x86_64 ;;
    esac
    CC="$(xcrun --sdk "$sdk" --find clang)"
    CXX="$(xcrun --sdk "$sdk" --find clang++)"
    AR="$(xcrun --sdk "$sdk" --find ar)" NM="$(xcrun --sdk "$sdk" --find nm)"
    STRIP="$(xcrun --sdk "$sdk" --find strip)" RANLIB="$(xcrun --sdk "$sdk" --find ranlib)"
    T_CFLAGS="-O2 -target $triple -isysroot $sysroot"
    T_LDFLAGS="-target $triple -isysroot $sysroot"
    local ffarch=$arch; [ "$arch" = arm64 ] && ffarch=aarch64
    # Shared libs get an @rpath install name so they can be wrapped into
    # (XC)frameworks and embedded in the app bundle (PLAY-APL).
    FF_TARGET_FLAGS="--enable-cross-compile --target-os=darwin --arch=$ffarch \
--cc=$CC --cxx=$CXX --ar=$AR --nm=$NM --strip=$STRIP --ranlib=$RANLIB --sysroot=$sysroot \
--install-name-dir=@rpath --enable-pthreads --enable-videotoolbox --enable-audiotoolbox"
    ASS_FONT_FLAGS="-Dfontconfig=disabled -Ddirectwrite=disabled -Dcoretext=enabled"
}

setup_target() {
    _reset_target
    case "$1" in
        linux-x86_64)        _setup_linux_native ;;
        windows-x86_64)      _setup_windows ;;
        android-arm64-v8a)   _setup_android arm64-v8a ;;
        android-armeabi-v7a) _setup_android armeabi-v7a ;;
        android-x86_64)      _setup_android x86_64 ;;
        macos-arm64)         _setup_apple macos arm64 ;;
        macos-x86_64)        _setup_apple macos x86_64 ;;
        ios-arm64)           _setup_apple ios arm64 ;;
        ios-sim-arm64)       _setup_apple ios-sim arm64 ;;
        tvos-arm64)          _setup_apple tvos arm64 ;;
        tvos-sim-arm64)      _setup_apple tvos-sim arm64 ;;
        *) die "unknown target '$1' (known: $PA_FF_TARGETS)" ;;
    esac
    export CC CXX AR NM STRIP RANLIB
}

# write_meson_cross <file>: meson cross file for the current target.
write_meson_cross() {
    local f="$1" endian=little
    _meson_list() { local out="" w; for w in $1; do out="$out'$w', "; done; printf '[%s]' "${out%, }"; }
    {
        echo "[binaries]"
        echo "c = $(_meson_list "$CC")"
        echo "cpp = $(_meson_list "$CXX")"
        echo "ar = '$AR'"
        echo "nm = '$NM'"
        echo "strip = '$STRIP'"
        [ -n "$WINDRES" ] && echo "windres = '$WINDRES'"
        echo "pkg-config = 'pkg-config'"
        echo
        echo "[built-in options]"
        echo "c_args = $(_meson_list "$T_CFLAGS")"
        echo "cpp_args = $(_meson_list "$T_CFLAGS")"
        echo "c_link_args = $(_meson_list "$T_LDFLAGS")"
        echo "cpp_link_args = $(_meson_list "$T_LDFLAGS")"
        echo
        echo "[host_machine]"
        echo "system = '$T_OS'"
        echo "cpu_family = '$T_CPU_FAMILY'"
        echo "cpu = '$T_CPU'"
        echo "endian = '$endian'"
    } > "$f"
}
