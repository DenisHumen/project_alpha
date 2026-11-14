# SPDX-License-Identifier: MPL-2.0
# Per-target toolchain setup. setup_target <name> exports:
#   T_OS          linux | windows | android | darwin
#   T_KIND        linux | windows | android | macos | ios | ios-sim | tvos | tvos-sim
#   T_RUNNABLE    1 if the artifact can be executed on this host (runtime check)
#   T_FRAMEWORK   1 if the library is wrapped into an embedded smb2.framework
#   T_CMAKE_FLAGS CMake flags selecting the toolchain, sysroot and ABI
#   T_CFLAGS T_LDFLAGS
#   CC (and OBJDUMP for PE targets, used by check-license.sh)
#
# Target names and minimum OS versions are the same as tools/ffmpeg, so one
# client target picks the same slice of every native library.

PA_SMB2_TARGETS="linux-x86_64 windows-x86_64 \
android-arm64-v8a android-armeabi-v7a android-x86_64 \
macos-arm64 macos-x86_64 ios-arm64 ios-sim-arm64 tvos-arm64 tvos-sim-arm64"

: "${PA_SMB2_ANDROID_API:=24}"     # minSdk of the Android client
: "${PA_SMB2_MACOS_MIN:=11.0}"
: "${PA_SMB2_IOS_MIN:=15.0}"
: "${PA_SMB2_TVOS_MIN:=15.0}"

_reset_target() {
    T_OS="" T_KIND="" T_RUNNABLE=0 T_FRAMEWORK=0
    T_CMAKE_FLAGS="" T_CFLAGS="-O2" T_LDFLAGS=""
}

_setup_linux_native() {
    T_OS=linux T_KIND=linux T_RUNNABLE=1
    CC="${CC:-gcc}"
}

_setup_windows() {
    local p=x86_64-w64-mingw32
    T_OS=windows T_KIND=windows
    CC=$p-gcc OBJDUMP=$p-objdump
    # libsmb2 keys its Windows code paths on _WINDOWS, which only MSVC defines
    # by default. Static libgcc keeps the DLL free of libgcc_s_seh-1.dll.
    T_CFLAGS="-O2 -D_WINDOWS"
    T_LDFLAGS="-static-libgcc"
    T_CMAKE_FLAGS="-DCMAKE_SYSTEM_NAME=Windows -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
-DCMAKE_C_COMPILER=$p-gcc -DCMAKE_RC_COMPILER=$p-windres \
-DCMAKE_FIND_ROOT_PATH=/usr/$p -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
-DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY"
}

_setup_android() {
    local abi="$1"
    local ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
    [ -n "$ndk" ] && [ -d "$ndk" ] || die "set ANDROID_NDK_HOME to an Android NDK (r27+) for $abi"
    T_OS=android T_KIND=android
    CC=""
    # 16 KB page alignment is required for apps targeting Android 15+.
    T_LDFLAGS="-Wl,-z,max-page-size=16384"
    T_CMAKE_FLAGS="-DCMAKE_TOOLCHAIN_FILE=$ndk/build/cmake/android.toolchain.cmake \
-DANDROID_ABI=$abi -DANDROID_PLATFORM=android-$PA_SMB2_ANDROID_API"
}

_setup_apple() {
    local kind="$1" arch="$2" sdk system min
    command -v xcrun >/dev/null 2>&1 || die "Apple targets must be built on macOS with Xcode"
    case "$kind" in
        macos)    sdk=macosx           system=Darwin min=$PA_SMB2_MACOS_MIN ;;
        ios)      sdk=iphoneos         system=iOS    min=$PA_SMB2_IOS_MIN ;;
        ios-sim)  sdk=iphonesimulator  system=iOS    min=$PA_SMB2_IOS_MIN ;;
        tvos)     sdk=appletvos        system=tvOS   min=$PA_SMB2_TVOS_MIN ;;
        tvos-sim) sdk=appletvsimulator system=tvOS   min=$PA_SMB2_TVOS_MIN ;;
        *) die "unknown apple kind $kind" ;;
    esac
    T_OS=darwin T_KIND=$kind
    CC="$(xcrun --sdk "$sdk" --find clang)"
    # A macOS slice matching the build host can be loaded for the runtime check.
    [ "$kind" = macos ] && [ "$(uname -m)" = "$arch" ] && T_RUNNABLE=1
    # iOS and tvOS ship it as an embedded dynamic framework (App Store rules
    # allow dynamic code only in frameworks inside the app bundle).
    [ "$kind" != macos ] && T_FRAMEWORK=1
    # @rpath install name: the app (or the framework wrapper) sets the rpath.
    T_CMAKE_FLAGS="-DCMAKE_SYSTEM_NAME=$system -DCMAKE_OSX_SYSROOT=$(xcrun --sdk "$sdk" --show-sdk-path) \
-DCMAKE_OSX_ARCHITECTURES=$arch -DCMAKE_OSX_DEPLOYMENT_TARGET=$min \
-DCMAKE_C_COMPILER=$CC -DCMAKE_INSTALL_NAME_DIR=@rpath -DCMAKE_MACOSX_RPATH=ON"
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
        *) die "unknown target '$1' (known: $PA_SMB2_TARGETS)" ;;
    esac
    export CC
}
