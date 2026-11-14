#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# LGPL-2.1+ libsmb2 build for project_alpha clients (NET-002): a shared
# library for every client target (embedded smb2.framework on iOS and tvOS).
#
# Usage:
#   build.sh list              list known targets
#   build.sh fetch             download and verify the pinned source
#   build.sh <target>          build libsmb2 (shared) for <target>
#   build.sh xcframework       pack built ios/tvos slices into smb2.xcframework (macOS)
#   build.sh sources           pack the corresponding-source bundle (LGPL §6)
#
# Environment:
#   PA_SMB2_JOBS          parallel jobs (default: all cores)
#   PA_SMB2_CACHE         download cache   (default: tools/libsmb2/.cache)
#   PA_SMB2_WORK          build tree       (default: tools/libsmb2/build)
#   PA_SMB2_OUT           artifacts        (default: tools/libsmb2/out)
#   PA_SMB2_EXTRA_FLAGS   extra CMake flags (still checked by the licence gate)
#   ANDROID_NDK_HOME      Android NDK for android-* targets
set -euo pipefail

PA_SMB2_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${PA_SMB2_CACHE:=$PA_SMB2_ROOT/.cache}"
: "${PA_SMB2_WORK:=$PA_SMB2_ROOT/build}"
: "${PA_SMB2_OUT:=$PA_SMB2_ROOT/out}"
export PA_SMB2_ROOT PA_SMB2_CACHE

# shellcheck source=versions.sh
. "$PA_SMB2_ROOT/versions.sh"
# shellcheck source=lib/common.sh
. "$PA_SMB2_ROOT/lib/common.sh"
# shellcheck source=lib/targets.sh
. "$PA_SMB2_ROOT/lib/targets.sh"
# shellcheck source=lib/smb2.sh
. "$PA_SMB2_ROOT/lib/smb2.sh"

install_licenses() {
    local dst="$OUT_DIR/share/licenses"
    mkdir -p "$dst"
    cp "$SRC_DIR/libsmb2/COPYING" "$dst/libsmb2-COPYING"
    cp "$SRC_DIR/libsmb2/LICENCE-LGPL-2.1.txt" "$dst/libsmb2-LICENCE-LGPL-2.1.txt"
}

write_buildinfo() {
    local f="$OUT_DIR/BUILDINFO.txt"
    {
        echo "project_alpha LGPL libsmb2 build (tools/libsmb2, NET-002)"
        echo "target: $TARGET"
        echo "license: $LIBSMB2_LICENSE (shared library, dynamically linked)"
        echo "libsmb2 $LIBSMB2_VERSION sha256=$LIBSMB2_SHA256"
        echo "url: $LIBSMB2_URL"
        echo
        echo "patches:"
        (cd "$PA_SMB2_ROOT/patches" && find . -name '*.patch' | sort | sed 's/^/  /') || true
        echo
        echo "cmake: $T_CMAKE_FLAGS $SMB2_COMMON_FLAGS ${PA_SMB2_EXTRA_FLAGS:-}" | tr -s ' '
        echo "cflags: $T_CFLAGS"
        echo "ldflags: $T_LDFLAGS"
        echo
        echo "source: https://github.com/DenisHumen/project_alpha/tree/main/tools/libsmb2"
    } > "$f"
    log "wrote $f"
}

build_target() {
    TARGET="$1"
    setup_target "$TARGET"
    # Reject a static build before spending time on it.
    # shellcheck disable=SC2086
    assert_dynamic_flags $SMB2_COMMON_FLAGS $T_CMAKE_FLAGS ${PA_SMB2_EXTRA_FLAGS:-}
    SRC_DIR="$PA_SMB2_WORK/$TARGET/src"
    BUILD_DIR="$PA_SMB2_WORK/$TARGET/build"
    OUT_DIR="$PA_SMB2_OUT/$TARGET"
    mkdir -p "$SRC_DIR"

    log "target $TARGET (os=$T_OS kind=$T_KIND jobs=$(jobs))"
    build_smb2
    install_licenses
    write_buildinfo

    OBJDUMP="${OBJDUMP:-}" "$PA_SMB2_ROOT/check-license.sh" --artifact "$OUT_DIR"
    if [ "$T_RUNNABLE" = 1 ]; then
        CFLAGS="$T_CFLAGS" "$PA_SMB2_ROOT/check-license.sh" --run "$OUT_DIR"
    fi
    log "done: $OUT_DIR"
}

# xcframework: one smb2.xcframework from every ios/tvos slice already built.
pack_xcframework() {
    command -v xcodebuild >/dev/null 2>&1 || die "xcframework needs macOS with Xcode"
    local args=() t
    for t in ios-arm64 ios-sim-arm64 tvos-arm64 tvos-sim-arm64; do
        [ -d "$PA_SMB2_OUT/$t/smb2.framework" ] && args+=(-framework "$PA_SMB2_OUT/$t/smb2.framework")
    done
    [ "${#args[@]}" -gt 0 ] || die "no ios/tvos slices in $PA_SMB2_OUT (build them first)"
    rm -rf "$PA_SMB2_OUT/smb2.xcframework"
    xcodebuild -create-xcframework "${args[@]}" -output "$PA_SMB2_OUT/smb2.xcframework" >&2
    log "wrote $PA_SMB2_OUT/smb2.xcframework"
}

pack_sources() {
    local name="project_alpha-libsmb2-sources-${LIBSMB2_VERSION}"
    local stage="$PA_SMB2_WORK/$name" pkg file
    rm -rf "$stage"; mkdir -p "$stage/upstream"
    for pkg in $PA_SMB2_PACKAGES; do
        file="$(fetch "$pkg")"
        cp "$file" "$stage/upstream/"
    done
    # Our build scripts and patches, i.e. everything needed to rebuild/relink.
    tar -C "$PA_SMB2_ROOT" --exclude=.cache --exclude=build --exclude=out -cf - . \
        | { mkdir -p "$stage/tools-libsmb2" && tar -C "$stage/tools-libsmb2" -xf -; }
    (cd "$stage/upstream" && for file in *; do printf '%s  %s\n' "$(sha256_of "$file")" "$file"; done) \
        > "$stage/upstream/SHA256SUMS"
    mkdir -p "$PA_SMB2_OUT"
    tar -C "$PA_SMB2_WORK" -cJf "$PA_SMB2_OUT/$name.tar.xz" "$name"
    log "wrote $PA_SMB2_OUT/$name.tar.xz"
}

case "${1:-}" in
    list) printf '%s\n' $PA_SMB2_TARGETS ;;
    fetch) for p in $PA_SMB2_PACKAGES; do fetch "$p" >/dev/null; done; log "all sources verified" ;;
    xcframework) pack_xcframework ;;
    sources) pack_sources ;;
    ""|-h|--help) sed -n '4,20p' "$0" ;;
    *) build_target "$1" ;;
esac
