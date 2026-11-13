#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Write a bootable raw disk image with `bootc install to-disk --via-loopback`.
#   distro/scripts/make-disk.sh SOURCE_IMGREF TARGET_IMGREF [DISK] [SSH_PUBKEY]
#
# SOURCE_IMGREF  where to pull the OS from during install, containers-image syntax,
#                e.g. docker://localhost:5000/alpha-os:dev
# TARGET_IMGREF  what the installed system tracks for `bootc upgrade`,
#                e.g. 10.0.2.2:5000/alpha-os:dev (the host as seen from a QEMU VM)
# DISK           output path (default: distro/.work/disk.raw)
# SSH_PUBKEY     public key injected for root (default: distro/.work/vm_key.pub)
#
# Needs a privileged container (loop devices, mkfs, mounts): the user must be able to run
# `docker run --privileged` (member of the docker group) or rootful podman. No sudo on the
# host itself. Rootless podman cannot do this.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

SOURCE="${1:?source imgref required}"
TARGET="${2:?target imgref required}"
DISK="${3:-${WORK_DIR}/disk.raw}"
PUBKEY="${4:-${WORK_DIR}/vm_key.pub}"

[[ -f "${PUBKEY}" ]] || die "SSH public key not found: ${PUBKEY}"
mkdir -p "$(dirname "${DISK}")"
DISK_DIR="$(cd "$(dirname "${DISK}")" && pwd)"
DISK_NAME="$(basename "${DISK}")"

# The installer container may need to reach a plain-HTTP registry on localhost.
REG_CONF="${WORK_DIR}/local-registry.conf"
cat > "${REG_CONF}" <<EOF
[[registry]]
location = "localhost:${REGISTRY_PORT}"
insecure = true
EOF

rm -f "${DISK}"
truncate -s "${VM_DISK_SIZE}" "${DISK}"

log "Installing ${SOURCE} -> ${DISK} (tracks ${TARGET})"
# The installer runs from the pinned base image; it only needs bootc + bootupd.
nice -n 10 "${ENGINE}" run --rm --privileged --pid=host --net=host \
    --security-opt label=disable \
    -v /dev:/dev \
    -v /var/lib/containers:/var/lib/containers \
    -v "${DISK_DIR}:/output" \
    -v "${PUBKEY}:/run/vm_key.pub:ro" \
    -v "${REG_CONF}:/etc/containers/registries.conf.d/50-alpha-local.conf:ro" \
    "${BASE_IMAGE}" \
    bootc install to-disk --generic-image --via-loopback --wipe --filesystem xfs \
        --source-imgref "${SOURCE}" \
        --target-imgref "${TARGET}" \
        --root-ssh-authorized-keys /run/vm_key.pub \
        "/output/${DISK_NAME}"
log "Disk ready: ${DISK}"
