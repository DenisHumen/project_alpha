# SPDX-License-Identifier: MPL-2.0
# Shared helpers: logging, download with SHA-256 verification, unpack + patch.
# Sourced by build.sh; expects PA_SMB2_ROOT, PA_SMB2_CACHE to be set.
# Same logic as tools/ffmpeg/lib/common.sh, kept separate so the two recipes
# are versioned (and cached in CI) independently.

log() { printf '\033[1;34m[libsmb2-build]\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m[libsmb2-build] error:\033[0m %s\n' "$*" >&2; exit 1; }

# pkg_var <pkg> <FIELD> -> value of e.g. LIBSMB2_VERSION
pkg_var() {
    local name
    name="$(printf '%s' "$1" | tr '[:lower:]' '[:upper:]')_$2"
    printf '%s' "${!name:-}"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1   # macOS
    fi
}

# fetch <pkg>: download the pinned tarball into the cache and verify its hash.
# GitHub tag archives are all called <tag>.tar.gz, so the cache file is named
# after the package and version.
fetch() {
    local pkg="$1" url sha file
    url="$(pkg_var "$pkg" URL)"
    sha="$(pkg_var "$pkg" SHA256)"
    [ -n "$url" ] && [ -n "$sha" ] || die "no URL/SHA256 pinned for '$pkg' in versions.sh"
    file="$PA_SMB2_CACHE/$pkg-$(pkg_var "$pkg" VERSION).tar.gz"
    mkdir -p "$PA_SMB2_CACHE"
    if [ ! -f "$file" ]; then
        log "downloading $url"
        curl -fL --retry 3 --proto '=https' -o "$file.part" "$url" || die "download failed: $url"
        mv "$file.part" "$file"
    fi
    local got
    got="$(sha256_of "$file")"
    if [ "$got" != "$sha" ]; then
        rm -f "$file"
        die "SHA-256 mismatch for $(basename "$file"): expected $sha, got $got"
    fi
    printf '%s' "$file"
}

# unpack <pkg> <dest-parent>: extract the verified tarball and apply our patches
# from patches/<pkg>/*.patch (sorted). Prints the source directory.
unpack() {
    local pkg="$1" parent="$2" file dir
    file="$(fetch "$pkg")"
    mkdir -p "$parent"
    dir="$parent/$pkg"
    rm -rf "$dir"
    mkdir -p "$dir"
    tar -xf "$file" -C "$dir" --strip-components=1
    local p
    for p in "$PA_SMB2_ROOT/patches/$pkg"/*.patch; do
        [ -e "$p" ] || continue
        log "applying patch $(basename "$p") to $pkg"
        patch -d "$dir" -p1 --forward --batch < "$p" >&2 || die "patch failed: $p"
    done
    printf '%s' "$dir"
}

# nproc that works on Linux and macOS, capped by PA_SMB2_JOBS if set.
jobs() {
    if [ -n "${PA_SMB2_JOBS:-}" ]; then printf '%s' "$PA_SMB2_JOBS"; return; fi
    getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2
}
