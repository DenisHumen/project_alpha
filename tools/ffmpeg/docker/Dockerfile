# SPDX-License-Identifier: MPL-2.0
#
# Builder image for the LGPL FFmpeg build (PLAY-001): Linux x86_64 natively,
# Windows x86_64 via mingw-w64, Android via the NDK (optional, large).
# Apple targets need macOS + Xcode and are not built here.
#
#   docker build -t pa-ffmpeg-builder tools/ffmpeg/docker
#   docker build --build-arg ANDROID_NDK_VERSION=r28c -t pa-ffmpeg-builder-ndk tools/ffmpeg/docker

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bash ca-certificates curl xz-utils bzip2 unzip patch file \
        build-essential pkgconf meson ninja-build nasm python3 \
        zlib1g-dev libva-dev libdrm-dev \
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

WORKDIR /src
