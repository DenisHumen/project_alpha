#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Runs the pa-vfs contract suite against real WebDAV servers in Docker:
# nginx + dav-ext, Apache mod_dav, rclone serve webdav and Nextcloud.
# Needs Docker with the compose plugin and cargo. Extra arguments go to
# `cargo test` (e.g. `-- nginx` to run one server).
#
#   core/crates/pa-vfs-webdav/tests/servers/run.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
# Throwaway credentials of the throwaway containers.
export PA_WEBDAV_USER=kinolune
export PA_WEBDAV_PASS=contract-test
export PA_WEBDAV_DATA="$work/data" PA_WEBDAV_AUTH="$work/auth"
compose=(docker compose -p pa-webdav-contract -f "$here/docker-compose.yml")

cleanup() {
    "${compose[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

# Fixture under /media/контракт next to unrelated data.
mkdir -p "$work/data/media" "$work/auth"
cargo run -q -p pa-vfs-webdav --example write_fixture -- "$work/data/media/контракт"
echo x >"$work/data/media/other.txt"
printf '%s:{PLAIN}%s\n' "$PA_WEBDAV_USER" "$PA_WEBDAV_PASS" >"$work/auth/htpasswd"
ha1="$(printf '%s:kinolune:%s' "$PA_WEBDAV_USER" "$PA_WEBDAV_PASS" | md5sum | cut -d' ' -f1)"
printf '%s:kinolune:%s\n' "$PA_WEBDAV_USER" "$ha1" >"$work/auth/digest"
chmod -R a+rX "$work"

"${compose[@]}" up -d --quiet-pull

wait_http() { # url
    for _ in $(seq 1 180); do
        if curl -s -o /dev/null "$1"; then return 0; fi
        sleep 1
    done
    echo "timeout waiting for $1" >&2
    "${compose[@]}" logs --tail 50 >&2
    return 1
}
for port in 18081 18082 18083 18084; do wait_http "http://127.0.0.1:$port/"; done

# Nextcloud installs itself on first start; then the fixture is copied into
# the user's files and indexed.
for _ in $(seq 1 180); do
    if "${compose[@]}" exec -T -u www-data nextcloud php occ status 2>/dev/null | grep -q 'installed: true'; then
        break
    fi
    sleep 2
done
"${compose[@]}" cp "$work/data/media" "nextcloud:/var/www/html/data/$PA_WEBDAV_USER/files/"
"${compose[@]}" exec -T nextcloud chown -R www-data:www-data "/var/www/html/data/$PA_WEBDAV_USER/files"
"${compose[@]}" exec -T -u www-data nextcloud php occ files:scan "$PA_WEBDAV_USER" >/dev/null

export PA_WEBDAV_NGINX=http://127.0.0.1:18081/dav/
export PA_WEBDAV_APACHE=http://127.0.0.1:18082/dav/
export PA_WEBDAV_RCLONE=http://127.0.0.1:18083/
export PA_WEBDAV_NEXTCLOUD="http://127.0.0.1:18084/remote.php/dav/files/$PA_WEBDAV_USER/"
cargo test -p pa-vfs-webdav --features server-tests --test contract_servers "$@"
