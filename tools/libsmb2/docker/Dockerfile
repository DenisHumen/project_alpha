# SPDX-License-Identifier: MPL-2.0
#
# Builder image for the LGPL libsmb2 build (NET-002): Linux x86_64 natively,
# Windows x86_64 via mingw-w64, Android via the NDK (optional, large).
# Apple targets need macOS + Xcode and are not built here.
#
# Ubuntu 24.04 (glibc 2.39), as for libmpv: the Linux library is shipped
# inside the app, so it must not need a newer glibc than the runner that
# builds the Flutter app (R01-REL).
#
#   docker build -t pa-libsmb2-builder tools/libsmb2/docker
#   docker build --build-arg ANDROID_NDK_VERSION=r28c -t pa-libsmb2-builder-ndk tools/libsmb2/docker

FROM ubuntu:24.04@sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bash ca-certificates curl xz-utils unzip patch file binutils \
        build-essential cmake ninja-build \
        gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 mingw-w64-x86-64-dev \
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
