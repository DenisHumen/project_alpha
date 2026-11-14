# SPDX-License-Identifier: MPL-2.0
# The single place with the libsmb2 CMake options and the list of forbidden
# ones. Sourced by build.sh and check-license.sh.

# Always a shared library: libsmb2 is LGPL-2.1+, and the project links LGPL
# code only dynamically (ROADMAP §0.3 p.10, ADR-0005). Kerberos is off: the
# built-in NTLMSSP covers NAS and Windows shares without pulling libkrb5 or
# GSS.framework into every client.
SMB2_COMMON_FLAGS="-DBUILD_SHARED_LIBS=ON -DENABLE_EXAMPLES=OFF \
-DENABLE_LIBKRB5=OFF -DENABLE_GSSAPI=OFF -DCMAKE_BUILD_TYPE=Release \
-DCMAKE_POSITION_INDEPENDENT_CODE=ON"

# Options that would produce a static library or link a dependency that the
# licence review has not covered.
SMB2_FORBIDDEN_RE='-DBUILD_SHARED_LIBS(:BOOL)?=(OFF|0|FALSE|NO)|-DENABLE_(LIBKRB5|GSSAPI)(:BOOL)?=(ON|1|TRUE|YES)'

# Functions the Rust wrapper (core/crates/pa-smb2) loads at runtime. The
# licence gate checks they are exported, so a library that loads also works.
SMB2_REQUIRED_SYMBOLS="smb2_init_context smb2_destroy_context smb2_set_timeout \
smb2_set_version smb2_set_security_mode smb2_set_seal smb2_set_sign \
smb2_set_authentication smb2_set_user smb2_set_password smb2_set_domain \
smb2_set_workstation smb2_connect_share smb2_disconnect_share smb2_get_error \
smb2_get_nterror smb2_get_dialect smb2_get_max_read_size smb2_opendir \
smb2_readdir smb2_closedir smb2_open smb2_close smb2_pread smb2_fstat smb2_stat"

# assert_dynamic_flags <flags...>: dies on a forbidden option.
assert_dynamic_flags() {
    local f
    for f in "$@"; do
        if printf '%s\n' "$f" | grep -Eq -- "^($SMB2_FORBIDDEN_RE)$"; then
            die "forbidden libsmb2 option (static build or unreviewed dependency): $f"
        fi
    done
}

build_smb2() {
    local src
    src="$(unpack libsmb2 "$SRC_DIR")"
    rm -rf "$BUILD_DIR" "$OUT_DIR"
    mkdir -p "$BUILD_DIR"
    log "configuring libsmb2 $LIBSMB2_VERSION"
    # shellcheck disable=SC2086 # flag lists are word lists
    CFLAGS="$T_CFLAGS" LDFLAGS="$T_LDFLAGS" cmake -S "$src" -B "$BUILD_DIR" -G Ninja \
        -DCMAKE_INSTALL_PREFIX="$OUT_DIR" \
        -DCMAKE_SHARED_LINKER_FLAGS="$T_LDFLAGS" \
        $T_CMAKE_FLAGS $SMB2_COMMON_FLAGS ${PA_SMB2_EXTRA_FLAGS:-} >&2
    "$PA_SMB2_ROOT/check-license.sh" --config "$BUILD_DIR"
    cmake --build "$BUILD_DIR" -j "$(jobs)" >&2
    cmake --install "$BUILD_DIR" --strip >&2
    # CMake package files and pkg-config are not needed by the clients.
    rm -rf "$OUT_DIR/lib/cmake" "$OUT_DIR/lib/pkgconfig"
    if [ "$T_FRAMEWORK" = 1 ]; then make_framework; fi
}

# make_framework: wrap the dylib into smb2.framework for iOS and tvOS.
# Flat (iOS-style) bundle; install name @rpath/smb2.framework/smb2. Assemble
# slices into an XCFramework with `build.sh xcframework`.
make_framework() {
    local fw="$OUT_DIR/smb2.framework" dylib plat
    dylib="$(find "$OUT_DIR/lib" -name 'libsmb2.*.dylib' -type f | head -n1)"
    [ -n "$dylib" ] || die "no libsmb2 dylib to wrap"
    case "$T_KIND" in
        ios) plat=iPhoneOS ;; ios-sim) plat=iPhoneSimulator ;;
        tvos) plat=AppleTVOS ;; tvos-sim) plat=AppleTVSimulator ;;
    esac
    local min=$PA_SMB2_IOS_MIN
    case "$T_KIND" in tvos*) min=$PA_SMB2_TVOS_MIN ;; esac
    rm -rf "$fw"; mkdir -p "$fw/Headers"
    cp "$dylib" "$fw/smb2"
    install_name_tool -id @rpath/smb2.framework/smb2 "$fw/smb2"
    cp "$OUT_DIR"/include/smb2/*.h "$fw/Headers/"
    cat > "$fw/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>smb2</string>
  <key>CFBundleIdentifier</key><string>app.kinolune.libsmb2</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>smb2</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>$LIBSMB2_VERSION</string>
  <key>CFBundleVersion</key><string>$LIBSMB2_VERSION</string>
  <key>CFBundleSupportedPlatforms</key><array><string>$plat</string></array>
  <key>MinimumOSVersion</key><string>$min</string>
</dict>
</plist>
PLIST
    # The framework replaces the bare dylib for these targets.
    rm -rf "$OUT_DIR/lib"
}
