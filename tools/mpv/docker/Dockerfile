# SPDX-License-Identifier: MPL-2.0
#
# Builder image for the LGPL libmpv build (PLAY-002): the FFmpeg builder
# (tools/ffmpeg/docker) plus mpv's Linux system dependencies (EGL, VA-API,
# libdrm, PulseAudio, ALSA; all linked dynamically, none GPL).
# Apple targets need macOS + Xcode and are not built here.
#
# Ubuntu 24.04 (glibc 2.39), not Debian trixie (glibc 2.41) as for FFmpeg:
# the Linux libmpv is shipped inside the app, so it must not need a newer
# glibc than the ubuntu-24.04 runner that builds the Flutter app (R01-REL).
#
#   docker build -t pa-mpv-builder tools/mpv/docker
#   docker build --build-arg ANDROID_NDK_VERSION=r28c -t pa-mpv-builder-ndk tools/mpv/docker

FROM ubuntu:24.04@sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bash ca-certificates curl xz-utils bzip2 unzip patch file binutils \
        build-essential pkgconf meson ninja-build nasm python3 \
        zlib1g-dev libva-dev libdrm-dev libegl-dev libpulse-dev libasound2-dev \
        gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 mingw-w64-x86-64-dev \
    && rm -rf /var/lib/apt/lists/*

# Optional Android NDK (~2 GB unpacked). Empty = not installed.
ARG ANDROID_NDK_VERSION=""
RUN if [ -n "$ANDROID_NDK_VERSION" ]; then \
        curl -fL --proto '=https' -o /tmp/ndk.zip \
            "https://dl.google.com/android/repository/android-ndk-${ANDROID_NDK_VERSION}-linux.zip" \
        && unzip -q /tmp/ndk.zip -d /opt && rm /tmp/ndk.zip \
        && mv "/opt/android-ndk-${ANDROID_NDK_VERSION}" /opt/android-ndk; \
    fi
ENV ANDROID_NDK_HOME=/opt/android-ndk

WORKDIR /src/tools/mpv
