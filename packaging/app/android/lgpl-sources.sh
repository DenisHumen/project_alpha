#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Corresponding-source bundle for the LGPL libraries in the Android APK (R01-REL).
#
#   lgpl-sources.sh <flutter-app-dir> <out.tar> <notice.md>
#
# media_kit_libs_android_video ships a prebuilt libmpv.so (mpv + FFmpeg +
# fribidi and permissive deps linked statically) from
# github.com/media-kit/libmpv-android-video-build. This script finds the exact
# release tag the app's resolved media_kit version downloads, then collects:
#   - that build repository at the tag (build scripts, patches, version pins);
#   - the upstream sources of the copyleft components at the pinned versions
#     (mpv, FFmpeg, fribidi), with SHA256SUMS.
# The tarball is attached to the GitHub Release next to the APKs (LGPL-2.1 §6 /
# LGPL-3.0 §4 "equivalent access to copy the source"). <notice.md> gets a
# human-readable list of components, versions and links.
#
# Run after `flutter pub get` in the app (needs .dart_tool/package_config.json).
set -euo pipefail

die() { printf 'lgpl-sources.sh: %s\n' "$*" >&2; exit 1; }
[ $# -eq 3 ] || { sed -n '4,19p' "$0" >&2; exit 2; }
app="$(cd "$1" && pwd)"
out_tar="$2"
notice="$3"

cfg="$app/.dart_tool/package_config.json"
[ -f "$cfg" ] || die "no $cfg (run flutter pub get first)"

pkg_dir="$(python3 -I - "$cfg" <<'PY'
import json, sys, urllib.parse, os
cfg = sys.argv[1]
d = json.load(open(cfg))
for p in d["packages"]:
    if p["name"] == "media_kit_libs_android_video":
        uri = p["rootUri"]
        if uri.startswith("file://"):
            print(urllib.parse.unquote(uri[len("file://"):]))
        else:
            print(os.path.normpath(os.path.join(os.path.dirname(cfg), uri)))
PY
)"
[ -n "$pkg_dir" ] || die "media_kit_libs_android_video is not a dependency of $app"
gradle="$pkg_dir/android/build.gradle"
[ -f "$gradle" ] || die "no $gradle"

tag="$(grep -oE 'libmpv-android-video-build/releases/download/v[0-9][0-9A-Za-z.-]*/' "$gradle" \
    | sed -E 's#.*/download/([^/]+)/#\1#' | sort -u)"
{ [ "$(printf '%s\n' "$tag" | wc -l)" -eq 1 ] && [ -n "$tag" ]; } || die "cannot find one libmpv build tag in $gradle: '$tag'"
pkg_ver="$(basename "$pkg_dir" | sed 's/^media_kit_libs_android_video-//')"
printf 'media_kit_libs_android_video %s -> libmpv-android-video-build %s\n' "$pkg_ver" "$tag" >&2

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
name="$(basename "$out_tar" .tar)"
stage="$work/$name"
mkdir -p "$stage"

get() { # url file
    printf 'fetch %s\n' "$1" >&2
    curl -fsSL --retry 3 -o "$stage/$2" "$1"
}

build_repo="https://github.com/media-kit/libmpv-android-video-build"
get "https://codeload.github.com/media-kit/libmpv-android-video-build/tar.gz/refs/tags/$tag" \
    "libmpv-android-video-build-$tag.tar.gz"

# Read the pins without executing anything from the download.
tar -xzf "$stage/libmpv-android-video-build-$tag.tar.gz" -C "$work" --wildcards '*/buildscripts/include/depinfo.sh'
depinfo="$(find "$work" -path '*/buildscripts/include/depinfo.sh' | head -n1)"
pin() {
    local v
    v="$(grep -E "^v_$1=" "$depinfo" | head -n1 | cut -d= -f2-)"
    [[ "$v" =~ ^[0-9A-Za-z._-]+$ ]] || die "unexpected pin v_$1='$v' in depinfo.sh"
    printf '%s' "$v"
}
v_ffmpeg="$(pin ffmpeg)"
v_mpv="$(pin mpv)"
v_fribidi="$(pin fribidi)"
v_libass="$(pin libass)"
v_freetype="$(pin freetype)"
v_harfbuzz="$(pin harfbuzz)"
v_mbedtls="$(pin mbedtls)"
v_dav1d="$(pin dav1d)"
v_libxml2="$(pin libxml2)"

get "https://ffmpeg.org/releases/ffmpeg-$v_ffmpeg.tar.xz" "ffmpeg-$v_ffmpeg.tar.xz"
get "https://codeload.github.com/mpv-player/mpv/tar.gz/$v_mpv" "mpv-$v_mpv.tar.gz"
get "https://github.com/fribidi/fribidi/releases/download/v$v_fribidi/fribidi-$v_fribidi.tar.xz" \
    "fribidi-$v_fribidi.tar.xz"

cat > "$stage/README.md" <<EOF
# Corresponding source: LGPL libraries in the Kinolune Android APK

The APK contains \`lib/<abi>/libmpv.so\` from media_kit_libs_android_video $pkg_ver,
built by $build_repo at tag \`$tag\` (FFmpeg configured with --disable-gpl
--disable-nonfree --enable-version3, mpv with -Dgpl=false).

| File | What |
|---|---|
| libmpv-android-video-build-$tag.tar.gz | build scripts, patches and version pins used for the binaries |
| ffmpeg-$v_ffmpeg.tar.xz | FFmpeg $v_ffmpeg (LGPL-3.0-or-later as configured) |
| mpv-$v_mpv.tar.gz | mpv commit $v_mpv (LGPL-2.1-or-later with -Dgpl=false) |
| fribidi-$v_fribidi.tar.xz | GNU FriBidi $v_fribidi (LGPL-2.1-or-later) |

To rebuild: unpack the build repository and follow its README
(\`buildscripts/download.sh\`, then \`buildscripts/build.sh\`). To relink the app
with a modified libmpv.so, replace \`lib/<abi>/libmpv.so\` in the APK and re-sign
it with your own key (\`apksigner sign\`).
EOF
(cd "$stage" && sha256sum -- * > SHA256SUMS)
mkdir -p "$(dirname "$out_tar")"
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$work" -cf "$out_tar" "$name"
printf 'wrote %s\n' "$out_tar" >&2

cat > "$notice" <<EOF
### Android: libmpv / FFmpeg (media_kit_libs_android_video $pkg_ver)

Prebuilt \`libmpv.so\` from [libmpv-android-video-build $tag]($build_repo/tree/$tag),
dynamically loaded by the app. Corresponding sources: \`$(basename "$out_tar")\` in this release.

| Component | Version | Licence |
|---|---|---|
| mpv | $v_mpv | LGPL-2.1-or-later (-Dgpl=false) |
| FFmpeg | $v_ffmpeg | LGPL-3.0-or-later (--enable-version3, no GPL/nonfree) |
| GNU FriBidi | $v_fribidi | LGPL-2.1-or-later |
| libass | $v_libass | ISC |
| FreeType | $v_freetype | FTL |
| HarfBuzz | $v_harfbuzz | MIT |
| Mbed TLS | $v_mbedtls | Apache-2.0 |
| dav1d | $v_dav1d | BSD-2-Clause |
| libxml2 | $v_libxml2 | MIT |
EOF
