# SPDX-License-Identifier: MPL-2.0
#
# Pinned upstream sources for the LGPL FFmpeg build (PLAY-001).
# Every tarball is verified against its SHA-256 before use. Bumping a version
# means updating the version, URL and hash together in one commit.
#
# Format per package: <PKG>_VERSION, <PKG>_URL, <PKG>_SHA256, <PKG>_LICENSE
# (licence of the package itself, for BUILDINFO and NOTICE generation).

FFMPEG_VERSION=9.0.2
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
FFMPEG_LICENSE="LGPL-2.1-or-later"

DAV1D_VERSION=1.5.4
DAV1D_URL="https://downloads.videolan.org/pub/videolan/dav1d/${DAV1D_VERSION}/dav1d-${DAV1D_VERSION}.tar.xz"
DAV1D_SHA256=686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5
DAV1D_LICENSE="BSD-2-Clause"

LIBASS_VERSION=0.17.5
LIBASS_URL="https://github.com/libass/libass/releases/download/${LIBASS_VERSION}/libass-${LIBASS_VERSION}.tar.xz"
LIBASS_SHA256=2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc
LIBASS_LICENSE="ISC"

FRIBIDI_VERSION=1.0.18
FRIBIDI_URL="https://github.com/fribidi/fribidi/releases/download/v${FRIBIDI_VERSION}/fribidi-${FRIBIDI_VERSION}.tar.xz"
FRIBIDI_SHA256=1de5e8073d035388b71d0b2d452b1da1f8caeea8a7702997b401fe1b6250727f
FRIBIDI_LICENSE="LGPL-2.1-or-later"

HARFBUZZ_VERSION=15.1.0
HARFBUZZ_URL="https://github.com/harfbuzz/harfbuzz/releases/download/${HARFBUZZ_VERSION}/harfbuzz-${HARFBUZZ_VERSION}.tar.xz"
HARFBUZZ_SHA256=0fc7c565443a5f4be7da65ac5ee5bd737d9b158dc908708b7689a0a5796b0c8d
HARFBUZZ_LICENSE="MIT"

# FreeType is dual-licensed FTL / GPL-2.0-or-later; we use it under the FTL.
FREETYPE_VERSION=2.15.0
FREETYPE_URL="https://download.savannah.gnu.org/releases/freetype/freetype-${FREETYPE_VERSION}.tar.xz"
FREETYPE_SHA256=93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5
FREETYPE_LICENSE="FTL"

# zlib is only built from source for targets whose SDK does not ship it (Windows).
ZLIB_VERSION=1.4
ZLIB_URL="https://github.com/madler/zlib/releases/download/v${ZLIB_VERSION}/zlib-${ZLIB_VERSION}.tar.xz"
ZLIB_SHA256=4f11c512053d7bab8d9e9b89bde3575add3e4c50a383b42ef6cb837035fc5a7c
ZLIB_LICENSE="Zlib"

# Order matters: dependencies first.
PA_FFMPEG_PACKAGES="zlib fribidi freetype harfbuzz libass dav1d ffmpeg"
