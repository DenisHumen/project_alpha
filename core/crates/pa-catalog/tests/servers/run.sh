#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# LIB-018 end to end: the catalog scans a Samba share and a WebDAV
# collection in Docker through pa-sources (libsmb2, reqwest), without any
# mount, and streams their files through the loopback proxy.
# Needs Docker with the compose plugin and cargo. libsmb2 comes from
# $PA_SMB2_LIB or tools/libsmb2/out/linux-x86_64 (built in Docker if
# missing). Extra arguments go to `cargo test`.
#
#   core/crates/pa-catalog/tests/servers/run.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../../.." && pwd)"
work="$(mktemp -d)"
# Throwaway credentials of the throwaway containers.
export PA_SERVERS_USER=kinolune
export PA_SERVERS_PASS=catalog-test
export PA_SERVERS_DATA="$work/data" PA_SERVERS_AUTH="$work/auth"
compose=(docker compose -p pa-catalog-servers -f "$here/docker-compose.yml")

cleanup() {
    # Containers and the network only: there are no named volumes.
    "${compose[@]}" down --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

if [ -z "${PA_SMB2_LIB:-}" ]; then
    lib="$repo/tools/libsmb2/out/linux-x86_64/lib/libsmb2.so.1"
    [ -f "$lib" ] || "$repo/tools/libsmb2/docker-build.sh" linux-x86_64
    export PA_SMB2_LIB="$lib"
fi

# A small library: movies, a show, Cyrillic names, sizes the stream test
# compares byte for byte.
mkdir -p "$work/data" "$work/auth"
mk() { mkdir -p "$(dirname "$work/data/$1")"; head -c "$2" /dev/urandom >"$work/data/$1"; }
mk "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv" 700000
mk "Movies/The Matrix 1999.mkv" 300000
mk "Фильмы/Брат (1997)/Брат.1997.BDRip.mkv" 400000
mk "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv" 500000
mk "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv" 500000
printf '%s:{PLAIN}%s\n' "$PA_SERVERS_USER" "$PA_SERVERS_PASS" >"$work/auth/htpasswd"
chmod -R a+rX "$work"

"${compose[@]}" up -d --quiet-pull

wait_smb() {
    for _ in $(seq 1 180); do
        if "${compose[@]}" exec -T samba nc -z 127.0.0.1 445 >/dev/null 2>&1; then return 0; fi
        sleep 1
    done
    echo "timeout waiting for samba" >&2
    "${compose[@]}" logs --tail 50 samba >&2
    return 1
}
wait_http() {
    for _ in $(seq 1 180); do
        if curl -s -o /dev/null "$1"; then return 0; fi
        sleep 1
    done
    echo "timeout waiting for $1" >&2
    "${compose[@]}" logs --tail 50 webdav >&2
    return 1
}
wait_smb
wait_http http://127.0.0.1:18581/dav/

export PA_SERVERS_SMB="smb://127.0.0.1:18545/media"
export PA_SERVERS_WEBDAV="http://127.0.0.1:18581/dav/"
cargo test -p pa-catalog --features server-tests --test servers "$@"
