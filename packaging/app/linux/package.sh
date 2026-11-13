#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Package a Flutter Linux release bundle (R01-REL).
#
#   package.sh <bundle-dir> <version> <out-dir>
#
# Writes to <out-dir>:
#   kinolune-<version>-linux-x86_64.tar.gz   the bundle plus licences
#   kinolune-<version>-x86_64.AppImage       the same bundle as an AppImage
#
# libmpv is NOT bundled by default: media_kit on Linux links the system libmpv
# (see packaging/app/README.md). With PA_LIBMPV_DIR=<libmpv-linux-x86_64
# artifact> (tools/mpv, PLAY-002) our LGPL libmpv + FFmpeg go into lib/ via
# tools/mpv/bundle.sh (needs patchelf), with their licences.
#
# Needs: tar, gzip, curl, sha256sum. appimagetool and the AppImage runtime are
# downloaded at pinned versions and verified by SHA-256 (override the cache
# directory with PA_APPIMAGE_TOOLS).
set -euo pipefail

APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"
RUNTIME_SHA256="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"

APP_ID="app.kinolune"
APP_NAME="Kinolune"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app_pkg="$(cd "$here/.." && pwd)"
repo_root="$(cd "$app_pkg/../.." && pwd)"

die() { printf 'package.sh: %s\n' "$*" >&2; exit 1; }

[ $# -eq 3 ] || { sed -n '4,17p' "$0"; exit 2; }
bundle="$(cd "$1" && pwd)"
version="$2"
mkdir -p "$3"
out="$(cd "$3" && pwd)"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "bad version: $version"
{ [ -d "$bundle/lib" ] && [ -d "$bundle/data" ]; } || die "$bundle is not a Flutter Linux bundle"

# The runner binary is the only executable regular file at the bundle root.
mapfile -t exes < <(find "$bundle" -maxdepth 1 -type f -perm -u+x -printf '%f\n')
[ "${#exes[@]}" -eq 1 ] || die "expected one executable in $bundle, found: ${exes[*]:-none}"
exe="${exes[0]}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Optional LGPL libmpv hook (PLAY-002): bundle a copy, never touch the input.
if [ -n "${PA_LIBMPV_DIR:-}" ]; then
    cp -a "$bundle" "$work/bundle" && "$repo_root/tools/mpv/bundle.sh" linux "$PA_LIBMPV_DIR" "$work/bundle" && bundle="$work/bundle"
fi

fetch() { # url sha256 dest
    local url="$1" sum="$2" dest="$3"
    if [ ! -f "$dest" ] || ! printf '%s  %s\n' "$sum" "$dest" | sha256sum -c --quiet - 2>/dev/null; then
        curl -fsSL --retry 3 -o "$dest.part" "$url"
        printf '%s  %s\n' "$sum" "$dest.part" | sha256sum -c --quiet - || die "checksum mismatch: $url"
        mv "$dest.part" "$dest"
    fi
}

# Licence payload shared by both formats.
add_docs() { # dest-dir
    local d="$1"
    mkdir -p "$d/licenses"
    cp "$repo_root/LICENSE" "$d/LICENSE"
    "$app_pkg/fetch-licences.sh" "$d/licenses"
    cp "$app_pkg/THIRD-PARTY-NOTICES.md" "$d/THIRD-PARTY-NOTICES.md"
    # Flutter collects the licences of the engine and every Dart package here.
    if [ -f "$bundle/data/flutter_assets/NOTICES.Z" ]; then
        gzip -dc "$bundle/data/flutter_assets/NOTICES.Z" > "$d/licenses/FLUTTER-NOTICES.txt"
    elif [ -f "$bundle/data/flutter_assets/NOTICES" ]; then
        cp "$bundle/data/flutter_assets/NOTICES" "$d/licenses/FLUTTER-NOTICES.txt"
    fi
}

# --- tar.gz ------------------------------------------------------------------
name="kinolune-${version}-linux-x86_64"
stage="$work/$name"
mkdir -p "$stage"
cp -a "$bundle/." "$stage/"
add_docs "$stage"
cp "$here/README-linux.txt" "$stage/README.txt"
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$work" -czf "$out/$name.tar.gz" "$name"
printf 'wrote %s\n' "$out/$name.tar.gz"

# --- AppImage ----------------------------------------------------------------
tools="${PA_APPIMAGE_TOOLS:-$work/tools}"
mkdir -p "$tools"
fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256" "$tools/appimagetool"
fetch "$RUNTIME_URL" "$RUNTIME_SHA256" "$tools/runtime-x86_64"
chmod +x "$tools/appimagetool"

appdir="$work/AppDir"
mkdir -p "$appdir/usr/lib/kinolune" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/scalable/apps" "$appdir/usr/share/doc/kinolune"
cp -a "$bundle/." "$appdir/usr/lib/kinolune/"
add_docs "$appdir/usr/share/doc/kinolune"
sed "s/@EXEC@/$exe/" "$here/AppRun" > "$appdir/AppRun"
chmod +x "$appdir/AppRun"
cp "$here/$APP_ID.desktop" "$appdir/$APP_ID.desktop"
cp "$here/$APP_ID.desktop" "$appdir/usr/share/applications/$APP_ID.desktop"
cp "$here/$APP_ID.svg" "$appdir/$APP_ID.svg"
cp "$here/$APP_ID.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
ln -s "$APP_ID.svg" "$appdir/.DirIcon"

appimage="$out/kinolune-${version}-x86_64.AppImage"
# appimagetool is itself an AppImage; extract-and-run avoids needing FUSE on CI.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 VERSION="$version" \
    "$tools/appimagetool" --no-appstream --runtime-file "$tools/runtime-x86_64" "$appdir" "$appimage"
chmod +x "$appimage"
printf 'wrote %s (%s)\n' "$appimage" "$APP_NAME"
