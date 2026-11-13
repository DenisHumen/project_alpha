#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Run build.sh inside the pinned Debian builder image (no host toolchain needed).
#
#   docker-build.sh <target|fetch|sources|list>
#
# Environment: PA_FF_JOBS (default 4), ANDROID_NDK_VERSION (for android-* targets,
# e.g. r28c; builds a separate image with the NDK), PA_MPV_EXTRA_FLAGS, PA_FF_EXTRA_FLAGS.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tools="$(cd "$root/.." && pwd)"
target="${1:?usage: docker-build.sh <target|fetch|sources|list>}"

image=pa-mpv-builder
build_args=()
case "$target" in
    android-*)
        : "${ANDROID_NDK_VERSION:=r28c}"
        image="pa-mpv-builder-ndk-$ANDROID_NDK_VERSION"
        build_args=(--build-arg "ANDROID_NDK_VERSION=$ANDROID_NDK_VERSION")
        ;;
    windows-*)
        # Same base as tools/ffmpeg/docker (newer mingw-w64 headers, see Dockerfile).
        image=pa-mpv-builder-mingw
        build_args=(--build-arg "BASE_IMAGE=debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2")
        ;;
    macos-*)
        echo "Apple targets need macOS + Xcode: run tools/mpv/build.sh $target on a Mac" >&2
        exit 1
        ;;
esac

docker build "${build_args[@]}" -t "$image" "$root/docker" >&2

# build.sh reuses the FFmpeg recipe from tools/ffmpeg (mounted read-only).
exec docker run --rm \
    -u "$(id -u):$(id -g)" \
    -e HOME=/tmp \
    -e PA_FF_JOBS="${PA_FF_JOBS:-4}" \
    -e PA_MPV_EXTRA_FLAGS="${PA_MPV_EXTRA_FLAGS:-}" \
    -e PA_FF_EXTRA_FLAGS="${PA_FF_EXTRA_FLAGS:-}" \
    -v "$tools/ffmpeg:/src/tools/ffmpeg:ro" \
    -v "$root:/src/tools/mpv" \
    "$image" \
    nice -n 10 /src/tools/mpv/build.sh "$target"
