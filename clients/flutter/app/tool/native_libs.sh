#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Native LGPL libraries for local and dev builds of the app (LIB-019): our
# libmpv + FFmpeg (tools/mpv) and libsmb2 (tools/libsmb2), the same ones the
# release ships. Without libmpv.so in the APK the app stops at start
# (media_kit loads it in its plugin); without libsmb2 SMB sources fail.
#
#   native_libs.sh fetch [--any-branch]
#       Download the newest unexpired CI artifacts (libmpv.yml, libsmb2.yml or
#       release-app.yml runs; `main` only unless --any-branch) with the gh CLI
#       into the cache, then `install`. Artifacts live 7 days; when none is
#       left, run `gh workflow run libmpv.yml` and `gh workflow run
#       libsmb2.yml` (on main) and fetch again after they finish, or `build`.
#
#   native_libs.sh build [<target>...]
#       Build from the recipes: tools/libsmb2 (about a minute per target) and
#       tools/mpv (FFmpeg + mpv, about an hour per target) in Docker, or with
#       a local NDK for android-* when ANDROID_NDK_HOME is set. Default
#       targets: android-arm64-v8a android-armeabi-v7a android-x86_64
#       linux-x86_64. Then `install`.
#
#   native_libs.sh install [--linux-bundle <flutter-linux-bundle>]
#       Fill the APK jniLibs (packages/media_kit_libs_android_video) with every
#       Android ABI found in the cache. With --linux-bundle, also put libmpv
#       and libsmb2 into a built Linux bundle (build/linux/x64/<mode>/bundle;
#       needs patchelf), as packaging/app/linux/package.sh does.
#
#   native_libs.sh env
#       Print exports for `flutter run -d linux` against the cache:
#       PKG_CONFIG_PATH (our libmpv instead of libmpv-dev), LIBMPV_LIBRARY_PATH
#       and PA_SMB2_LIB.
#
#   native_libs.sh status
#       What the cache and the jniLibs hold.
#
# The cache is $PA_NATIVE_DIR (default ~/.cache/kinolune-native), one
# directory per artifact, named like the CI artifacts: libmpv-<target>,
# libsmb2-<target>. GH_REPO overrides the repository (default: origin's).
set -euo pipefail

app="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo="$(cd "$app/../../.." && pwd)"
jni="$repo/clients/flutter/packages/media_kit_libs_android_video/android/src/main/jniLibs"
cache="${PA_NATIVE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/kinolune-native}"
ABIS=(arm64-v8a armeabi-v7a x86_64)
DEFAULT_TARGETS=(android-arm64-v8a android-armeabi-v7a android-x86_64 linux-x86_64)

die() { printf 'native_libs.sh: error: %s\n' "$*" >&2; exit 1; }
log() { printf 'native_libs.sh: %s\n' "$*" >&2; }

usage() { sed -n '4,40p' "$0"; exit 2; }

# --- fetch -------------------------------------------------------------------

gh_repo() {
    if [ -n "${GH_REPO:-}" ]; then echo "$GH_REPO"; return; fi
    git -C "$repo" remote get-url origin | sed -E 's#^(git@github.com:|https://github.com/)##; s#\.git$##'
}

# Newest unexpired artifact <name>: prints "<id> <branch> <run id>".
find_artifact() {
    local r="$1" name="$2" any="$3"
    gh api "repos/$r/actions/artifacts?name=$name&per_page=30" \
        --jq '.artifacts[] | select(.expired | not) | "\(.id) \(.workflow_run.head_branch) \(.workflow_run.id)"' \
        | while read -r id branch run; do
            if [ "$any" = 1 ] || [ "$branch" = main ]; then echo "$id $branch $run"; break; fi
        done
}

fetch_one() { # repo name any
    local r="$1" name="$2" any="$3" found id branch run tmp
    found="$(find_artifact "$r" "$name" "$any")"
    if [ -z "$found" ]; then
        log "no unexpired artifact $name$([ "$any" = 1 ] || echo ' on main')"
        return 1
    fi
    read -r id branch run <<<"$found"
    tmp="$(mktemp -d)"
    gh api "repos/$r/actions/artifacts/$id/zip" > "$tmp/a.zip"
    rm -rf "${cache:?}/$name"
    mkdir -p "$cache/$name"
    # Plain extraction into its own directory; nothing in it is executed.
    unzip -q "$tmp/a.zip" -d "$cache/$name"
    rm -rf "$tmp"
    log "$name <- run $run ($branch)"
}

