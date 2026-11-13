# SPDX-License-Identifier: MPL-2.0
#
# Pinned upstream sources for the LGPL libmpv build (PLAY-002), on top of the
# FFmpeg sources pinned in tools/ffmpeg/versions.sh. Every tarball is verified
# against its SHA-256 before use. Same format: <PKG>_VERSION, _URL, _SHA256, _LICENSE.
#
# GitHub archive tarballs are used where upstream publishes no release tarball;
# their contents are stable and pinned by hash like any other download.

MPV_VERSION=0.42.0
MPV_URL="https://github.com/mpv-player/mpv/archive/v${MPV_VERSION}/mpv-${MPV_VERSION}.tar.gz"
MPV_SHA256=bcfebbfa2b65a6d88dc97ea18117831072373e38b5785a2afb874ed4df5070dd
# LGPL-2.1-or-later only with -Dgpl=false (the default build is GPL-2.0-or-later).
MPV_LICENSE="LGPL-2.1-or-later"

# mpv 0.42 requires libplacebo >= 7.387.0.
LIBPLACEBO_VERSION=7.387.0
LIBPLACEBO_URL="https://github.com/haasn/libplacebo/archive/v${LIBPLACEBO_VERSION}/libplacebo-${LIBPLACEBO_VERSION}.tar.gz"
LIBPLACEBO_SHA256=f9e62d3774a9c4cfd198ec3abaa88294aaade2e31c04d5ab02bcf4abc98b1df5
LIBPLACEBO_LICENSE="LGPL-2.1-or-later"

# libplacebo's git submodules (3rdparty/), missing from its archive tarball.
# glad generates the OpenGL loader at build time; jinja2 and markupsafe are
# its Python dependencies (build-time only, nothing of them is shipped);
# fast_float is a header-only library compiled into libplacebo.
GLAD_VERSION=2.0.8
GLAD_URL="https://github.com/Dav1dde/glad/archive/v${GLAD_VERSION}/glad-${GLAD_VERSION}.tar.gz"
GLAD_SHA256=44f06f9195427c7017f5028d0894f57eb216b0a8f7c4eda7ce883732aeb2d0fc
GLAD_LICENSE="MIT (generator); generated loader: CC0-1.0 / Apache-2.0"

JINJA_VERSION=3.1.6
JINJA_URL="https://github.com/pallets/jinja/releases/download/${JINJA_VERSION}/jinja2-${JINJA_VERSION}.tar.gz"
JINJA_SHA256=0137fb05990d35f1275a587e9aee6d56da821fc83491a0fb838183be43f66d6d
JINJA_LICENSE="BSD-3-Clause (build-time only)"

MARKUPSAFE_VERSION=3.0.3
MARKUPSAFE_URL="https://github.com/pallets/markupsafe/releases/download/${MARKUPSAFE_VERSION}/markupsafe-${MARKUPSAFE_VERSION}.tar.gz"
MARKUPSAFE_SHA256=722695808f4b6457b320fdc131280796bdceb04ab50fe1795cd540799ebe1698
MARKUPSAFE_LICENSE="BSD-3-Clause (build-time only)"

FAST_FLOAT_VERSION=8.1.0
FAST_FLOAT_URL="https://github.com/fastfloat/fast_float/archive/v${FAST_FLOAT_VERSION}/fast_float-${FAST_FLOAT_VERSION}.tar.gz"
FAST_FLOAT_SHA256=4bfabb5979716995090ce68dce83f88f99629bc17ae280eae79311c5340143e1
FAST_FLOAT_LICENSE="MIT OR Apache-2.0 OR BSL-1.0"

# Vulkan headers: libplacebo always compiles src/vulkan/stubs.c, which includes
# <vulkan/vulkan.h>, even with -Dvulkan=disabled. Headers only (declarations),
# pinned to the submodule commit of libplacebo v7.387.0 (Vulkan 1.4.345).
VULKAN_HEADERS_VERSION=74d8a6cb930c68ef617b202c3ff3c59d919e086b
VULKAN_HEADERS_URL="https://github.com/KhronosGroup/Vulkan-Headers/archive/${VULKAN_HEADERS_VERSION}/Vulkan-Headers-${VULKAN_HEADERS_VERSION}.tar.gz"
VULKAN_HEADERS_SHA256=75967a8288ff16044a86d1af462b96d2e75c435937efb75f2bb4102ab0382f29
VULKAN_HEADERS_LICENSE="Apache-2.0 OR MIT"

# media_kit's JNI helper (libmediakitandroidhelper.so), loaded by media_kit on
# Android next to libmpv.so. Built from source here so that the whole Android
# .so set comes from this recipe and media_kit's prebuilt libmpv is not needed.
MEDIAKIT_HELPER_VERSION=42054e5d479f39ccbb0ae604862e2bcaf59b74c2
MEDIAKIT_HELPER_URL="https://github.com/media-kit/media-kit-android-helper/archive/${MEDIAKIT_HELPER_VERSION}/media-kit-android-helper-${MEDIAKIT_HELPER_VERSION}.tar.gz"
MEDIAKIT_HELPER_SHA256=53ea71476d1cbc9a5d63d4898c6b027af4ccb6525800c7b42e267e6128652423
MEDIAKIT_HELPER_LICENSE="MIT"

# Packages added by this build (FFmpeg and its deps come from tools/ffmpeg).
PA_MPV_PACKAGES="glad jinja markupsafe fast_float vulkan_headers libplacebo mpv mediakit_helper"
