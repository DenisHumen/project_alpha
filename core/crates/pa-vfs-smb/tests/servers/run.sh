#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Runs the pa-vfs contract suite against Samba in Docker: SMB 3.1.1,
# SMB 2.0.2, mandatory signing, SMB3 encryption, and a 30 ms / 1 % loss
# netem instance for the streaming throughput check.
# Needs Docker with the compose plugin and cargo. libsmb2 comes from
# $PA_SMB2_LIB or tools/libsmb2/out/linux-x86_64 (built in Docker if
# missing). Extra arguments go to `cargo test` (e.g. `-- smb3`).
#
#   core/crates/pa-vfs-smb/tests/servers/run.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../../.." && pwd)"
work="$(mktemp -d)"
# Throwaway credentials of the throwaway containers.
export PA_SMB_USER=kinolune
export PA_SMB_PASS=contract-test
export PA_SMB_DATA="$work/data"
compose=(docker compose -p pa-smb-contract -f "$here/docker-compose.yml")

cleanup() {
    "${compose[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

if [ -z "${PA_SMB2_LIB:-}" ]; then
    lib="$repo/tools/libsmb2/out/linux-x86_64/lib/libsmb2.so.1"
    [ -f "$lib" ] || "$repo/tools/libsmb2/docker-build.sh" linux-x86_64
    export PA_SMB2_LIB="$lib"
fi

# Fixture under /media/контракт next to unrelated data, plus a 192 MiB
# stream file for the throughput check.
mkdir -p "$work/data/media" "$work/data/stream"
cargo run -q -p pa-vfs-smb --example write_fixture -- "$work/data/media/контракт"
echo x >"$work/data/media/other.txt"
head -c $((192 * 1024 * 1024)) /dev/urandom >"$work/data/stream/stream.bin"
PA_SMB_STREAM_SHA256="$(sha256sum "$work/data/stream/stream.bin" | cut -d' ' -f1)"
export PA_SMB_STREAM_SHA256
chmod -R a+rX "$work"

"${compose[@]}" up -d --quiet-pull

wait_port() { # port
    for _ in $(seq 1 120); do
        if (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; then return 0; fi
        sleep 1
    done
    echo "timeout waiting for port $1" >&2
    "${compose[@]}" logs --tail 50 >&2
    return 1
}
for port in 18445 18446 18447; do wait_port "$port"; done
# smbd accepts before it has read the user database; give it a moment.
sleep 2

export PA_SMB_SERVER=127.0.0.1:18445
export PA_SMB_SIGNED_SERVER=127.0.0.1:18446
export PA_SMB_NETEM_SERVER=127.0.0.1:18447
cargo test -p pa-vfs-smb --features server-tests --test contract_servers "$@"