cmd_fetch() {
    local any=0
    [ "${1:-}" = --any-branch ] && any=1
    command -v gh >/dev/null || die "the GitHub CLI (gh) is required: https://cli.github.com"
    command -v unzip >/dev/null || die "unzip is required"
    local r missing=0 t
    r="$(gh_repo)"
    mkdir -p "$cache"
    for t in "${DEFAULT_TARGETS[@]}"; do
        fetch_one "$r" "libmpv-$t" "$any" || missing=1
        fetch_one "$r" "libsmb2-$t" "$any" || missing=1
    done
    if [ "$missing" = 1 ]; then
        log "some artifacts are missing: start the workflows on main and fetch again when they finish:"
        log "  gh workflow run libmpv.yml --ref main && gh workflow run libsmb2.yml --ref main"
        log "or build them: $0 build"
    fi
    cmd_install
}

# --- build -------------------------------------------------------------------

cmd_build() {
    local targets=("$@") t
    [ "${#targets[@]}" -gt 0 ] || targets=("${DEFAULT_TARGETS[@]}")
    mkdir -p "$cache"
    for t in "${targets[@]}"; do
        case "$t" in
            android-*|linux-x86_64) ;;
            *) die "unsupported target $t (android-<abi> or linux-x86_64)" ;;
        esac
        log "libsmb2 $t"
        if [[ $t == android-* ]] && [ -n "${ANDROID_NDK_HOME:-}" ]; then
            "$repo/tools/libsmb2/build.sh" "$t"
        else
            "$repo/tools/libsmb2/docker-build.sh" "$t"
        fi
        rm -rf "${cache:?}/libsmb2-$t" && cp -a "$repo/tools/libsmb2/out/$t" "$cache/libsmb2-$t"
        log "libmpv $t (FFmpeg + mpv: this takes a while)"
        if [[ $t == android-* ]] && [ -n "${ANDROID_NDK_HOME:-}" ]; then
            "$repo/tools/mpv/build.sh" "$t"
        else
            "$repo/tools/mpv/docker-build.sh" "$t"
        fi
        rm -rf "${cache:?}/libmpv-$t" && cp -a "$repo/tools/mpv/out/$t" "$cache/libmpv-$t"
    done
    cmd_install
}

# --- install -----------------------------------------------------------------

cmd_install() {
    local bundle="" abi mpv=() smb=()
    if [ "${1:-}" = --linux-bundle ]; then
        bundle="${2:?--linux-bundle needs a directory}"
    fi
    for abi in "${ABIS[@]}"; do
        [ -d "$cache/libmpv-android-$abi/lib" ] && mpv+=("$cache/libmpv-android-$abi")
        [ -d "$cache/libsmb2-android-$abi/lib" ] && smb+=("$cache/libsmb2-android-$abi")
    done
    if [ "${#mpv[@]}" -gt 0 ]; then
        "$repo/tools/mpv/bundle.sh" android "$jni" "${mpv[@]}"
    else
        log "no Android libmpv in $cache: APKs built now stop at start"
    fi
    if [ "${#smb[@]}" -gt 0 ]; then
        "$repo/tools/libsmb2/bundle.sh" android "$jni" "${smb[@]}"
    else
        log "no Android libsmb2 in $cache: SMB sources will fail in APKs built now"
    fi
    if [ -n "$bundle" ]; then
        [ -d "$cache/libmpv-linux-x86_64" ] || die "no libmpv-linux-x86_64 in $cache"
        [ -d "$cache/libsmb2-linux-x86_64" ] || die "no libsmb2-linux-x86_64 in $cache"
        "$repo/tools/mpv/bundle.sh" linux "$cache/libmpv-linux-x86_64" "$bundle"
        "$repo/tools/libsmb2/bundle.sh" linux "$cache/libsmb2-linux-x86_64" "$bundle"
    fi
    cmd_status
}

cmd_env() {
    local mpv="$cache/libmpv-linux-x86_64" smb="$cache/libsmb2-linux-x86_64"
    [ -d "$mpv" ] && printf 'export PKG_CONFIG_PATH=%q\nexport LIBMPV_LIBRARY_PATH=%q\n' \
        "$mpv/lib/pkgconfig" "$mpv/lib/libmpv.so.2"
    [ -d "$smb" ] && printf 'export PA_SMB2_LIB=%q\n' "$smb/lib/libsmb2.so.1"
    return 0
}

cmd_status() {
    local abi f have
    printf 'cache: %s\n' "$cache"
    for f in "$cache"/lib*-*; do [ -d "$f" ] && printf '  %s\n' "$(basename "$f")"; done
    printf 'jniLibs: %s\n' "$jni"
    for abi in "${ABIS[@]}"; do
        have="-"
        [ -d "$jni/$abi" ] && have="$(cd "$jni/$abi" && ls -- *.so 2>/dev/null | tr '\n' ' ')"
        printf '  %s: %s\n' "$abi" "${have:--}"
    done
}

case "${1:-}" in
    fetch) shift; cmd_fetch "$@" ;;
    build) shift; cmd_build "$@" ;;
    install) shift; cmd_install "$@" ;;
    env) cmd_env ;;
    status) cmd_status ;;
    *) usage ;;
esac
