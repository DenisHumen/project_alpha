#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Run build.sh inside the pinned Ubuntu builder image (no host toolchain needed).
#
#   docker-build.sh <target|fetch|sources|list>
#
# Environment: PA_SMB2_JOBS (default 4), ANDROID_NDK_VERSION (for android-*
# targets, e.g. r28c; builds a separate image with the NDK), PA_SMB2_EXTRA_FLAGS.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="${1:?usage: docker-build.sh <target|fetch|sources|list>}"

image=pa-libsmb2-builder
build_args=()
case "$target" in
    android-*)
        : "${ANDROID_NDK_VERSION:=r28c}"
        image="pa-libsmb2-builder-ndk-$ANDROID_NDK_VERSION"
        build_args=(--build-arg "ANDROID_NDK_VERSION=$ANDROID_NDK_VERSION")
        ;;
    macos-*|ios-*|tvos-*|xcframework)
        echo "Apple targets need macOS + Xcode: run tools/libsmb2/build.sh $target on a Mac" >&2
        exit 1
        ;;
esac

docker build "${build_args[@]}" -t "$image" "$root/docker" >&2

exec docker run --rm \
    -u "$(id -u):$(id -g)" \
    -e HOME=/tmp \
    -e PA_SMB2_JOBS="${PA_SMB2_JOBS:-4}" \
    -e PA_SMB2_EXTRA_FLAGS="${PA_SMB2_EXTRA_FLAGS:-}" \
    -v "$root:/src" \
    "$image" \
    nice -n 10 /src/build.sh "$target"
